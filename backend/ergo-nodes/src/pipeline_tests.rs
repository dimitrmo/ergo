//! Tests for the Data nodes, alone and as the doc's RSS pipeline.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use ergo_core::{
    Engine, ErrorKind, Graph, NodeExecutor, NodeRecord, RunCtx, RunRequest, RunSink, RunStart,
    RunStatus,
};
use serde_json::{Value, json};
use wiremock::matchers::{body_json, body_string, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::*;

const FEED: &str = r#"<?xml version="1.0"?>
<rss version="2.0">
  <channel>
    <title>Blog &amp; news</title>
    <item><title>Rust 2.0</title><link>https://e.x/1</link><category>rust</category><pubDate>Mon</pubDate></item>
    <item><title>Gardening</title><link>https://e.x/2</link><category>home</category><pubDate>Tue</pubDate></item>
    <item><title><![CDATA[Async <3]]></title><link>https://e.x/3</link><category>rust</category><pubDate>Wed</pubDate></item>
  </channel>
</rss>"#;

fn ctx_parts() -> (Value, Value) {
    (json!({ "time": "2026-09-29 07:00" }), json!({}))
}

#[test]
fn xml_maps_to_json() {
    let v = XmlParser.parse(FEED.as_bytes(), &json!({})).unwrap();
    assert_eq!(v["rss"]["@version"], "2.0");
    assert_eq!(v["rss"]["channel"]["title"], "Blog & news");
    let items = v["rss"]["channel"]["item"].as_array().unwrap();
    assert_eq!(items.len(), 3, "repeated elements become an array");
    assert_eq!(items[2]["title"], "Async <3", "CDATA is text");
    // Mixed content keeps text under #text.
    let mixed = XmlParser
        .parse(b"<p class=\"x\">hi <b>there</b></p>", &json!({}))
        .unwrap();
    assert_eq!(
        mixed,
        json!({ "p": { "@class": "x", "#text": "hi", "b": "there" } })
    );
}

#[test]
fn xml_errors_have_a_position() {
    let e = XmlParser.parse(b"<a>\n<b></a>", &json!({})).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Parse);
    assert_eq!(e.details["line"], 2);
}

#[tokio::test]
async fn parse_detects_the_format() {
    let (t, s) = ctx_parts();
    let ctx = RunCtx::new(&t, &s);
    let input = json!({ "content_type": "application/rss+xml; charset=utf-8", "body": FEED });
    let out = DataParse::new()
        .run(&json!({ "format": "auto" }), &input, &ctx)
        .await
        .unwrap();
    assert_eq!(out.output["rss"]["channel"]["item"][0]["title"], "Rust 2.0");
    let json_in = json!({ "content_type": "", "body": "{\"a\":[1,2]}" });
    let out = DataParse::new()
        .run(&json!({ "format": "auto" }), &json_in, &ctx)
        .await
        .unwrap();
    assert_eq!(out.output, json!({ "a": [1, 2] }));
}

#[tokio::test]
async fn parse_refuses_files_it_did_not_download() {
    let (t, s) = ctx_parts();
    let dir = std::env::temp_dir();
    let ctx = RunCtx::new(&t, &s).with_run("run1", Some(&dir));
    let input = json!({ "path": "/etc/passwd" });
    let e = DataParse::new()
        .run(&json!({ "format": "xml" }), &input, &ctx)
        .await
        .unwrap_err();
    assert_eq!(e.kind, ErrorKind::Config);
}

#[tokio::test]
async fn filter_keeps_matching_items_and_uses_the_empty_port() {
    let (t, s) = ctx_parts();
    let ctx = RunCtx::new(&t, &s);
    let parsed = XmlParser.parse(FEED.as_bytes(), &json!({})).unwrap();
    let cfg = json!({
        "path": "$.rss.channel.item", "mode": "rules", "match": "all",
        "rules": [{ "field": "category", "op": "equals", "value": "rust" }]
    });
    let out = DataFilter.run(&cfg, &parsed, &ctx).await.unwrap();
    assert_eq!(out.port, "out");
    assert_eq!(out.output["count"], 2);

    let expr = json!({ "path": "$.rss.channel.item", "mode": "expression", "expression": "{{ item.title == 'Gardening' }}" });
    assert_eq!(
        DataFilter.run(&expr, &parsed, &ctx).await.unwrap().output["count"],
        1
    );

    let none = json!({ "path": "$.rss.channel.item", "mode": "rules", "rules": [{ "field": "category", "op": "equals", "value": "cars" }] });
    let out = DataFilter.run(&none, &parsed, &ctx).await.unwrap();
    assert_eq!(out.port, "empty");
    assert_eq!(out.output, json!({ "items": [], "count": 0 }));
}

#[tokio::test]
async fn map_reshapes_each_item_with_paths_and_templates() {
    let (t, s) = ctx_parts();
    let ctx = RunCtx::new(&t, &s);
    let input = json!({ "items": [{ "title": "A", "link": "l1", "pubDate": "Mon" }], "count": 1 });
    let cfg = json!({ "path": "$.items", "fields": [
        { "name": "title", "value": "$.title" },
        { "name": "meta.published", "value": "$.pubDate" },
        { "name": "label", "value": "{{ item.title }} ({{ trigger.time }})" }
    ]});
    let out = DataMap.run(&cfg, &input, &ctx).await.unwrap();
    assert_eq!(
        out.output,
        json!({ "count": 1, "items": [
            { "title": "A", "meta": { "published": "Mon" }, "label": "A (2026-09-29 07:00)" }
        ]})
    );
}

#[tokio::test]
async fn compose_can_produce_json() {
    let (t, s) = ctx_parts();
    let ctx = RunCtx::new(&t, &s);
    let out = TextCompose
        .run(
            &json!({ "template": "{\"on\": true}", "format": "json" }),
            &json!({}),
            &ctx,
        )
        .await
        .unwrap();
    assert_eq!(out.output["json"], json!({ "on": true }));
    let bad = TextCompose
        .run(
            &json!({ "template": "{nope", "format": "json" }),
            &json!({}),
            &ctx,
        )
        .await
        .unwrap_err();
    assert_eq!(bad.kind, ErrorKind::Parse);
}

#[tokio::test]
async fn download_inline_large_and_errors() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/feed.xml"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/rss+xml")
                .set_body_string(FEED),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/big"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'x'; 1024 * 1024 + 10]))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/gone"))
        .respond_with(ResponseTemplate::new(503).set_body_string("try later"))
        .mount(&server)
        .await;

    let (t, s) = ctx_parts();
    let dir = std::env::temp_dir().join(format!("ergo-dl-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let ctx = RunCtx::new(&t, &s).with_run("run42", Some(&dir));
    let dl = HttpDownload::new();

    let out = dl.run(&json!({ "url": format!("{}/feed.xml", server.uri()), "query": [{ "key": "q", "value": "1" }] }), &json!({}), &ctx).await.unwrap();
    assert_eq!(out.output["status"], 200);
    assert_eq!(out.output["body"], FEED);
    assert!(out.output["path"].is_null());

    let big = dl
        .run(
            &json!({ "url": format!("{}/big", server.uri()) }),
            &json!({}),
            &ctx,
        )
        .await
        .unwrap();
    let file = big.output["path"].as_str().unwrap();
    assert!(
        file.contains("run42-"),
        "temp files are named after the run"
    );
    assert_eq!(std::fs::metadata(file).unwrap().len(), 1024 * 1024 + 10);
    assert_eq!(big.output["size"], 1024 * 1024 + 10);

    let e = dl
        .run(
            &json!({ "url": format!("{}/gone", server.uri()) }),
            &json!({}),
            &ctx,
        )
        .await
        .unwrap_err();
    assert_eq!(e.kind, ErrorKind::HttpStatus);
    assert_eq!(e.details["status"], 503);
    assert_eq!(e.details["body"], "try later");

    let limit = dl
        .run(
            &json!({ "url": format!("{}/big", server.uri()), "max_mb": 1 }),
            &json!({}),
            &ctx,
        )
        .await
        .unwrap_err();
    assert!(limit.message.contains("1 MB limit"));
    assert!(ctx.take_logs().iter().any(|l| l.contains("streamed to")));
}

#[derive(Default)]
struct Sink {
    nodes: Mutex<Vec<NodeRecord>>,
    done: Mutex<Option<(RunStatus, Option<String>)>>,
}

impl RunSink for Sink {
    fn run_started(&self, _: &RunStart) {}
    fn node_finished(&self, n: &NodeRecord) {
        self.nodes.lock().unwrap().push(n.clone());
    }
    fn run_finished(&self, _: &str, s: RunStatus, e: Option<&str>) {
        *self.done.lock().unwrap() = Some((s, e.map(str::to_string)));
    }
    fn run_skipped(&self, _: &RunStart, _: &str) {}
}

struct NoMqtt;
#[async_trait]
impl MqttPublisher for NoMqtt {
    async fn publish(&self, _: &str, _: Vec<u8>, _: u8, _: bool) -> Result<(), String> {
        Ok(())
    }
}

#[tokio::test]
async fn the_rss_pipeline_from_the_doc() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/feed.xml"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/rss+xml")
                .set_body_string(FEED),
        )
        .mount(&server)
        .await;

    let graph: Graph = serde_json::from_value(json!({
        "nodes": [
            { "id": "t", "type": "trigger.manual" },
            { "id": "dl", "type": "http.download", "config": { "url": format!("{}/feed.xml", server.uri()) } },
            { "id": "parse", "type": "data.parse", "config": { "format": "auto" } },
            { "id": "filter", "type": "data.filter", "config": { "path": "$.rss.channel.item", "mode": "rules", "match": "all",
                "rules": [{ "field": "category", "op": "equals", "value": "rust" }] } },
            { "id": "map", "type": "data.map", "config": { "path": "$.items", "fields": [
                { "name": "title", "value": "$.title" }, { "name": "link", "value": "$.link" }, { "name": "published", "value": "$.pubDate" } ] } },
            { "id": "msg", "type": "text.compose", "config": { "template": "New post: {{ input.items[0].title }} ({{ input.count }} new)" } }
        ],
        "edges": [
            { "from": "t", "to": "dl" }, { "from": "dl", "to": "parse" }, { "from": "parse", "to": "filter" },
            { "from": "filter", "to": "map" }, { "from": "map", "to": "msg" }
        ]
    })).unwrap();

    let dir = std::env::temp_dir().join(format!("ergo-pipe-{}", uuid::Uuid::new_v4()));
    let sink = Arc::new(Sink::default());
    let engine = Engine::new(
        Arc::new(registry(Some(Arc::new(NoMqtt)))),
        sink.clone(),
        4,
        Duration::from_secs(10),
    )
    .with_temp_dir(dir);
    engine.start(RunRequest {
        workflow_id: "wf".into(),
        version: 0,
        graph: Arc::new(graph),
        trigger_node: "t".into(),
        trigger: json!({ "kind": "manual" }),
    });
    for _ in 0..200 {
        if sink.done.lock().unwrap().is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let done = sink.done.lock().unwrap().clone().unwrap();
    assert_eq!(done.0, RunStatus::Success, "{:?}", done.1);
    let nodes = sink.nodes.lock().unwrap();
    let msg = nodes.iter().find(|n| n.node_id == "msg").unwrap();
    assert_eq!(msg.output["text"], "New post: Rust 2.0 (2 new)");
    let map = nodes.iter().find(|n| n.node_id == "map").unwrap();
    assert_eq!(
        map.output["items"][1],
        json!({ "title": "Async <3", "link": "https://e.x/3", "published": "Wed" })
    );
}

#[tokio::test]
async fn map_with_jsonata() {
    let trigger = json!({ "time": "07:00" });
    let steps = json!({ "dl": { "output": { "status": 200 } } });
    let ctx = RunCtx::new(&trigger, &steps);
    let parsed = XmlParser.parse(FEED.as_bytes(), &json!({})).unwrap();

    // A list: filtered, reshaped and wrapped as { count, items }.
    let cfg = json!({ "mode": "jsonata", "expression":
        "rss.channel.item[category = 'rust'].{ 'title': title, 'n': $count($split(title, ' ')), 'at': $trigger.time }" });
    let out = DataMap.run(&cfg, &parsed, &ctx).await.unwrap();
    assert_eq!(
        out.output,
        json!({ "count": 2, "items": [
            { "title": "Rust 2.0", "n": 2, "at": "07:00" },
            { "title": "Async <3", "n": 2, "at": "07:00" }
        ]})
    );

    // One value comes out as is; earlier steps are $steps.
    let one = json!({ "mode": "jsonata", "expression": "{ 'feed': rss.channel.title, 'http': $steps.dl.output.status }" });
    assert_eq!(
        DataMap.run(&one, &parsed, &ctx).await.unwrap().output,
        json!({ "feed": "Blog & news", "http": 200 })
    );

    // Nothing matched: an empty list rather than an error.
    let none = json!({ "mode": "jsonata", "expression": "rss.channel.item[category = 'cars']" });
    assert_eq!(
        DataMap.run(&none, &parsed, &ctx).await.unwrap().output,
        json!({ "count": 0, "items": [] })
    );

    // Errors: syntax is caught by validate, runtime problems are typed.
    assert!(
        !DataMap
            .validate(&json!({ "mode": "jsonata", "expression": "items[" }))
            .is_empty()
    );
    assert!(
        DataMap
            .validate(&json!({ "mode": "jsonata", "expression": "items.title" }))
            .is_empty()
    );
    let bad = json!({ "mode": "jsonata", "expression": "$number('abc') + 1" });
    let e = DataMap.run(&bad, &parsed, &ctx).await.unwrap_err();
    assert_eq!(e.kind, ErrorKind::Template);
    assert!(e.message.starts_with("JSONata:"), "{}", e.message);
    // Runaway expressions stop at the guardrails instead of hanging a run.
    let forever =
        json!({ "mode": "jsonata", "expression": "($f := function($x){ $f($x + 1) }; $f(0))" });
    assert_eq!(
        DataMap.run(&forever, &parsed, &ctx).await.unwrap_err().kind,
        ErrorKind::Template
    );
}

#[tokio::test]
async fn filter_with_jsonata() {
    let (t, s) = ctx_parts();
    let ctx = RunCtx::new(&t, &s);
    let parsed = XmlParser.parse(FEED.as_bytes(), &json!({})).unwrap();
    let run = |expr: &str| {
        let cfg = json!({ "path": "$.rss.channel.item", "mode": "jsonata", "jsonata": expr });
        let (parsed, ctx) = (&parsed, &ctx);
        async move { DataFilter.run(&cfg, parsed, ctx).await }
    };

    let out = run("category = 'rust' and $contains($lowercase(title), 'async')")
        .await
        .unwrap();
    assert_eq!(out.output["count"], 1);
    assert_eq!(out.output["items"][0]["title"], "Async <3");

    // $input is the whole input; the channel title is outside the items.
    let out = run("$contains($input.rss.channel.title, 'news') and pubDate in ['Mon', 'Tue']")
        .await
        .unwrap();
    assert_eq!(out.output["count"], 2);

    // JSONata truthiness: a missing field is false, a non-empty string is true.
    assert_eq!(run("author").await.unwrap().port, "empty");
    assert_eq!(run("link").await.unwrap().output["count"], 3);

    assert!(
        !DataFilter
            .validate(&json!({ "mode": "jsonata", "jsonata": "title = " }))
            .is_empty()
    );
    assert!(
        DataFilter
            .validate(&json!({ "mode": "jsonata", "jsonata": "title = 'x'" }))
            .is_empty()
    );
    assert_eq!(
        run("$number(title) > 1").await.unwrap_err().kind,
        ErrorKind::Template
    );
}

#[tokio::test]
async fn request_sends_every_kind_of_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/items"))
        .and(query_param("dry", "1"))
        .and(header("authorization", "Bearer s3cret"))
        .and(header("x-source", "ergo"))
        .and(body_json(json!({ "title": "Rust \"2.0\"", "n": 2 })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "id": 7 })))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/form"))
        .and(header("content-type", "application/x-www-form-urlencoded"))
        .and(body_string("a=1&b=two+words"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/text"))
        .and(header("content-type", "text/csv"))
        .and(body_string("a,b\n1,2"))
        .respond_with(ResponseTemplate::new(200).set_body_string("fine"))
        .mount(&server)
        .await;

    let (t, s) = ctx_parts();
    let ctx = RunCtx::new(&t, &s);
    let req = HttpRequest::new();

    let out = req
        .run(
            &json!({
                "method": "POST", "url": format!("{}/items", server.uri()),
                "query": [{ "key": "dry", "value": "1" }],
                "headers": [{ "key": "X-Source", "value": "ergo" }],
                "auth": { "type": "bearer", "token": "s3cret" },
                "body_type": "json", "body": "{ \"title\": \"Rust \\\"2.0\\\"\", \"n\": 2 }"
            }),
            &json!({}),
            &ctx,
        )
        .await
        .unwrap();
    assert_eq!(out.output["status"], 201);
    assert_eq!(out.output["ok"], true);
    assert_eq!(
        out.output["json"],
        json!({ "id": 7 }),
        "JSON responses are parsed"
    );

    let out = req
        .run(
            &json!({ "method": "PUT", "url": format!("{}/form", server.uri()), "body_type": "form",
                "form": [{ "key": "a", "value": "1" }, { "key": "b", "value": "two words" }] }),
            &json!({}),
            &ctx,
        )
        .await
        .unwrap();
    assert_eq!(out.output["status"], 204);

    let out = req
        .run(
            &json!({ "method": "PATCH", "url": format!("{}/text", server.uri()), "body_type": "text",
                "body": "a,b\n1,2", "content_type": "text/csv" }),
            &json!({}),
            &ctx,
        )
        .await
        .unwrap();
    assert_eq!(out.output["body"], "fine");
    assert!(out.output["json"].is_null());
}

#[tokio::test]
async fn request_errors_and_status_handling() {
    let server = MockServer::start().await;
    Mock::given(path("/boom"))
        .respond_with(ResponseTemplate::new(500).set_body_string("database down"))
        .mount(&server)
        .await;
    let (t, s) = ctx_parts();
    let ctx = RunCtx::new(&t, &s);
    let req = HttpRequest::new();
    let url = format!("{}/boom", server.uri());

    let e = req
        .run(&json!({ "method": "DELETE", "url": url }), &json!({}), &ctx)
        .await
        .unwrap_err();
    assert_eq!(e.kind, ErrorKind::HttpStatus);
    assert_eq!(e.details["body"], "database down");

    // With "fail on HTTP errors" off, the response is output for later steps.
    let out = req
        .run(
            &json!({ "method": "GET", "url": url, "fail_on_status": false }),
            &json!({}),
            &ctx,
        )
        .await
        .unwrap();
    assert_eq!(
        (out.output["status"].clone(), out.output["ok"].clone()),
        (json!(500), json!(false))
    );

    // A template that rendered into broken JSON fails before sending.
    let e = req
        .run(
            &json!({ "method": "POST", "url": url, "body_type": "json", "body": "{ \"t\": Rust }" }),
            &json!({}),
            &ctx,
        )
        .await
        .unwrap_err();
    assert_eq!(e.kind, ErrorKind::Template);

    // Checked in the editor when the body has no template tags.
    assert!(
        !req.validate(&json!({ "url": url, "body_type": "json", "body": "{ nope" }))
            .is_empty()
    );
    assert!(
        req.validate(
            &json!({ "url": url, "body_type": "json", "body": "{ \"a\": {{ input.x }} }" })
        )
        .is_empty()
    );
    assert!(
        !req.validate(&json!({ "url": url, "method": "FETCH" }))
            .is_empty()
    );

    let refused = req
        .run(
            &json!({ "method": "GET", "url": "http://127.0.0.1:9/" }),
            &json!({}),
            &ctx,
        )
        .await
        .unwrap_err();
    assert_eq!(refused.kind, ErrorKind::Network);
}
