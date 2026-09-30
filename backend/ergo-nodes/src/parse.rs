//! `data.parse` and the format parsers behind it.
//!
//! Each format is a [`Parser`] registered by name. Adding CSV or YAML later
//! is one implementation and one line in [`parsers`]; `data.parse` lists the
//! registered formats in its dropdown and existing workflows are untouched.

use async_trait::async_trait;
use ergo_core::{
    ErrorKind, Field, FieldType, NodeError, NodeExecutor, NodeKind, NodeOutput, NodeSchema, RunCtx,
};
use quick_xml::events::Event;
use serde_json::{Map, Value, json};

/// Turns bytes of one format into JSON.
pub trait Parser: Send + Sync {
    /// Short name shown in the editor, e.g. "xml".
    fn format(&self) -> &'static str;
    /// Content types this parser handles, for automatic detection.
    fn content_types(&self) -> &[&'static str];
    /// Always returns JSON.
    fn parse(&self, bytes: &[u8], opts: &Value) -> Result<Value, NodeError>;
}

/// The registered formats.
pub fn parsers() -> Vec<Box<dyn Parser>> {
    vec![Box::new(XmlParser), Box::new(JsonParser)]
}

pub struct JsonParser;

impl Parser for JsonParser {
    fn format(&self) -> &'static str {
        "json"
    }
    fn content_types(&self) -> &[&'static str] {
        &["application/json", "text/json", "application/feed+json"]
    }
    fn parse(&self, bytes: &[u8], _: &Value) -> Result<Value, NodeError> {
        serde_json::from_slice(bytes).map_err(|e| {
            NodeError::new(ErrorKind::Parse, format!("not valid JSON: {e}"))
                .details(json!({ "line": e.line(), "column": e.column() }))
        })
    }
}

/// XML to JSON: elements become keys, attributes `@name`, text `#text`, and
/// repeated elements become arrays. An element with only text becomes a string.
pub struct XmlParser;

struct Frame {
    name: String,
    attrs: Map<String, Value>,
    children: Map<String, Value>,
    text: String,
}

impl Frame {
    fn finish(self) -> (String, Value) {
        let text = self.text.trim().to_string();
        if self.attrs.is_empty() && self.children.is_empty() {
            return (self.name, Value::String(text));
        }
        let mut obj = self.attrs;
        obj.extend(self.children);
        if !text.is_empty() {
            obj.insert("#text".into(), Value::String(text));
        }
        (self.name, Value::Object(obj))
    }
}

/// Repeated names turn into arrays.
fn insert_child(children: &mut Map<String, Value>, name: String, value: Value) {
    match children.get_mut(&name) {
        None => {
            children.insert(name, value);
        }
        Some(Value::Array(items)) => items.push(value),
        Some(existing) => {
            let first = existing.take();
            *existing = Value::Array(vec![first, value]);
        }
    }
}

fn line_col(bytes: &[u8], pos: usize) -> (usize, usize) {
    let before = &bytes[..pos.min(bytes.len())];
    let line = before.iter().filter(|b| **b == b'\n').count() + 1;
    let col = before.iter().rev().take_while(|b| **b != b'\n').count() + 1;
    (line, col)
}

impl Parser for XmlParser {
    fn format(&self) -> &'static str {
        "xml"
    }
    fn content_types(&self) -> &[&'static str] {
        &[
            "application/xml",
            "text/xml",
            "application/rss+xml",
            "application/atom+xml",
        ]
    }
    fn parse(&self, bytes: &[u8], _: &Value) -> Result<Value, NodeError> {
        let mut reader = quick_xml::Reader::from_reader(bytes);
        reader.config_mut().trim_text(false);
        let mut buf = Vec::new();
        let mut stack: Vec<Frame> = Vec::new();
        let mut root = Map::new();

        let err = |reader: &quick_xml::Reader<&[u8]>, msg: String| {
            let (line, column) = line_col(bytes, reader.buffer_position() as usize);
            NodeError::new(
                ErrorKind::Parse,
                format!("not valid XML: {msg} (line {line}, column {column})"),
            )
            .details(json!({ "line": line, "column": column }))
        };

        loop {
            let event = reader
                .read_event_into(&mut buf)
                .map_err(|e| err(&reader, e.to_string()))?;
            match event {
                Event::Start(e) => {
                    let frame = start_frame(&e).map_err(|m| err(&reader, m))?;
                    stack.push(frame);
                }
                Event::Empty(e) => {
                    let (name, value) = start_frame(&e).map_err(|m| err(&reader, m))?.finish();
                    match stack.last_mut() {
                        Some(parent) => insert_child(&mut parent.children, name, value),
                        None => insert_child(&mut root, name, value),
                    }
                }
                Event::Text(t) => {
                    if let Some(top) = stack.last_mut() {
                        // Entities arrive as separate GeneralRef events, so this is plain text.
                        top.text.push_str(&t.xml10_content());
                    }
                }
                Event::GeneralRef(r) => {
                    // Entities such as &amp; arrive separately in recent quick-xml.
                    if let Some(top) = stack.last_mut() {
                        let entity = format!("&{};", r.xml10_content());
                        let text = quick_xml::escape::unescape(&entity)
                            .map_err(|e| err(&reader, e.to_string()))?;
                        top.text.push_str(&text);
                    }
                }
                Event::CData(c) => {
                    if let Some(top) = stack.last_mut() {
                        top.text.push_str(&c.xml10_content());
                    }
                }
                Event::End(_) => {
                    let Some(frame) = stack.pop() else {
                        return Err(err(&reader, "unexpected closing tag".into()));
                    };
                    let (name, value) = frame.finish();
                    match stack.last_mut() {
                        Some(parent) => insert_child(&mut parent.children, name, value),
                        None => insert_child(&mut root, name, value),
                    }
                }
                Event::Eof => break,
                _ => {} // declarations, comments, processing instructions, doctype
            }
            buf.clear();
        }
        if !stack.is_empty() {
            return Err(err(
                &reader,
                format!("<{}> is never closed", stack.last().unwrap().name),
            ));
        }
        if root.is_empty() {
            return Err(NodeError::new(
                ErrorKind::Parse,
                "not valid XML: no root element",
            ));
        }
        Ok(Value::Object(root))
    }
}

fn start_frame(e: &quick_xml::events::BytesStart) -> Result<Frame, String> {
    let name = e.name().as_ref().to_string();
    let mut attrs = Map::new();
    for a in e.attributes() {
        let a = a.map_err(|x| x.to_string())?;
        let key = format!("@{}", a.key.as_ref());
        let value = a
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|x| x.to_string())?
            .to_string();
        attrs.insert(key, Value::String(value));
    }
    Ok(Frame {
        name,
        attrs,
        children: Map::new(),
        text: String::new(),
    })
}

/// The `data.parse` node.
pub struct DataParse {
    parsers: Vec<Box<dyn Parser>>,
}

impl DataParse {
    pub fn new() -> Self {
        Self { parsers: parsers() }
    }

    fn by_name(&self, name: &str) -> Option<&dyn Parser> {
        self.parsers
            .iter()
            .find(|p| p.format() == name)
            .map(|p| p.as_ref())
    }

    /// From the content type, else by sniffing the first character.
    fn detect(&self, content_type: &str, bytes: &[u8]) -> Option<&dyn Parser> {
        let ct = content_type
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if let Some(p) = self
            .parsers
            .iter()
            .find(|p| p.content_types().contains(&ct.as_str()))
        {
            return Some(p.as_ref());
        }
        if ct.ends_with("+xml") {
            return self.by_name("xml");
        }
        if ct.ends_with("+json") {
            return self.by_name("json");
        }
        match bytes.iter().find(|b| !b.is_ascii_whitespace()) {
            Some(b'<') => self.by_name("xml"),
            Some(b'{') | Some(b'[') => self.by_name("json"),
            _ => None,
        }
    }
}

impl Default for DataParse {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NodeExecutor for DataParse {
    fn schema(&self) -> NodeSchema {
        let mut options = vec!["auto".to_string()];
        options.extend(self.parsers.iter().map(|p| p.format().to_string()));
        NodeSchema::new("data.parse", NodeKind::Data, "Parse")
            .description("Turns a downloaded body or file into JSON.")
            .field(
                Field::new("format", "Format", FieldType::Select { options })
                    .default("auto")
                    .help("Auto uses the content type, then looks at the first character."),
            )
            .ports(vec!["out"])
    }

    async fn run(
        &self,
        cfg: &Value,
        input: &Value,
        ctx: &RunCtx<'_>,
    ) -> Result<NodeOutput, NodeError> {
        // The bytes: a streamed file, the inline body, or the input itself as text.
        let bytes: Vec<u8> = if let Some(path) = input["path"].as_str() {
            let path = std::path::Path::new(path);
            // Only files this run downloaded, never arbitrary paths from the input.
            let allowed = ctx.temp_dir.is_some_and(|d| path.starts_with(d))
                && path
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with(ctx.run_id));
            if !allowed {
                return Err(NodeError::new(
                    ErrorKind::Config,
                    "can only read files downloaded in this run",
                ));
            }
            tokio::fs::read(path).await.map_err(|e| {
                NodeError::new(ErrorKind::Other, format!("reading {}: {e}", path.display()))
            })?
        } else if let Some(body) = input["body"].as_str() {
            body.as_bytes().to_vec()
        } else if let Some(text) = input.as_str() {
            text.as_bytes().to_vec()
        } else {
            return Err(NodeError::new(
                ErrorKind::Config,
                "the input has no body or file to parse; connect a Download step before this one",
            ));
        };

        let format = cfg["format"].as_str().unwrap_or("auto");
        let parser = if format == "auto" {
            self.detect(input["content_type"].as_str().unwrap_or_default(), &bytes)
                .ok_or_else(|| {
                    NodeError::new(
                        ErrorKind::Parse,
                        "couldn't tell the format; pick one in Format",
                    )
                    .details(json!({ "content_type": input["content_type"] }))
                })?
        } else {
            self.by_name(format).ok_or_else(|| {
                NodeError::new(ErrorKind::Config, format!("unknown format {format}"))
            })?
        };
        let value = parser.parse(&bytes, cfg)?;
        ctx.log(format!(
            "parsed {} bytes as {}",
            bytes.len(),
            parser.format()
        ));
        Ok(NodeOutput::out(value))
    }
}
