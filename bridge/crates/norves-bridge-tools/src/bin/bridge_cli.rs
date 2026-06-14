//! `bridge-cli` — manual debug CLI for the NorvesEditor Bridge.
//!
//! Connects to a running engine (mock or real) over WebSocket and exercises the
//! Bridge protocol. Intended for local debugging; not part of the editor runtime.
//!
//! # Subcommands
//!
//! * `connect` — dial, perform `bridge.hello`, print the [`HelloOutcome`], and
//!   exit cleanly.
//! * `ping` — dial, hello, then `engine.getStatus`, print the [`StatusSnapshot`].
//! * `log` — dial, hello, then `log.subscribe`; receive `--count` log.message
//!   events and print each, then exit.
//!
//! # Diagnostics
//!
//! Internal tracing is gated by `RUST_LOG` (e.g. `RUST_LOG=debug`). Human-
//! readable CLI output goes to `println!` (permitted for CLI output per rust.md).

use std::time::Duration;

use anyhow::{Context as _, Result};
use clap::{Parser, Subcommand};
use norves_bridge_core::{
    CorrelationId, MethodName, ResponsePayload, ValidatedEnvelope, VersionString,
};
use norves_bridge_editor_client::{
    connect_with_retry, parse_hello_result, parse_log_message, parse_status_result, HelloParams,
    RetryConfig,
};
use tracing::debug;

/// Default WebSocket URL for a locally-running mock engine.
const DEFAULT_URL: &str = "ws://127.0.0.1:38080";

/// Timeout for a single request/response round trip.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// The protocol version the editor supports.
fn protocol_version() -> VersionString {
    // "0.1" always satisfies the VersionString invariant (non-empty, valid).
    VersionString::try_from("0.1".to_owned()).expect("0.1 is a valid version string")
}

/// Builds a request-kind [`ValidatedEnvelope`] with the supplied `id`, `method`,
/// and optional `params`. Mirrors the pattern in `ws_roundtrip.rs` /
/// `loopback_roundtrip.rs`.
fn build_request(
    id: &str,
    method: &str,
    params: Option<serde_json::Map<String, serde_json::Value>>,
) -> Result<ValidatedEnvelope> {
    let id = CorrelationId::try_from(id.to_owned())
        .map_err(|e| anyhow::anyhow!("invalid correlation id {id:?}: {e}"))?;
    let method = MethodName::try_from(method.to_owned())
        .map_err(|e| anyhow::anyhow!("invalid method name {method:?}: {e}"))?;
    Ok(ValidatedEnvelope::Request {
        version: protocol_version(),
        id,
        method,
        params,
        session_id: None,
        seq: None,
    })
}

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

/// NorvesEditor Bridge debug CLI.
///
/// Connects to a running engine (or mock engine) over WebSocket and exercises
/// the Bridge protocol. Useful for manual diagnostics without the full editor.
#[derive(Parser, Debug)]
#[command(name = "bridge-cli", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Dial the engine, perform the bridge.hello handshake, and print the result.
    Connect {
        /// WebSocket URL of the engine.
        #[arg(long, default_value = DEFAULT_URL)]
        url: String,
    },

    /// Dial the engine, handshake, then query engine.getStatus.
    Ping {
        /// WebSocket URL of the engine.
        #[arg(long, default_value = DEFAULT_URL)]
        url: String,
    },

    /// Dial the engine, handshake, subscribe to logs, and print received events.
    Log {
        /// WebSocket URL of the engine.
        #[arg(long, default_value = DEFAULT_URL)]
        url: String,

        /// Number of log.message events to receive before exiting.
        #[arg(long, default_value_t = 3)]
        count: usize,
    },
}

// ---------------------------------------------------------------------------
// Connection helper
// ---------------------------------------------------------------------------

/// Dials `url` with a generous retry budget and returns the live handle plus
/// the negotiated [`HelloOutcome`].
async fn dial_and_hello(
    url: &str,
) -> Result<(
    norves_bridge_editor_client::DispatchHandle,
    norves_bridge_editor_client::HelloOutcome,
)> {
    let cfg = RetryConfig {
        initial_backoff: Duration::from_millis(100),
        max_backoff: Duration::from_secs(2),
        max_elapsed: Duration::from_secs(15),
        jitter: true,
    };

    println!("Connecting to {url} ...");
    let handle = connect_with_retry(url, &cfg)
        .await
        .with_context(|| format!("failed to connect to {url}"))?;
    debug!("dispatcher spawned");

    // Build and send bridge.hello.
    let hello_params = HelloParams::new("NorvesEditor", vec![protocol_version()])
        .to_params()
        .context("failed to serialize bridge.hello params")?;

    let hello_req = build_request("req-hello", "bridge.hello", Some(hello_params))
        .context("failed to build bridge.hello request")?;

    let hello_response = handle
        .request(hello_req, REQUEST_TIMEOUT)
        .await
        .context("bridge.hello request failed")?;

    let hello_result = match hello_response {
        ResponsePayload::Result(value) => value,
        ResponsePayload::Error(err) => {
            anyhow::bail!("bridge.hello engine error: {err:?}")
        }
    };

    let outcome =
        parse_hello_result(&hello_result).context("failed to parse bridge.hello result")?;

    Ok((handle, outcome))
}

// ---------------------------------------------------------------------------
// Subcommand implementations
// ---------------------------------------------------------------------------

async fn cmd_connect(url: &str) -> Result<()> {
    let (handle, outcome) = dial_and_hello(url).await?;

    println!("--- bridge.hello ---");
    println!("  session_id       : {}", outcome.session_id);
    println!("  protocol_version : {}", outcome.protocol_version.as_str());
    println!("  server_name      : {}", outcome.server_name);
    if let Some(ver) = &outcome.server_version {
        println!("  server_version   : {ver}");
    }
    if let Some(eng) = &outcome.server_engine {
        println!("  server_engine    : {eng}");
    }

    handle.shutdown().await;
    println!("Done.");
    Ok(())
}

async fn cmd_ping(url: &str) -> Result<()> {
    let (handle, outcome) = dial_and_hello(url).await?;

    println!("--- bridge.hello ---");
    println!("  session_id       : {}", outcome.session_id);
    println!("  protocol_version : {}", outcome.protocol_version.as_str());
    println!("  server_name      : {}", outcome.server_name);

    // engine.getStatus
    let status_req = build_request("req-status", "engine.getStatus", None)
        .context("failed to build engine.getStatus request")?;

    let status_response = handle
        .request(status_req, REQUEST_TIMEOUT)
        .await
        .context("engine.getStatus request failed")?;

    let status_result = match status_response {
        ResponsePayload::Result(value) => value,
        ResponsePayload::Error(err) => anyhow::bail!("engine.getStatus engine error: {err:?}"),
    };

    let snapshot =
        parse_status_result(&status_result).context("failed to parse engine.getStatus result")?;

    println!("--- engine.getStatus ---");
    println!("  engine_state  : {:?}", snapshot.engine_state);
    println!("  runtime_state : {:?}", snapshot.runtime_state);
    if let Some(name) = &snapshot.engine_name {
        println!("  engine_name   : {name}");
    }
    if let Some(ver) = &snapshot.engine_version {
        println!("  engine_version: {ver}");
    }
    if let Some(title) = &snapshot.title {
        println!("  title         : {title}");
    }

    handle.shutdown().await;
    println!("Done.");
    Ok(())
}

async fn cmd_log(url: &str, count: usize) -> Result<()> {
    let (handle, outcome) = dial_and_hello(url).await?;

    println!("--- bridge.hello ---");
    println!("  session_id       : {}", outcome.session_id);
    println!("  protocol_version : {}", outcome.protocol_version.as_str());
    println!("  server_name      : {}", outcome.server_name);

    // Subscribe to broadcast events BEFORE sending log.subscribe so no event
    // is missed between the server's ack and the first event (matches the
    // ordering discipline in loopback_roundtrip.rs).
    let mut events = handle.subscribe_events();

    // log.subscribe
    let sub_req = build_request("req-logsub", "log.subscribe", None)
        .context("failed to build log.subscribe request")?;

    let sub_response = handle
        .request(sub_req, REQUEST_TIMEOUT)
        .await
        .context("log.subscribe request failed")?;

    match sub_response {
        ResponsePayload::Result(_) => {
            println!("log.subscribe accepted; waiting for {count} log.message event(s) ...");
        }
        ResponsePayload::Error(err) => anyhow::bail!("log.subscribe engine error: {err:?}"),
    }

    // Receive `count` log.message events.
    for i in 0..count {
        let event = tokio::time::timeout(REQUEST_TIMEOUT, events.recv())
            .await
            .with_context(|| format!("timed out waiting for log.message event #{}", i + 1))?
            .with_context(|| format!("event channel closed before event #{}", i + 1))?;

        let log = match event.as_ref() {
            ValidatedEnvelope::Event { event, params, .. } => {
                if event.as_str() != "log.message" {
                    debug!(event = event.as_str(), "skipping non-log.message event");
                    // Re-enter the loop without advancing `i` would be complex; for the
                    // CLI we count all events received and print whatever arrives.
                    println!(
                        "[{}] (skipped non-log.message event: {})",
                        i + 1,
                        event.as_str()
                    );
                    continue;
                }
                let params = params
                    .clone()
                    .with_context(|| format!("log.message event #{} has no params", i + 1))?;
                parse_log_message(&serde_json::Value::Object(params))
                    .with_context(|| format!("failed to parse log.message params #{}", i + 1))?
            }
            other => {
                debug!(?other, "unexpected envelope kind in event stream");
                println!("[{}] (unexpected envelope kind, skipping)", i + 1);
                continue;
            }
        };

        let category = log.category.as_deref().unwrap_or("-");
        let timestamp = log.timestamp.as_deref().unwrap_or("-");
        println!(
            "[{}] {:?} [{}] {} (ts: {})",
            i + 1,
            log.level,
            category,
            log.message,
            timestamp
        );
    }

    handle.shutdown().await;
    println!("Done.");
    Ok(())
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize the tracing subscriber so RUST_LOG-controlled diagnostics work.
    // `try_init` avoids a panic if a global subscriber is already installed.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let cli = Cli::parse();

    match &cli.command {
        Commands::Connect { url } => cmd_connect(url).await,
        Commands::Ping { url } => cmd_ping(url).await,
        Commands::Log { url, count } => cmd_log(url, *count).await,
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory as _;

    // 1. clap derive 定義の健全性チェック。
    // `debug_assert` は clap の公式推奨手順で、derive マクロが生成した
    // コマンド定義に矛盾がないことを確認する。
    #[test]
    fn cli_command_debug_assert() {
        Cli::command().debug_assert();
    }

    // 2-a. `log` サブコマンドのパース: `--count 5` が count=5 になること。
    #[test]
    fn parse_log_subcommand_count() {
        let cli = Cli::try_parse_from(["bridge-cli", "log", "--count", "5"])
            .expect("log --count 5 must parse successfully");
        match cli.command {
            Commands::Log { url, count } => {
                assert_eq!(count, 5, "count must be 5");
                assert_eq!(url, DEFAULT_URL, "url must default to {DEFAULT_URL}");
            }
            other => panic!("expected Log, got {other:?}"),
        }
    }

    // 2-b. `ping` のデフォルト url が ws://127.0.0.1:38080 になること。
    #[test]
    fn parse_ping_default_url() {
        let cli =
            Cli::try_parse_from(["bridge-cli", "ping"]).expect("ping must parse successfully");
        match cli.command {
            Commands::Ping { url } => {
                assert_eq!(url, DEFAULT_URL, "url must default to {DEFAULT_URL}");
            }
            other => panic!("expected Ping, got {other:?}"),
        }
    }

    // 2-c. `connect` のデフォルト url が ws://127.0.0.1:38080 になること。
    #[test]
    fn parse_connect_default_url() {
        let cli = Cli::try_parse_from(["bridge-cli", "connect"])
            .expect("connect must parse successfully");
        match cli.command {
            Commands::Connect { url } => {
                assert_eq!(url, DEFAULT_URL, "url must default to {DEFAULT_URL}");
            }
            other => panic!("expected Connect, got {other:?}"),
        }
    }

    // 3-a. `build_request` の正常系: 有効な method/params で
    // `ValidatedEnvelope::Request` が組めること。
    #[test]
    fn build_request_valid_method_returns_request_envelope() {
        let result = build_request("req-1", "bridge.hello", None);
        let env = result.expect("bridge.hello must be a valid method name");
        assert!(
            matches!(env, ValidatedEnvelope::Request { .. }),
            "must produce a Request variant"
        );

        // params 付きでも正常に組めること。
        let mut params = serde_json::Map::new();
        params.insert("key".to_owned(), serde_json::Value::Bool(true));
        let result2 = build_request("req-2", "engine.getStatus", Some(params));
        assert!(result2.is_ok(), "engine.getStatus with params must succeed");
    }

    // 3-b. `build_request` の異常系: 不正な method 名では Err になること。
    // MethodName::try_from はドット区切りの namespaced token でないものを弾く。
    #[test]
    fn build_request_invalid_method_returns_err() {
        // ドット無し
        assert!(
            build_request("req-1", "nodot", None).is_err(),
            "method without dot must be rejected"
        );
        // 空文字
        assert!(
            build_request("req-1", "", None).is_err(),
            "empty method must be rejected"
        );
        // 先頭が大文字
        assert!(
            build_request("req-1", "Bridge.hello", None).is_err(),
            "uppercase-leading method must be rejected"
        );
        // ドットが複数
        assert!(
            build_request("req-1", "a.b.c", None).is_err(),
            "doubly-namespaced method must be rejected"
        );
    }
}
