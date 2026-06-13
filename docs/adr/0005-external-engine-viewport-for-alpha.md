# 0005: External Engine Viewport For Alpha

Status: Accepted for alpha

## Context

Embedding an engine-owned Vulkan, DirectX, or other native GPU viewport inside a Tauri WebView is platform-specific and high risk. The alpha only needs to prove the editor-to-engine connection/control loop.

## Decision

Use an external engine-owned native viewport window for alpha. NorvesEditor provides a Game View panel for launch, connection, runtime control, state display, logs, and best-effort focus/raise.

## Consequences

Native viewport embedding, shared GPU textures, frame streaming, and docked render target composition are post-alpha research topics. The alpha should not block on them.
