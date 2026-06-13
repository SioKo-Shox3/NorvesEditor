# Viewport Strategy

The alpha uses an external engine-owned native viewport window. NorvesEditor provides a Game View panel that controls and reflects that external window, but does not embed GPU output inside the Tauri WebView.

## Alpha Responsibilities

The Game View panel owns editor-side controls and status display:

```text
- Launch engine process.
- Stop engine process.
- Reconnect Bridge session.
- Send Play, Pause, Stop, and Focus Window requests.
- Display process state, connection state, runtime state, PID, endpoint, and recent logs/status.
```

The Rust backend owns actual process launch, process termination, reconnect behavior, and best-effort focus/raise logic. The TypeScript UI does not directly spawn processes.

## Post-Alpha Research

```text
- Native child-window embedding.
- Shared GPU textures.
- Frame streaming or screenshots/thumbnails.
- Docked render target composition.
- Platform-specific focus and parenting behavior.
```

Do not let post-alpha viewport research block the alpha connection/control slice.
