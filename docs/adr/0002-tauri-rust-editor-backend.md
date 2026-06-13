# 0002: Tauri Rust Editor Backend

Status: Accepted for alpha

## Context

NorvesEditor needs a desktop shell with modern UI iteration, native process/filesystem integration, and a backend that can own long-lived engine sessions.

## Decision

Use Tauri 2 for the app shell. Use Rust for the editor backend. The TypeScript frontend communicates with Rust through Tauri commands and events.

## Consequences

The Rust backend owns engine process lifecycle, Bridge connection state, reconnect behavior, and event fan-out. The TypeScript UI must not directly manage raw WebSocket transport or engine process execution.
