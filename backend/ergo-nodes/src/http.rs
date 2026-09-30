//! HTTP steps: `http.download` fetches data, `http.request` calls APIs.
//!
//! Both share one client setup, and the same URL, query, header and
//! authentication options.
//!
//! Download: small responses come out inline as `{ status, headers,
//! content_type, body }`. Larger ones stream to a temp file (deleted when the
//! run ends), and the output carries `path` and `size` instead of `body`.
//!
//! Request: any method, with a JSON, form or raw body. The response is read
//! inline (up to a size limit) and parsed as JSON when it is JSON.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use ergo_core::{
    ErrorKind, Field, FieldType, NodeError, NodeExecutor, NodeKind, NodeOutput, NodeSchema, RunCtx,
};
use futures_util::StreamExt;
use serde_json::{Map, Value, json};
use tokio::io::AsyncWriteExt;

use crate::{object, string};

/// Responses up to this size stay inline; bigger ones go to a temp file.
const INLINE_LIMIT: usize = 1024 * 1024;
const DEFAULT_MAX_MB: u64 = 25;
const DEFAULT_REQUEST_MAX_MB: u64 = 5;
const DEFAULT_TIMEOUT_S: u64 = 30;
const METHODS: [&str; 7] = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

/// A rustls client with the `ring` provider and Mozilla's roots bundled in,
/// so it needs no system certificate store and cross-compiles cleanly.
fn tls_config() -> rustls::ClientConfig {
    let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("ring supports the default TLS versions")
        .with_root_certificates(roots)
        .with_no_client_auth()
}

/// One client that follows redirects and one that doesn't.
struct Clients {
    follow: reqwest::Client,
    no_follow: reqwest::Client,
}

impl Clients {
    fn new() -> Self {
        let build = |redirects: reqwest::redirect::Policy| {
            reqwest::Client::builder()
                .tls_backend_preconfigured(tls_config())
                .redirect(redirects)
                .user_agent(concat!("ergo/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("HTTP client")
        };
        Self {
            follow: build(reqwest::redirect::Policy::limited(10)),
            no_follow: build(reqwest::redirect::Policy::none()),
        }
    }

    /// The request described by the shared options: URL with query
    /// parameters, headers, authentication, timeout and redirects.
    fn prepare(
        &self,
        cfg: &Value,
        method: reqwest::Method,
    ) -> Result<(reqwest::RequestBuilder, url::Url), NodeError> {
        let raw_url = cfg["url"].as_str().unwrap_or_default().trim();
        let mut url = url::Url::parse(raw_url).map_err(|e| {
            NodeError::new(ErrorKind::Config, format!("URL is not valid: {e}"))
                .details(json!({ "url": raw_url }))
        })?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(NodeError::new(
                ErrorKind::Config,
                "URL must start with http:// or https://",
            ));
        }
        let query = pairs(&cfg["query"]);
        if !query.is_empty() {
            let mut q = url.query_pairs_mut();
            for (k, v) in &query {
                q.append_pair(k, v);
            }
        }
        let client = if cfg["follow_redirects"].as_bool().unwrap_or(true) {
            &self.follow
        } else {
            &self.no_follow
        };
        let timeout = Duration::from_secs(
            cfg["timeout_s"]
                .as_u64()
                .unwrap_or(DEFAULT_TIMEOUT_S)
                .clamp(1, 600),
        );
        let mut req = client.request(method, url.clone()).timeout(timeout);
        for (k, v) in pairs(&cfg["headers"]) {
            req = req.header(k, v);
        }
        let auth = &cfg["auth"];
        req = match auth["type"].as_str().unwrap_or("none") {
            "bearer" => req.bearer_auth(auth["token"].as_str().unwrap_or_default()),
            "basic" => req.basic_auth(
                auth["username"].as_str().unwrap_or_default(),
                auth["password"].as_str(),
            ),
            "header" => match auth["header"]
                .as_str()
                .map(str::trim)
                .filter(|h| !h.is_empty())
            {
                Some(h) => req.header(h, auth["value"].as_str().unwrap_or_default()),
                None => req,
            },
            _ => req,
        };
        Ok((req, url))
    }
}

fn send_error(e: reqwest::Error, url: &url::Url) -> NodeError {
    let kind = if e.is_timeout() {
        ErrorKind::Timeout
    } else {
        ErrorKind::Network
    };
    NodeError::new(kind, e.to_string()).details(json!({ "url": url.as_str() }))
}

fn stream_error(e: reqwest::Error) -> NodeError {
    let kind = if e.is_timeout() {
        ErrorKind::Timeout
    } else {
        ErrorKind::Network
    };
    NodeError::new(kind, format!("download interrupted: {e}"))
}

fn too_large(url: &str, max: usize) -> NodeError {
    NodeError::new(
        ErrorKind::Other,
        format!("response is larger than the {} MB limit", max / 1024 / 1024),
    )
    .details(json!({ "url": url, "limit_bytes": max }))
}

/// `[{ key, value }]` lists from the editor.
fn pairs(v: &Value) -> Vec<(String, String)> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let k = p["key"].as_str()?.trim();
            (!k.is_empty()).then(|| {
                (
                    k.to_string(),
                    p["value"].as_str().unwrap_or_default().to_string(),
                )
            })
        })
        .collect()
}

fn human(bytes: usize) -> String {
    match bytes {
        b if b < 1024 => format!("{b} B"),
        b if b < 1024 * 1024 => format!("{:.1} KB", b as f64 / 1024.0),
        b => format!("{:.1} MB", b as f64 / 1024.0 / 1024.0),
    }
}

fn response_headers(resp: &reqwest::Response) -> (String, Map<String, Value>) {
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let headers = resp
        .headers()
        .iter()
        .filter_map(|(k, v)| Some((k.to_string(), Value::from(v.to_str().ok()?))))
        .collect();
    (content_type, headers)
}

fn validate_url(config: &Value) -> Vec<String> {
    match config["url"].as_str() {
        Some(u) if !u.contains("{{") && !u.trim().is_empty() => match url::Url::parse(u.trim()) {
            Ok(p) if matches!(p.scheme(), "http" | "https") => vec![],
            Ok(_) => vec!["URL must start with http:// or https://".into()],
            Err(e) => vec![format!("URL is not valid: {e}")],
        },
        _ => vec![],
    }
}

/// The options both HTTP steps share.
fn common_fields(schema: NodeSchema, max_mb: u64) -> NodeSchema {
    schema
        .field(Field::new("query", "Query parameters", FieldType::Text))
        .field(Field::new("headers", "Headers", FieldType::Text))
        .field(Field::new("auth", "Authentication", FieldType::Text))
        .field(
            Field::new("timeout_s", "Timeout (seconds)", FieldType::Number)
                .default(DEFAULT_TIMEOUT_S),
        )
        .field(Field::new("follow_redirects", "Follow redirects", FieldType::Bool).default(true))
        .field(Field::new("max_mb", "Size limit (MB)", FieldType::Number).default(max_mb))
}

pub struct HttpDownload {
    clients: Clients,
}

impl HttpDownload {
    pub fn new() -> Self {
        Self {
            clients: Clients::new(),
        }
    }
}

impl Default for HttpDownload {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NodeExecutor for HttpDownload {
    fn schema(&self) -> NodeSchema {
        let schema = NodeSchema::new("http.download", NodeKind::Data, "Download")
            .description("Fetches data from a URL (HTTP GET).")
            .field(
                Field::new("url", "URL", FieldType::Template)
                    .required()
                    .placeholder("https://example.com/feed.xml"),
            );
        common_fields(schema, DEFAULT_MAX_MB)
            .ports(vec!["out"])
            .output(object(json!({
                "status": { "type": "integer", "description": "HTTP status code" },
                "content_type": string("Content type of the response"),
                "body": string("The response text (small responses)"),
                "path": string("Temp file with the response (large responses)"),
                "size": { "type": "integer", "description": "Size in bytes" },
                "url": string("The final URL, after redirects"),
                "headers": { "type": "object", "description": "Response headers" },
            })))
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        validate_url(config)
    }

    async fn run(&self, cfg: &Value, _: &Value, ctx: &RunCtx<'_>) -> Result<NodeOutput, NodeError> {
        let (req, url) = self.clients.prepare(cfg, reqwest::Method::GET)?;
        let max = cfg["max_mb"]
            .as_u64()
            .unwrap_or(DEFAULT_MAX_MB)
            .clamp(1, 1024) as usize
            * 1024
            * 1024;

        let resp = req.send().await.map_err(|e| send_error(e, &url))?;
        let status = resp.status();
        if resp.url() != &url {
            ctx.log(format!("redirected to {}", resp.url()));
        }
        let final_url = resp.url().to_string();
        let (content_type, headers) = response_headers(&resp);

        let mut stream = resp.bytes_stream();
        let mut buf: Vec<u8> = Vec::new();
        let mut file: Option<(tokio::fs::File, std::path::PathBuf)> = None;
        let mut size = 0usize;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(stream_error)?;
            size += chunk.len();
            if size > max {
                return Err(too_large(&final_url, max));
            }
            match &mut file {
                Some((f, _)) => f.write_all(&chunk).await.map_err(io_error)?,
                None if buf.len() + chunk.len() > INLINE_LIMIT && status.is_success() => {
                    // Too big to keep inline: move what we have to a temp file.
                    let dir = ctx.temp_dir.ok_or_else(|| {
                        NodeError::new(ErrorKind::Other, "no temp folder for large downloads")
                    })?;
                    let path = dir.join(format!(
                        "{}-{}.download",
                        ctx.run_id,
                        uuid::Uuid::new_v4().simple()
                    ));
                    let mut f = tokio::fs::File::create(&path).await.map_err(io_error)?;
                    f.write_all(&buf).await.map_err(io_error)?;
                    f.write_all(&chunk).await.map_err(io_error)?;
                    buf.clear();
                    file = Some((f, path));
                }
                None => buf.extend_from_slice(&chunk),
            }
        }

        if !status.is_success() {
            let preview = String::from_utf8_lossy(&buf[..buf.len().min(2048)]).to_string();
            return Err(
                NodeError::new(ErrorKind::HttpStatus, format!("{status}")).details(json!({
                    "status": status.as_u16(),
                    "url": final_url,
                    "body": preview,
                })),
            );
        }

        let mut out = json!({
            "status": status.as_u16(),
            "url": final_url,
            "content_type": content_type,
            "headers": headers,
            "size": size,
        });
        match file {
            Some((mut f, path)) => {
                f.flush().await.map_err(io_error)?;
                ctx.log(format!(
                    "GET {final_url} → {} · {} · {content_type}, streamed to {}",
                    status.as_u16(),
                    human(size),
                    path.display()
                ));
                out["path"] = Value::from(path.to_string_lossy().to_string());
            }
            None => {
                ctx.log(format!(
                    "GET {final_url} → {} · {} · {content_type}",
                    status.as_u16(),
                    human(size)
                ));
                out["body"] = Value::from(String::from_utf8_lossy(&buf).to_string());
            }
        }
        Ok(NodeOutput::out(out))
    }
}

fn io_error(e: std::io::Error) -> NodeError {
    NodeError::new(ErrorKind::Other, format!("temp file: {e}"))
}

/// Calls an API: any method, headers, auth, and a JSON, form or raw body.
pub struct HttpRequest {
    clients: Clients,
}

impl HttpRequest {
    pub fn new() -> Self {
        Self {
            clients: Clients::new(),
        }
    }
}

impl Default for HttpRequest {
    fn default() -> Self {
        Self::new()
    }
}

fn select(options: &[&str]) -> FieldType {
    FieldType::Select {
        options: options.iter().map(|s| s.to_string()).collect(),
    }
}

#[async_trait]
impl NodeExecutor for HttpRequest {
    fn schema(&self) -> NodeSchema {
        let schema = NodeSchema::new("http.request", NodeKind::Action, "Web request")
            .description("Calls a web API or webhook with any method, headers and body.")
            .field(Field::new("method", "Method", select(&METHODS)).default("POST"))
            .field(
                Field::new("url", "URL", FieldType::Template)
                    .required()
                    .placeholder("https://api.example.com/v1/items"),
            )
            .field(
                Field::new(
                    "body_type",
                    "Body",
                    select(&["none", "json", "form", "text"]),
                )
                .default("json"),
            )
            .field(
                Field::new("body", "Body", FieldType::Template)
                    .placeholder("{ \"title\": {{ input.title | tojson }} }"),
            )
            .field(Field::new("form", "Form fields", FieldType::Text))
            .field(
                Field::new("content_type", "Content type", FieldType::Text).default("text/plain"),
            );
        common_fields(schema, DEFAULT_REQUEST_MAX_MB)
            .field(
                Field::new("fail_on_status", "Fail on HTTP errors", FieldType::Bool)
                    .default(true)
                    .help("Off: 4xx and 5xx responses are output like any other."),
            )
            .field(
                Field::new(
                    "response",
                    "Read the response as",
                    select(&["auto", "json", "text"]),
                )
                .default("auto"),
            )
            .ports(vec!["out"])
            .output(object(json!({
                "status": { "type": "integer", "description": "HTTP status code" },
                "ok": { "type": "boolean", "description": "True for 2xx statuses" },
                "json": { "type": "object", "description": "The response parsed as JSON" },
                "body": string("The response text"),
                "content_type": string("Content type of the response"),
                "size": { "type": "integer", "description": "Size in bytes" },
                "url": string("The final URL, after redirects"),
                "headers": { "type": "object", "description": "Response headers" },
            })))
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        let mut problems = validate_url(config);
        let method = config["method"].as_str().unwrap_or("GET");
        if !METHODS.contains(&method) {
            problems.push(format!("Unknown method {method}"));
        }
        // A JSON body with no template tags can be checked right away.
        if config["body_type"].as_str() == Some("json")
            && let Some(body) = config["body"]
                .as_str()
                .filter(|b| !b.trim().is_empty() && !b.contains("{{") && !b.contains("{%"))
            && let Err(e) = serde_json::from_str::<Value>(body)
        {
            problems.push(format!("Body is not valid JSON: {e}"));
        }
        problems
    }

    async fn run(&self, cfg: &Value, _: &Value, ctx: &RunCtx<'_>) -> Result<NodeOutput, NodeError> {
        let method_name = cfg["method"].as_str().unwrap_or("GET").to_ascii_uppercase();
        let method = reqwest::Method::from_bytes(method_name.as_bytes()).map_err(|_| {
            NodeError::new(ErrorKind::Config, format!("unknown method {method_name}"))
        })?;
        let (mut req, url) = self.clients.prepare(cfg, method)?;
        let body = cfg["body"].as_str().unwrap_or_default();
        match cfg["body_type"].as_str().unwrap_or("none") {
            "json" if !body.trim().is_empty() => {
                // Checked after rendering, so a template that produced broken
                // JSON fails here rather than at the other end.
                let value: Value = serde_json::from_str(body).map_err(|e| {
                    NodeError::new(
                        ErrorKind::Template,
                        format!("the body isn't valid JSON after filling in the template: {e}"),
                    )
                    .details(json!({ "body": body, "line": e.line(), "column": e.column() }))
                })?;
                req = req.json(&value);
            }
            "form" => req = req.form(&pairs(&cfg["form"])),
            "text" if !body.is_empty() => {
                let ct = cfg["content_type"]
                    .as_str()
                    .map(str::trim)
                    .filter(|c| !c.is_empty())
                    .unwrap_or("text/plain");
                req = req
                    .header(reqwest::header::CONTENT_TYPE, ct)
                    .body(body.to_string());
            }
            _ => {}
        }
        let max = cfg["max_mb"]
            .as_u64()
            .unwrap_or(DEFAULT_REQUEST_MAX_MB)
            .clamp(1, 100) as usize
            * 1024
            * 1024;

        let resp = req.send().await.map_err(|e| send_error(e, &url))?;
        let status = resp.status();
        if resp.url() != &url {
            ctx.log(format!("redirected to {}", resp.url()));
        }
        let final_url = resp.url().to_string();
        let (content_type, headers) = response_headers(&resp);
        let mut stream = resp.bytes_stream();
        let mut buf: Vec<u8> = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(stream_error)?;
            if buf.len() + chunk.len() > max {
                return Err(too_large(&final_url, max));
            }
            buf.extend_from_slice(&chunk);
        }
        let text = String::from_utf8_lossy(&buf).to_string();
        ctx.log(format!(
            "{method_name} {final_url} → {} · {}{}",
            status.as_u16(),
            human(buf.len()),
            if content_type.is_empty() {
                String::new()
            } else {
                format!(" · {content_type}")
            }
        ));

        if !status.is_success() && cfg["fail_on_status"].as_bool().unwrap_or(true) {
            return Err(
                NodeError::new(ErrorKind::HttpStatus, format!("{status}")).details(json!({
                    "status": status.as_u16(),
                    "url": final_url,
                    "body": text.chars().take(2048).collect::<String>(),
                })),
            );
        }

        let mut out = json!({
            "status": status.as_u16(),
            "ok": status.is_success(),
            "url": final_url,
            "content_type": content_type,
            "headers": headers,
            "size": buf.len(),
            "body": text,
        });
        let looks_json = {
            let ct = content_type.split(';').next().unwrap_or_default().trim();
            ct == "application/json" || ct.ends_with("+json")
        };
        match cfg["response"].as_str().unwrap_or("auto") {
            "json" => {
                let parsed: Value = serde_json::from_slice(&buf).map_err(|e| {
                    NodeError::new(
                        ErrorKind::Parse,
                        format!("the response isn't valid JSON: {e}"),
                    )
                    .details(json!({ "body": text.chars().take(2048).collect::<String>() }))
                })?;
                out["json"] = parsed;
            }
            "auto" if looks_json && !buf.is_empty() => {
                // Auto is lenient: a broken JSON response still comes out as text.
                match serde_json::from_slice::<Value>(&buf) {
                    Ok(parsed) => out["json"] = parsed,
                    Err(e) => ctx.log(format!("response says JSON but doesn't parse: {e}")),
                }
            }
            _ => {}
        }
        Ok(NodeOutput::out(out))
    }
}
