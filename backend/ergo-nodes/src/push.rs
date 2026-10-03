//! Web push: notifications sent straight to browsers that turned them on in
//! ergo, with no Home Assistant notifier in between. `push.send` is the step.
//!
//! The browser's push service (Google's, Mozilla's, Apple's) delivers the
//! message; ergo signs each request with its VAPID key and encrypts the
//! payload for that browser alone (RFC 8030, 8291, 8292).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use base64ct::{Base64UrlUnpadded, Encoding};
use ergo_core::{
    ErrorKind, Field, FieldType, NodeError, NodeExecutor, NodeKind, NodeOutput, NodeSchema, RunCtx,
};
use serde_json::{Value, json};
use web_push_native::jwt_simple::algorithms::{ECDSAP256PublicKeyLike, ES256KeyPair};
use web_push_native::{Auth, WebPushBuilder, p256};

use crate::{object, string};

/// Who the pushes say they come from; push services may contact it.
const CONTACT: &str = "https://github.com/dimitrmo/ergo";
/// How long a push service keeps trying to deliver a message.
const TTL: Duration = Duration::from_secs(12 * 3600);
const SEND_TIMEOUT: Duration = Duration::from_secs(15);
/// Every web push notification's title; the step's message is its text.
pub const TITLE: &str = "Ergo";
/// RFC 8030's urgencies: how soon a device on battery should wake up for it.
pub const URGENCIES: [&str; 4] = ["very-low", "low", "normal", "high"];

/// A browser that turned on notifications.
#[derive(Debug, Clone, PartialEq)]
pub struct PushSubscription {
    pub id: String,
    /// What the person called it, e.g. "Work laptop".
    pub name: String,
    pub endpoint: String,
    /// The browser's public key and auth secret, base64url as the browser gives them.
    pub p256dh: String,
    pub auth: String,
}

/// Where subscriptions live; implemented by the binary's database.
pub trait PushStore: Send + Sync {
    fn subscriptions(&self) -> Result<Vec<PushSubscription>, String>;
    /// Drops a subscription the push service says is gone.
    fn forget(&self, id: &str);
}

/// What happened when a message went to several browsers.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct PushReport {
    pub sent: Vec<String>,
    pub failed: Vec<(String, String)>,
    /// Browsers that unsubscribed or expired; they were forgotten.
    pub removed: Vec<String>,
}

/// Why one push didn't go through.
#[derive(Debug)]
enum SendError {
    /// The subscription is gone for good (404 or 410).
    Gone,
    Other(String),
}

/// Signs, encrypts and sends web push messages.
pub struct WebPush {
    key: ES256KeyPair,
    client: reqwest::Client,
    store: Arc<dyn PushStore>,
}

impl WebPush {
    /// A new private VAPID key, as bytes to keep.
    pub fn generate_key() -> Vec<u8> {
        ES256KeyPair::generate().to_bytes()
    }

    pub fn new(private_key: &[u8], store: Arc<dyn PushStore>) -> Result<Self, String> {
        let key = ES256KeyPair::from_bytes(private_key).map_err(|e| e.to_string())?;
        let client = reqwest::Client::builder()
            .tls_backend_preconfigured(crate::http::tls_config())
            .timeout(SEND_TIMEOUT)
            .user_agent(concat!("ergo/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { key, client, store })
    }

    /// The public key browsers subscribe with (base64url, uncompressed point).
    pub fn public_key(&self) -> String {
        Base64UrlUnpadded::encode_string(
            &self.key.public_key().public_key().to_bytes_uncompressed(),
        )
    }

    pub fn subscriptions(&self) -> Result<Vec<PushSubscription>, String> {
        self.store.subscriptions()
    }

    async fn send_one(
        &self,
        sub: &PushSubscription,
        payload: &[u8],
        urgency: &str,
    ) -> Result<(), SendError> {
        let bad = |what: &str| SendError::Other(format!("the subscription's {what} isn't valid"));
        let endpoint = sub.endpoint.parse().map_err(|_| bad("address"))?;
        let public = Base64UrlUnpadded::decode_vec(&sub.p256dh)
            .ok()
            .and_then(|b| p256::PublicKey::from_sec1_bytes(&b).ok())
            .ok_or_else(|| bad("key"))?;
        let auth = Base64UrlUnpadded::decode_vec(&sub.auth)
            .ok()
            .filter(|b| b.len() == 16)
            .ok_or_else(|| bad("secret"))?;
        let request = WebPushBuilder::new(endpoint, public, Auth::clone_from_slice(&auth))
            .with_valid_duration(TTL)
            .with_vapid(&self.key, CONTACT)
            .build(payload.to_vec())
            .map_err(|e| SendError::Other(e.to_string()))?;

        let (parts, body) = request.into_parts();
        let mut req = self
            .client
            .post(parts.uri.to_string())
            .header("Urgency", urgency)
            .body(body);
        for (name, value) in &parts.headers {
            req = req.header(name.as_str(), value.as_bytes());
        }
        let res = req
            .send()
            .await
            .map_err(|e| SendError::Other(e.to_string()))?;
        match res.status().as_u16() {
            200..=299 => Ok(()),
            404 | 410 => Err(SendError::Gone),
            code => {
                let text = res.text().await.unwrap_or_default();
                Err(SendError::Other(format!(
                    "the push service answered {code}{}",
                    if text.trim().is_empty() {
                        String::new()
                    } else {
                        format!(": {}", text.trim())
                    }
                )))
            }
        }
    }

    /// Sends `payload` to every subscription, or to the one with id `to`,
    /// with an RFC 8030 `urgency` (normal when unknown).
    pub async fn send(
        &self,
        to: Option<&str>,
        payload: &Value,
        urgency: &str,
    ) -> Result<PushReport, String> {
        let urgency = if URGENCIES.contains(&urgency) {
            urgency
        } else {
            "normal"
        };
        let subs: Vec<_> = self
            .store
            .subscriptions()?
            .into_iter()
            .filter(|s| to.is_none_or(|id| s.id == id))
            .collect();
        if subs.is_empty() {
            return Err(match to {
                Some(_) => "that browser no longer gets notifications; pick another or All browsers".into(),
                None => "no browser has turned on notifications yet: open this step in the browser that should get them and press Turn on".into(),
            });
        }
        let body = serde_json::to_vec(payload).map_err(|e| e.to_string())?;
        let mut report = PushReport::default();
        for sub in subs {
            match self.send_one(&sub, &body, urgency).await {
                Ok(()) => report.sent.push(sub.name),
                Err(SendError::Gone) => {
                    self.store.forget(&sub.id);
                    report.removed.push(sub.name);
                }
                Err(SendError::Other(e)) => report.failed.push((sub.name, e)),
            }
        }
        Ok(report)
    }
}

/// `push.send`: a notification to browsers with ergo notifications on.
pub struct PushSend {
    push: Arc<WebPush>,
}

impl PushSend {
    pub fn new(push: Arc<WebPush>) -> Self {
        Self { push }
    }
}

fn text(cfg: &Value, key: &str) -> String {
    match &cfg[key] {
        Value::Null => String::new(),
        Value::String(s) => s.trim().to_string(),
        other => other.to_string(),
    }
}

#[async_trait]
impl NodeExecutor for PushSend {
    fn schema(&self) -> NodeSchema {
        NodeSchema::new("push.send", NodeKind::Action, "Web push")
            .description("Sends a notification to browsers that turned on ergo notifications.")
            .field(
                Field::new("to", "Send to", FieldType::Text)
                    .default("all")
                    .help("all, or one browser's id."),
            )
            .field(
                Field::new("message", "Message", FieldType::Template)
                    .required()
                    .placeholder("The garage door is still open"),
            )
            .field(
                Field::new(
                    "urgency",
                    "Urgency",
                    FieldType::Select {
                        options: URGENCIES.iter().map(|u| u.to_string()).collect(),
                    },
                )
                .default("normal")
                .help("How soon devices on battery deliver it; high also keeps it on screen until dismissed."),
            )
            .field(
                Field::new("url", "Opens", FieldType::Template)
                    .placeholder("https://…")
                    .help("Where a click on the notification goes; ergo when empty."),
            )
            .output(object(json!({
                "sent": { "type": "array", "description": "Browsers it reached" },
                "failed": { "type": "array", "description": "Browsers it couldn't reach, with why" },
                "removed": { "type": "array", "description": "Browsers that had turned notifications off; forgotten" },
                "urgency": string("very-low, low, normal or high"),
                "message": string("The message that was sent"),
            })))
    }

    async fn run(&self, cfg: &Value, _: &Value, ctx: &RunCtx<'_>) -> Result<NodeOutput, NodeError> {
        let message = text(cfg, "message");
        if message.is_empty() {
            return Err(NodeError::new(ErrorKind::Config, "the message is empty"));
        }
        let url = text(cfg, "url");
        let to = match text(cfg, "to").as_str() {
            "" | "all" => None,
            id => Some(id.to_string()),
        };
        let urgency = match text(cfg, "urgency") {
            u if URGENCIES.contains(&u.as_str()) => u,
            _ => "normal".to_string(),
        };
        let payload = json!({
            "title": TITLE,
            "body": message,
            "url": url,
            "tag": ctx.workflow_id,
            "urgency": urgency,
        });
        let report = self
            .push
            .send(to.as_deref(), &payload, &urgency)
            .await
            .map_err(|e| NodeError::new(ErrorKind::Config, e))?;
        for name in &report.sent {
            ctx.log(format!("sent to {name}"));
        }
        for name in &report.removed {
            ctx.log(format!("{name} had turned notifications off; forgot it"));
        }
        for (name, e) in &report.failed {
            ctx.log(format!("couldn't reach {name}: {e}"));
        }
        let failed: Vec<_> = report
            .failed
            .iter()
            .map(|(name, e)| json!({ "name": name, "error": e }))
            .collect();
        if report.sent.is_empty() {
            return Err(
                NodeError::new(ErrorKind::Network, "no browser got the notification")
                    .details(json!({ "failed": failed, "removed": report.removed })),
            );
        }
        Ok(NodeOutput::out(json!({
            "sent": report.sent,
            "failed": failed,
            "removed": report.removed,
            "urgency": urgency,
            "message": message,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use web_push_native::p256::elliptic_curve::sec1::ToEncodedPoint;
    use wiremock::matchers::{header, header_exists, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[derive(Default)]
    struct MemStore(Mutex<Vec<PushSubscription>>);

    impl PushStore for MemStore {
        fn subscriptions(&self) -> Result<Vec<PushSubscription>, String> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn forget(&self, id: &str) {
            self.0.lock().unwrap().retain(|s| s.id != id);
        }
    }

    /// A browser's keys, as `pushManager.subscribe` would hand them over.
    fn browser(id: &str, endpoint: String) -> PushSubscription {
        browser_with_secret(id, endpoint).0
    }

    /// Also the browser's private key, to read what was sent to it.
    fn browser_with_secret(id: &str, endpoint: String) -> (PushSubscription, p256::SecretKey) {
        let secret =
            p256::SecretKey::random(&mut web_push_native::p256::elliptic_curve::rand_core::OsRng);
        let sub = PushSubscription {
            id: id.into(),
            name: format!("Browser {id}"),
            endpoint,
            p256dh: Base64UrlUnpadded::encode_string(
                secret.public_key().to_encoded_point(false).as_bytes(),
            ),
            auth: Base64UrlUnpadded::encode_string(&[7u8; 16]),
        };
        (sub, secret)
    }

    #[tokio::test]
    async fn only_that_browser_can_read_the_message() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(201))
            .mount(&server)
            .await;
        let (sub, secret) = browser_with_secret("a", format!("{}/a", server.uri()));
        let store = Arc::new(MemStore::default());
        *store.0.lock().unwrap() = vec![sub];
        let push = WebPush::new(&WebPush::generate_key(), store).unwrap();
        let payload = json!({ "title": "Garage", "body": "Still open" });
        push.send(None, &payload, "normal").await.unwrap();

        let body = server.received_requests().await.unwrap()[0].body.clone();
        assert!(
            !String::from_utf8_lossy(&body).contains("Still open"),
            "encrypted"
        );
        let plain =
            web_push_native::decrypt(body, &secret, &Auth::clone_from_slice(&[7u8; 16])).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&plain).unwrap(), payload);
    }

    #[tokio::test]
    async fn sends_signed_encrypted_pushes_and_forgets_gone_browsers() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/ok"))
            .and(header("content-encoding", "aes128gcm"))
            .and(header_exists("authorization"))
            .and(header_exists("ttl"))
            .and(header("urgency", "high"))
            .respond_with(ResponseTemplate::new(201))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/gone"))
            .respond_with(ResponseTemplate::new(410))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/broken"))
            .respond_with(ResponseTemplate::new(500).set_body_string("oops"))
            .mount(&server)
            .await;
        let store = Arc::new(MemStore::default());
        *store.0.lock().unwrap() = vec![
            browser("a", format!("{}/ok", server.uri())),
            browser("b", format!("{}/gone", server.uri())),
            browser("c", format!("{}/broken", server.uri())),
        ];
        let push = WebPush::new(&WebPush::generate_key(), store.clone()).unwrap();
        assert_eq!(push.public_key().len(), 87, "65 bytes, base64url");

        let report = push
            .send(None, &json!({ "body": "hi" }), "high")
            .await
            .unwrap();
        assert_eq!(report.sent, vec!["Browser a"]);
        assert_eq!(report.removed, vec!["Browser b"]);
        assert_eq!(report.failed[0].0, "Browser c");
        assert!(report.failed[0].1.contains("500: oops"));
        let left: Vec<_> = store
            .0
            .lock()
            .unwrap()
            .iter()
            .map(|s| s.id.clone())
            .collect();
        assert_eq!(left, vec!["a", "c"], "the gone browser is forgotten");

        // VAPID: the token is a JWT for the push service's origin, with ergo's key.
        let req = &server.received_requests().await.unwrap()[0];
        let auth = req.headers["authorization"].to_str().unwrap();
        assert!(auth.starts_with("vapid t="));
        assert!(auth.ends_with(&format!("k={}", push.public_key())));
    }

    #[tokio::test]
    async fn the_step_reports_who_got_it() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(201))
            .mount(&server)
            .await;
        let store = Arc::new(MemStore::default());
        let push = Arc::new(WebPush::new(&WebPush::generate_key(), store.clone()).unwrap());
        let node = PushSend::new(push);
        let (t, s) = (json!({}), json!({}));
        let ctx = RunCtx::new(&t, &s);
        let cfg = json!({ "to": "all", "message": "Still open", "urgency": "low" });

        // Nobody subscribed yet: a clear error.
        let e = node.run(&cfg, &json!({}), &ctx).await.unwrap_err();
        assert!(e.message.contains("no browser has turned on notifications"));

        *store.0.lock().unwrap() = vec![
            browser("a", format!("{}/a", server.uri())),
            browser("b", format!("{}/b", server.uri())),
        ];
        let out = node.run(&cfg, &json!({}), &ctx).await.unwrap();
        assert_eq!(out.output["sent"], json!(["Browser a", "Browser b"]));
        let reqs = server.received_requests().await.unwrap();
        assert!(reqs.iter().all(|r| r.headers["urgency"] == "low"));
        // Just one browser.
        let out = node
            .run(&json!({ "to": "b", "message": "Hi" }), &json!({}), &ctx)
            .await
            .unwrap();
        assert_eq!(out.output["sent"], json!(["Browser b"]));
        let e = node
            .run(&json!({ "message": " " }), &json!({}), &ctx)
            .await
            .unwrap_err();
        assert_eq!(e.kind, ErrorKind::Config);
    }
}
