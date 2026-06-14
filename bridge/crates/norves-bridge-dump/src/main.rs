//! `bridge-dump` — human-readable Bridge envelope dumper.
//!
//! Reads a JSON envelope (from a file or stdin), decodes it into a
//! [`ValidatedEnvelope`], and prints a structured, human-readable summary to
//! stdout.  Decode failures are reported to stderr and exit with a non-zero
//! status.
//!
//! This binary is intentionally **synchronous and tokio-free**. All I/O is
//! standard blocking I/O; the only runtime dependency beyond the standard
//! library is `norves-bridge-core` (which is itself tokio-free).
//!
//! Usage:
//!   bridge-dump --file path/to/envelope.json
//!   cat envelope.json | bridge-dump

use std::fs;
use std::io::{self, Read};
use std::process;

use anyhow::{Context, Result};
use clap::Parser;
use norves_bridge_core::{decode_typed, ResponsePayload, ValidatedEnvelope};

/// Dump a Bridge protocol envelope as human-readable text.
#[derive(Debug, Parser)]
#[command(name = "bridge-dump", about = "Decode and display a Bridge envelope")]
struct Cli {
    /// Path to a JSON file containing a single Bridge envelope.
    /// If omitted, the envelope is read from stdin.
    #[arg(long, value_name = "PATH")]
    file: Option<String>,
}

fn main() {
    let cli = Cli::parse();

    match run(&cli) {
        Ok(()) => {}
        Err(err) => {
            eprintln!("Error: {err:#}");
            process::exit(1);
        }
    }
}

fn run(cli: &Cli) -> Result<()> {
    let json = read_input(cli.file.as_deref())?;
    let validated = decode_typed(&json).with_context(|| "failed to decode envelope")?;
    print_envelope(&validated);
    Ok(())
}

/// Read the envelope JSON from a file or from stdin.
fn read_input(file: Option<&str>) -> Result<String> {
    match file {
        Some(path) => {
            fs::read_to_string(path).with_context(|| format!("failed to read file: {path}"))
        }
        None => {
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .context("failed to read from stdin")?;
            Ok(buf)
        }
    }
}

/// Format an optional JSON map as indented JSON, or "(none)" if absent.
fn fmt_params(params: &Option<serde_json::Map<String, serde_json::Value>>) -> String {
    match params {
        None => "(none)".to_owned(),
        Some(map) => {
            let value = serde_json::Value::Object(map.clone());
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| "(serialization error)".into())
        }
    }
}

/// Print a human-readable summary of a validated Bridge envelope to stdout.
fn print_envelope(env: &ValidatedEnvelope) {
    match env {
        ValidatedEnvelope::Request {
            version,
            id,
            method,
            params,
            session_id,
            seq,
        } => {
            println!("Kind      : request");
            println!("Version   : {}", version.as_str());
            println!("ID        : {}", id.as_str());
            println!("Method    : {}", method.as_str());
            if let Some(sid) = session_id {
                println!("SessionID : {sid}");
            }
            if let Some(n) = seq {
                println!("Seq       : {n}");
            }
            println!("Params    :\n{}", indent(&fmt_params(params)));
        }
        ValidatedEnvelope::Response {
            version,
            id,
            payload,
            session_id,
            seq,
        } => {
            println!("Kind      : response");
            println!("Version   : {}", version.as_str());
            println!("ID        : {}", id.as_str());
            if let Some(sid) = session_id {
                println!("SessionID : {sid}");
            }
            if let Some(n) = seq {
                println!("Seq       : {n}");
            }
            match payload {
                ResponsePayload::Result(value) => {
                    let pretty = serde_json::to_string_pretty(value)
                        .unwrap_or_else(|_| "(serialization error)".into());
                    println!("Payload   : result");
                    println!("{}", indent(&pretty));
                }
                ResponsePayload::Error(err) => {
                    println!("Payload   : error");
                    println!("  Code    : {}", err.code.as_str());
                    println!("  Message : {}", err.message);
                    if let Some(data) = &err.data {
                        let pretty = serde_json::to_string_pretty(data)
                            .unwrap_or_else(|_| "(serialization error)".into());
                        println!("  Data    :\n{}", indent(&pretty));
                    }
                }
            }
        }
        ValidatedEnvelope::Event {
            version,
            event,
            params,
            session_id,
            seq,
        } => {
            println!("Kind      : event");
            println!("Version   : {}", version.as_str());
            println!("Event     : {}", event.as_str());
            if let Some(sid) = session_id {
                println!("SessionID : {sid}");
            }
            if let Some(n) = seq {
                println!("Seq       : {n}");
            }
            println!("Params    :\n{}", indent(&fmt_params(params)));
        }
        // Non-exhaustive match arm: handle any future kinds gracefully.
        _ => {
            println!("Kind      : (unknown future variant)");
        }
    }
}

/// Indent every line of a multi-line string by two spaces.
fn indent(s: &str) -> String {
    s.lines()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}
