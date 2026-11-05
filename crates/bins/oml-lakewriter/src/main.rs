//! `oml-lakewriter` — the lake writer's commands (ADR-0060): `run` drains
//! the outbox under the single-writer lease and maintains the lake;
//! `doctor` prints the lake's health lines; `query` runs a named operator
//! query over the lake files; `load-bench` loads `oml-bench/1` results;
//! `keygen` writes a lake KEK file. No command serves a client.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use oml_lake::Lake;
use oml_lake::crypto::Kek;
use oml_lakewriter::source::{Lease, Source};
use oml_lakewriter::{Settings, Status, Writer, WriterError, load_bench};
use prometheus_client::registry::Registry;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

#[derive(Parser)]
#[command(
    name = "oml-lakewriter",
    version,
    about = "Drains the OML outbox into the Delta lake; the lake serves no client read."
)]
struct Cli {
    /// The lake root: an absolute path or file:// URL.
    #[arg(long, env = "OML_LAKE_URL", global = true)]
    lake: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(clap::Args)]
struct Db {
    /// Postgres URL of the lake writer's own role (never the lead's).
    #[arg(long, env = "OML_LAKE_DATABASE_URL", hide_env_values = true)]
    database_url: String,
}

#[derive(Subcommand)]
enum Cmd {
    /// Drain continuously under the lease, refresh telemetry and maintain.
    Run {
        #[command(flatten)]
        db: Db,
        /// Owner URL: when set, grants the writer's role at start, then closes.
        #[arg(long, env = "OML_LAKE_OWNER_URL", hide_env_values = true)]
        owner_url: Option<String>,
        /// The lake KEK file (32 bytes or 64 hex), owner-only.
        #[arg(long, env = "OML_LAKE_KEK_FILE")]
        kek_file: PathBuf,
        /// Prometheus listener, never the public port.
        #[arg(long, env = "OML_LAKE_METRICS_ADDR", default_value = "127.0.0.1:9466")]
        metrics_addr: String,
        #[arg(long, default_value_t = 1000)]
        poll_ms: u64,
        #[arg(long, default_value_t = 5000)]
        max_batch: i64,
        #[arg(long, default_value_t = 300)]
        telemetry_every_s: u64,
        #[arg(long, default_value_t = 86_400)]
        maintain_every_s: u64,
        /// VACUUM retention: the lake erasure bound (default 7 days).
        #[arg(long, default_value_t = 168)]
        retention_hours: i64,
    },
    /// Drain until caught up once, under the lease, and exit.
    DrainOnce {
        #[command(flatten)]
        db: Db,
        #[arg(long, env = "OML_LAKE_KEK_FILE")]
        kek_file: PathBuf,
    },
    /// The lake's health lines; exit 1 when one fails.
    Doctor {
        #[command(flatten)]
        db: Db,
        #[arg(long, default_value_t = 1000)]
        lag_rows_warn: i64,
        #[arg(long, default_value_t = 300)]
        lag_seconds_warn: i64,
    },
    /// Run a named operator query over the lake files (--list to list them).
    Query {
        name: Option<String>,
        #[arg(long)]
        list: bool,
    },
    /// Load oml-bench/1 result files or directories into bench.results.
    LoadBench { paths: Vec<PathBuf> },
    /// Write a fresh lake KEK file, owner-only; refuses to overwrite.
    Keygen {
        #[arg(long)]
        out: PathBuf,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt().json().with_target(false).init();
    let cli = Cli::parse();
    match dispatch(cli).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("oml-lakewriter: {e}");
            ExitCode::FAILURE
        }
    }
}

fn lake_of(lake: Option<&String>) -> Result<Lake, String> {
    let url = lake.ok_or("--lake or OML_LAKE_URL names the lake root")?;
    Lake::at(url).map_err(|e| e.to_string())
}

fn kek_of(path: &PathBuf) -> Result<Kek, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("lake KEK file {}: {e}", path.display()))?;
    Kek::from_file_bytes(&bytes).map_err(|e| e.to_string())
}

async fn dispatch(cli: Cli) -> Result<ExitCode, String> {
    match cli.cmd {
        Cmd::Keygen { out } => {
            keygen(&out)?;
            println!("wrote a lake KEK to {}", out.display());
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Query { name, list } => {
            let Some(name) = name.filter(|_| !list) else {
                for q in oml_lake::query::QUERIES {
                    println!("{:<28} {}", q.name, q.answers);
                }
                return Ok(ExitCode::SUCCESS);
            };
            let q = oml_lake::query::named(&name)
                .ok_or_else(|| format!("no query named {name:?}; --list names them"))?;
            let lake = lake_of(cli.lake.as_ref())?;
            let batches = oml_lake::query::run(&lake, q.sql)
                .await
                .map_err(|e| e.to_string())?;
            println!(
                "{}",
                oml_lake::query::pretty(&batches).map_err(|e| e.to_string())?
            );
            Ok(ExitCode::SUCCESS)
        }
        Cmd::LoadBench { paths } => {
            let lake = lake_of(cli.lake.as_ref())?;
            let (files, rows) = load_bench(&lake, &paths).await.map_err(|e| e.to_string())?;
            println!("loaded {files} oml-bench/1 files, {rows} numbers");
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Doctor {
            db,
            lag_rows_warn,
            lag_seconds_warn,
        } => {
            let lake = lake_of(cli.lake.as_ref())?;
            doctor(&lake, &db.database_url, lag_rows_warn, lag_seconds_warn).await
        }
        Cmd::DrainOnce { db, kek_file } => {
            let lake = lake_of(cli.lake.as_ref())?;
            let lease = Lease::try_acquire(&db.database_url)
                .await
                .map_err(|e| e.to_string())?
                .ok_or("another lake writer holds the lease")?;
            let src = Source::connect(&db.database_url)
                .await
                .map_err(|e| e.to_string())?;
            let mut w = Writer::new(src, lake, kek_of(&kek_file)?, Settings::default());
            let started = Instant::now();
            let batches = w.catch_up().await.map_err(|e| e.to_string())?;
            let events: usize = batches.iter().map(|b| b.events).sum();
            w.refresh_telemetry().await.map_err(|e| e.to_string())?;
            println!(
                "drained {} batches, {events} events in {} ms; frontier {}",
                batches.len(),
                started.elapsed().as_millis(),
                w.frontier().await.map_err(|e| e.to_string())?
            );
            lease.release().await.map_err(|e| e.to_string())?;
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Run {
            db,
            owner_url,
            kek_file,
            metrics_addr,
            poll_ms,
            max_batch,
            telemetry_every_s,
            maintain_every_s,
            retention_hours,
        } => {
            let lake = lake_of(cli.lake.as_ref())?;
            let kek = kek_of(&kek_file)?;
            if let Some(owner) = owner_url {
                // Grant before the privilege check reads anything.
                let role = Source::role_of(&db.database_url)
                    .await
                    .map_err(|e| e.to_string())?;
                Source::grant(&owner, &role)
                    .await
                    .map_err(|e| e.to_string())?;
                tracing::info!(event = "lake_role_granted", role);
            }
            let src = Source::connect(&db.database_url)
                .await
                .map_err(|e| e.to_string())?;
            let loop_cfg = Loop {
                url: db.database_url,
                metrics_addr,
                poll: Duration::from_millis(poll_ms),
                telemetry_every: Duration::from_secs(telemetry_every_s),
                maintain_every: Duration::from_secs(maintain_every_s),
                retention: chrono::Duration::hours(retention_hours),
            };
            run(
                Writer::new(src, lake, kek, Settings { max_batch }),
                loop_cfg,
            )
            .await
        }
    }
}

fn keygen(out: &PathBuf) -> Result<(), String> {
    use std::io::Write as _;
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt as _;
    let hex = Kek::generate_hex().map_err(|e| e.to_string())?;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    opts.mode(0o600);
    let mut f = opts
        .open(out)
        .map_err(|e| format!("{}: {e}", out.display()))?;
    f.write_all(hex.as_bytes()).map_err(|e| e.to_string())
}

struct Loop {
    url: String,
    metrics_addr: String,
    poll: Duration,
    telemetry_every: Duration,
    maintain_every: Duration,
    retention: chrono::Duration,
}

async fn run(mut writer: Writer, cfg: Loop) -> Result<ExitCode, String> {
    let mut registry = Registry::default();
    writer.metrics.register(&mut registry);
    let registry = Arc::new(registry);
    let listener = tokio::net::TcpListener::bind(&cfg.metrics_addr)
        .await
        .map_err(|e| format!("metrics listener {}: {e}", cfg.metrics_addr))?;
    tokio::spawn(serve_metrics(listener, registry));

    let mut status = Status {
        pid: std::process::id(),
        ..Status::default()
    };
    let mut lease: Option<Lease> = None;
    let mut last_telemetry: Option<Instant> = None;
    let mut last_maintain = Instant::now();
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!(event = "lake_writer_stopping");
                if let Some(l) = lease.take() { let _ = l.release().await; }
                return Ok(ExitCode::SUCCESS);
            }
            () = tokio::time::sleep(cfg.poll) => {}
        }
        // The lease: acquire it, or check it is still ours.
        let held = match lease.as_mut() {
            Some(l) => l.alive().await,
            None => false,
        };
        if !held {
            lease = None;
            match Lease::try_acquire(&cfg.url).await {
                Ok(Some(l)) => {
                    tracing::info!(event = "lake_lease_acquired");
                    lease = Some(l);
                }
                Ok(None) => continue,
                Err(e) => {
                    tracing::warn!(
                        event = "lake_lease_unavailable",
                        class = WriterError::from(e).class()
                    );
                    continue;
                }
            }
        }
        let result = async {
            writer.catch_up().await?;
            status.lag_seconds = writer.lag_seconds().await?;
            if last_telemetry.is_none_or(|t| t.elapsed() >= cfg.telemetry_every) {
                writer.refresh_telemetry().await?;
                last_telemetry = Some(Instant::now());
            }
            if last_maintain.elapsed() >= cfg.maintain_every {
                writer.maintain(cfg.retention).await?;
                last_maintain = Instant::now();
                status.last_maintenance_at = Some(chrono::Utc::now().timestamp());
            }
            Ok::<(), WriterError>(())
        }
        .await;
        if let Err(e) = result {
            status.failed_commits += 1;
            status.last_failure_class = Some(e.class().to_owned());
            status.last_failure_at = Some(chrono::Utc::now().timestamp());
            tracing::warn!(event = "lake_tick_failed", class = e.class());
        }
        status.at = chrono::Utc::now().timestamp();
        status.frontier = writer.metrics.frontier.get();
        status.horizon = writer.metrics.horizon.get();
        if let Err(e) = status.write(&writer.lake) {
            tracing::warn!(event = "lake_status_unwritten", error = %e);
        }
    }
}

async fn serve_metrics(listener: tokio::net::TcpListener, registry: Arc<Registry>) {
    loop {
        let Ok((mut sock, _)) = listener.accept().await else {
            continue;
        };
        let registry = Arc::clone(&registry);
        tokio::spawn(async move {
            let mut buf = [0_u8; 1024];
            let _ = sock.read(&mut buf).await;
            let mut body = String::new();
            let _ = prometheus_client::encoding::text::encode(&mut body, &registry);
            let resp = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/openmetrics-text; version=1.0.0; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
        });
    }
}

/// The doctor's lines (ADR-0060 d16): `ok`, `warn` or `fail`, a check name
/// and a sentence; exit 1 when any line fails.
async fn doctor(
    lake: &Lake,
    url: &str,
    lag_rows_warn: i64,
    lag_seconds_warn: i64,
) -> Result<ExitCode, String> {
    let mut failed = false;
    let mut line = |level: &str, check: &str, msg: String| {
        if level == "fail" {
            failed = true;
        }
        println!("{level:<4} {check}: {msg}");
    };
    let src = match Source::connect(url).await {
        Ok(s) => s,
        Err(e) => {
            line(
                "fail",
                "lake.postgres",
                format!(
                    "the writer's role cannot connect ({})",
                    WriterError::from(e).class()
                ),
            );
            return Ok(ExitCode::FAILURE);
        }
    };
    match src.lease_held().await {
        Ok(true) => line("ok", "lake.writer", "a writer holds the lease".to_owned()),
        Ok(false) => line(
            "fail",
            "lake.writer",
            "no writer holds the lease: the lake writer is down".to_owned(),
        ),
        Err(e) => line(
            "fail",
            "lake.writer",
            format!("lease unreadable ({})", WriterError::from(e).class()),
        ),
    }
    let frontier = lake
        .applied(oml_lake::Prefix::Provider, &oml_lake::table::LAKE_BATCHES)
        .await
        .map_err(|e| e.to_string())?;
    match src.horizon().await {
        Ok(Some(h)) => {
            let rows = (h - frontier).max(0);
            let level = if rows > lag_rows_warn { "warn" } else { "ok" };
            line(
                level,
                "lake.lag",
                format!("{rows} outbox rows past the frontier {frontier} (stable prefix {h})"),
            );
        }
        Ok(None) => line(
            "warn",
            "lake.lag",
            "the outbox's stable prefix is held by a long transaction".to_owned(),
        ),
        Err(e) => line(
            "fail",
            "lake.lag",
            format!("outbox unreadable ({})", WriterError::from(e).class()),
        ),
    }
    match Status::read(lake) {
        None => line(
            "fail",
            "lake.heartbeat",
            "the writer has written no status".to_owned(),
        ),
        Some(s) => {
            let age = chrono::Utc::now().timestamp() - s.at;
            line(
                if age > 60 { "fail" } else { "ok" },
                "lake.heartbeat",
                format!("last tick {age} s ago"),
            );
            line(
                if s.lag_seconds > lag_seconds_warn {
                    "warn"
                } else {
                    "ok"
                },
                "lake.lag_seconds",
                format!("oldest undrained outbox row is {} s old", s.lag_seconds),
            );
            line(
                if s.failed_commits > 0 { "warn" } else { "ok" },
                "lake.commits",
                match &s.last_failure_class {
                    Some(c) => format!(
                        "{} failed ticks since start, the last of class {c}",
                        s.failed_commits
                    ),
                    None => "no failed commit since start".to_owned(),
                },
            );
            line(
                "ok",
                "lake.maintenance",
                s.last_maintenance_at
                    .map_or("not run since start".to_owned(), |t| {
                        format!("last ran {} s ago", chrono::Utc::now().timestamp() - t)
                    }),
            );
        }
    }
    let mut drifted = Vec::new();
    for (prefix, spec) in lake.existing().map_err(|e| e.to_string())? {
        let cols = lake.drift(prefix, spec).await.map_err(|e| e.to_string())?;
        if !cols.is_empty() {
            drifted.push(format!("{} ({})", spec.name, cols.join(", ")));
        }
    }
    if drifted.is_empty() {
        line(
            "ok",
            "lake.schema",
            "every table matches its columns".to_owned(),
        );
    } else {
        line(
            "fail",
            "lake.schema",
            format!("schema drift in {}", drifted.join("; ")),
        );
    }
    match src.dead_letters().await {
        Ok(0) => line(
            "ok",
            "lake.dead_letter",
            "no outbox row is dead-lettered".to_owned(),
        ),
        Ok(n) => line(
            "warn",
            "lake.dead_letter",
            format!("{n} outbox rows are dead-lettered and absent from the lake until repaired"),
        ),
        Err(e) => line(
            "fail",
            "lake.dead_letter",
            format!("unreadable ({})", WriterError::from(e).class()),
        ),
    }
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}
