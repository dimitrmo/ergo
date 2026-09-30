//! Configuration: clap flags, each also readable from an `ERGO_*` env var.
//!
//! Priority, highest first: CLI flag, env var, `/data/options.json` (set by
//! the user in the HA add-on UI), built-in default.

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use serde::Deserialize;

#[derive(Parser, Debug)]
#[command(
    name = "ergo",
    version,
    about = "Lightweight workflow builder for Home Assistant"
)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Option<Cmd>,

    /// Home Assistant URL. Default: http://supervisor/core as an add-on, else http://localhost:8123.
    #[arg(long, env = "ERGO_HA_URL")]
    pub ha_url: Option<String>,

    /// Long-lived access token. Default: SUPERVISOR_TOKEN as an add-on.
    #[arg(long, env = "ERGO_HA_TOKEN", hide_env_values = true)]
    pub ha_token: Option<String>,

    /// MQTT broker, e.g. mqtt://user:pass@localhost:1883. Default: Supervisor discovery.
    #[arg(long, env = "ERGO_MQTT_URL", hide_env_values = true)]
    pub mqtt_url: Option<String>,

    /// Where ergo.db lives. Default: /data as an add-on, else ./data.
    #[arg(long, env = "ERGO_DATA_DIR")]
    pub data_dir: Option<PathBuf>,

    /// API listen address. nginx proxies to it, so it stays on localhost.
    #[arg(long, env = "ERGO_BIND")]
    pub bind: Option<SocketAddr>,

    /// Log filter, e.g. info or ergo=debug.
    #[arg(long, env = "ERGO_LOG")]
    pub log: Option<String>,

    /// Time zone for cron triggers when Home Assistant doesn't report one.
    #[arg(long, env = "ERGO_TZ")]
    pub tz: Option<String>,

    #[arg(long, env = "ERGO_MAX_CONCURRENT_RUNS")]
    pub max_concurrent_runs: Option<usize>,

    #[arg(long, env = "ERGO_RUN_RETENTION_DAYS")]
    pub run_retention_days: Option<u32>,

    #[arg(long, env = "ERGO_RUN_RETENTION_MAX")]
    pub run_retention_max: Option<u32>,

    /// Injected by the Supervisor inside the add-on container.
    #[arg(long, env = "SUPERVISOR_TOKEN", hide = true, hide_env_values = true)]
    pub supervisor_token: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Run the server (default).
    Serve,
    /// Create or upgrade the database, then exit.
    Migrate,
    /// Validate a workflow graph JSON file.
    Check { file: PathBuf },
    /// Print every workflow's draft as JSON.
    Export,
}

/// Options the user sets in the HA add-on UI.
#[derive(Debug, Default, Deserialize)]
struct AddonOptions {
    log_level: Option<String>,
    run_retention_days: Option<u32>,
    run_retention_max: Option<u32>,
    max_concurrent_runs: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub addon: bool,
    pub ha_url: String,
    pub ha_token: Option<String>,
    pub mqtt_url: Option<String>,
    pub supervisor_token: Option<String>,
    pub data_dir: PathBuf,
    pub bind: SocketAddr,
    pub log: String,
    pub tz: Option<String>,
    pub max_concurrent_runs: usize,
    pub run_retention_days: u32,
    pub run_retention_max: u32,
}

impl Config {
    pub fn resolve(cli: &Cli) -> Self {
        let addon = cli.supervisor_token.is_some();
        let data_dir = cli.data_dir.clone().unwrap_or_else(|| {
            if addon {
                "/data".into()
            } else {
                "./data".into()
            }
        });
        let opts: AddonOptions = std::fs::read_to_string(data_dir.join("options.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        Self {
            addon,
            ha_url: cli.ha_url.clone().unwrap_or_else(|| {
                if addon {
                    "http://supervisor/core"
                } else {
                    "http://localhost:8123"
                }
                .into()
            }),
            ha_token: cli
                .ha_token
                .clone()
                .or_else(|| cli.supervisor_token.clone()),
            mqtt_url: cli.mqtt_url.clone(),
            supervisor_token: cli.supervisor_token.clone(),
            bind: cli
                .bind
                .unwrap_or_else(|| "127.0.0.1:8100".parse().unwrap()),
            log: cli
                .log
                .clone()
                .or(opts.log_level)
                .unwrap_or_else(|| "info".into()),
            tz: cli.tz.clone(),
            max_concurrent_runs: cli
                .max_concurrent_runs
                .or(opts.max_concurrent_runs)
                .unwrap_or(20),
            run_retention_days: cli
                .run_retention_days
                .or(opts.run_retention_days)
                .unwrap_or(7),
            run_retention_max: cli
                .run_retention_max
                .or(opts.run_retention_max)
                .unwrap_or(1000),
            data_dir,
        }
    }
}
