# Alpha Scope

The alpha target is a complete vertical slice for engine connection and runtime control, not a full scene editor.

## Happy Path

```text
1. Start NorvesEditor.
2. Open or configure a local workspace.
3. Select an engine executable and launch arguments.
4. Press Launch in the Game View panel.
5. The Rust backend starts or attaches to the engine process.
6. The engine exposes a local Bridge endpoint.
7. NorvesEditor connects over WebSocket + JSON.
8. The editor receives hello, capabilities, status, and log events.
9. The Game View panel shows process, connection, and runtime state.
10. Play, Pause, Stop, Reconnect, Stop Process, and Focus Window controls work.
```

## Alpha Exit Criteria

```text
- Tauri desktop app builds and runs.
- Rust backend owns process lifecycle and Bridge connection state.
- TypeScript frontend uses typed Tauri command/event wrappers.
- Bridge protocol has JSON Schema and golden fixtures.
- Rust bridge client passes fixture and loopback tests.
- C++ engine SDK can host a mock Bridge endpoint.
- Editor connects to the C++ mock engine over WebSocket + JSON.
- Editor launches an external engine executable through controlled backend paths.
- Logs/status/runtime state are visible in the UI.
- NorvesLib reference integration is working or has a documented adapter task list after mock-engine parity.
```

## Non-Goals

```text
- Embedded native/GPU viewport inside Tauri.
- Full scene hierarchy editing.
- Complete inspector/property editor.
- Asset import pipeline.
- Undo/redo transaction system.
- Graph editors, shader graph, visual scripting, timeline, or animation tools.
- Remote Internet engine connections.
- Binary/protobuf default transport.
- UDP telemetry.
- Norves-gRPC or online multiplayer integration.
- Public "any editor, any engine" standardization.
```
