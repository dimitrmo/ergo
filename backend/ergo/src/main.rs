mod api;
mod config;
mod db;
mod ha;
mod mqtt;
mod triggers;

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::Parser;
use ergo_core::{Engine, Graph, has_errors, validate};
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use crate::config::{Cli, Cmd, Config, MqttSetting};
use crate::db::Db;
use crate::ha::Ha;
use crate::mqtt::{Broker, Mqtt};
use crate::triggers::Triggers;

const NODE_TIMEOUT: Duration = Duration::from_secs(30);

#[tokio::main]
async fn main() -> Result<()> {
    // reqwest is built with `rustls-no-provider` (ergo-nodes picks ring for its
    // own client), so every other client needs ring as the process default.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cli = Cli::parse();
    let cfg = Config::resolve(&cli);
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_new(&cfg.log).unwrap_or_else(|_| EnvFilter::new("info")))
        .with_target(false)
        .init();

    match cli.cmd.unwrap_or(Cmd::Serve) {
        Cmd::Serve => serve(cfg).await,
        Cmd::Migrate => {
            Db::open(&cfg.data_dir)?;
            info!("database is up to date");
            Ok(())
        }
        Cmd::Check { file } => check(&file),
        Cmd::Export => {
            let db = Db::open(&cfg.data_dir)?;
            let workflows = db
                .list_workflows()?
                .into_iter()
                .map(|wf| api::ExportedWorkflow {
                    name: wf.name,
                    draft: wf.draft,
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&api::ExportFile::new(workflows))?
            );
            Ok(())
        }
    }
}

/// Validates a workflow graph file; exits non-zero on errors (useful in CI).
fn check(file: &std::path::Path) -> Result<()> {
    let text =
        std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
    let graph: Graph = serde_json::from_str(&text).context("parsing workflow JSON")?;
    // Checks every step type, MQTT included, whatever this install's options.
    let registry = ergo_nodes::registry(Some(Mqtt::disabled(None)));
    let issues = validate(&graph, &registry);
    for issue in &issues {
        println!(
            "{:?} {}: {}",
            issue.severity,
            issue.node.as_deref().unwrap_or("-"),
            issue.message
        );
    }
    if has_errors(&issues) {
        std::process::exit(1);
    }
    println!("ok");
    Ok(())
}

/// How long startup waits for the broker. It covers a broker that starts
/// alongside ergo, and stays well inside the container healthcheck's grace.
const MQTT_CONNECT_WAIT: Duration = Duration::from_secs(20);

/// Connects to the broker `mqtt_url` names, checking the setting first.
/// MQTT is turned on only with a working connection; otherwise this returns
/// None and, unless MQTT was turned off on purpose, why.
async fn connect_mqtt(cfg: &Config) -> (Option<Arc<Mqtt>>, Option<String>) {
    let broker = match &cfg.mqtt {
        MqttSetting::Off => {
            info!("MQTT is turned off");
            return (None, None);
        }
        MqttSetting::Url(url) => Broker::parse(url).map_err(|e| e.to_string()),
        MqttSetting::Auto => match &cfg.supervisor_token {
            None => Err("mqtt_url is auto, but only the add-on can look up a broker; set a broker URL".into()),
            Some(token) => match Broker::discover(token).await {
                Ok(Some(b)) => Ok(b),
                Ok(None) => Err(
                    "no MQTT broker found; install the Mosquitto add-on or set mqtt_url to a broker URL"
                        .into(),
                ),
                Err(e) => Err(format!("looking up the MQTT broker failed: {e}")),
            },
        },
    };
    let result = match broker {
        Ok(b) => Mqtt::connect(b, MQTT_CONNECT_WAIT).await,
        Err(e) => Err(e),
    };
    match result {
        Ok(mqtt) => (Some(mqtt), None),
        Err(e) => {
            error!(error = %e, "MQTT stays off; fix mqtt_url and restart ergo");
            (None, Some(e))
        }
    }
}

async fn serve(cfg: Config) -> Result<()> {
    info!(
        version = env!("CARGO_PKG_VERSION"),
        addon = cfg.addon,
        data = %cfg.data_dir.display(),
        "starting ergo"
    );
    let db = Arc::new(Db::open(&cfg.data_dir)?);
    let interrupted = db.mark_interrupted()?;
    if interrupted > 0 {
        warn!(
            runs = interrupted,
            "marked runs that were in progress at shutdown as interrupted"
        );
    }

    let (ha, ha_commands) = Ha::new(&cfg.ha_url, cfg.ha_token.clone())?;
    tokio::spawn(ha.clone().run(ha_commands));

    let (mqtt, mqtt_error) = connect_mqtt(&cfg).await;
    let registry = Arc::new(ergo_nodes::registry(
        mqtt.clone()
            .map(|m| m as Arc<dyn ergo_nodes::MqttPublisher>),
    ));
    // Large downloads go to <data>/tmp; each run's files are deleted when it ends.
    let engine = Engine::new(registry, db.clone(), cfg.max_concurrent_runs, NODE_TIMEOUT)
        .with_temp_dir(cfg.data_dir.join("tmp"));

    let fallback_tz = cfg
        .tz
        .as_deref()
        .and_then(|z| z.parse().ok())
        .unwrap_or(chrono_tz::UTC);
    let triggers = Triggers::new(db.clone(), engine.clone(), ha.clone(), fallback_tz);
    triggers.reload()?;
    tokio::spawn(triggers.clone().dispatch_state_changes());

    let retention_db = db.clone();
    let (days, max) = (cfg.run_retention_days, cfg.run_retention_max);
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(3600));
        loop {
            tick.tick().await;
            match retention_db.prune_runs(days, max) {
                Ok(0) => {}
                Ok(n) => info!(runs = n, "pruned old runs"),
                Err(e) => error!(error = %e, "pruning runs"),
            }
        }
    });

    let state = Arc::new(api::AppState {
        db,
        engine,
        ha,
        mqtt,
        mqtt_error,
        triggers,
        started: Instant::now(),
        retention: (days, max),
    });
    // The UI is static files served by nginx, which also proxies /api,
    // /health and /ready here and enforces Ingress-only access.
    let app = api::router(state).layer(TraceLayer::new_for_http());

    let listener = TcpListener::bind(cfg.bind)
        .await
        .with_context(|| format!("binding {}", cfg.bind))?;
    info!(addr = %cfg.bind, "listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown())
        .await?;
    info!("stopped");
    Ok(())
}

async fn shutdown() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    let term = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = term => {}
    }
    info!("shutting down");
}
