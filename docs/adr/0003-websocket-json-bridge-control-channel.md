# 0003: WebSocket + JSON Bridge Control Channel

Status: Accepted for alpha

## Context

The alpha needs a reliable local bidirectional channel that is easy to inspect, fixture, and implement across Rust, TypeScript, and C++.

## Decision

Use WebSocket as the primary alpha transport and JSON text frames as the canonical debug codec. Use a NorvesEditor Bridge envelope inspired by JSON-RPC request/response/event patterns rather than strict JSON-RPC 2.0 compliance.

## Consequences

Every wire message should be represented by JSON Schema and golden fixtures. Binary codecs, protobuf, UDP telemetry, and large-payload streaming are post-alpha topics unless a reviewed ADR changes scope.
