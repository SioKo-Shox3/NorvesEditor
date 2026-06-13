# 0004: Standalone C++ Engine SDK

Status: Accepted for alpha

## Context

Target engines, including NorvesLib, are expected to expose C++ integration points. Engine authors should not need to embed Rust or TypeScript into the engine process for alpha.

## Decision

Provide a standalone C++ engine-side SDK under `bridge/cpp/engine-sdk`. The SDK is generic and does not depend on NorvesLib.

## Consequences

C++20 is the minimum language level. Public headers must not expose NorvesLib types or third-party WebSocket implementation types. Engine adapters must convert engine state into Bridge DTOs and keep ownership/lifetime explicit.
