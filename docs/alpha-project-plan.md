# NorvesEditor Alpha Project Plan

Codex / Claude / other coding agents should treat this document as the project-level plan for the first usable NorvesEditor alpha. This plan intentionally folds the former `NorvesBridge` idea into the `NorvesEditor` repository as a bridge subsystem.

---

## 1. Product Positioning

NorvesEditor is a modern desktop game editor. Its first alpha target is not a full scene editor. The alpha target is a narrow but complete vertical slice:

> NorvesEditor can launch or attach to a C++ engine process, establish a Bridge connection, display engine status/logs, expose a Game View panel, and start/control the game from that panel.

The bridge layer is not a separate product at this stage. It is a subsystem inside NorvesEditor.

```text
NorvesEditor
  apps/editor             Tauri application and UI
  bridge/                 Engine connection subsystem
    spec/                 Protocol schema and fixtures
    crates/               Rust editor-side runtime
    ts/                   TypeScript UI-facing API
    cpp/                  C++ engine-side SDK and mock engine
    tools/                Bridge inspector and conformance tools
```

`NorvesLib` is the first reference engine integration, but NorvesEditor must not become hard-wired to NorvesLib internals. The C++ engine-side SDK is the boundary. NorvesLib-specific code lives in a NorvesLib adapter.

---

## 2. Alpha Goal

### 2.1 Alpha user story

A developer opens NorvesEditor, selects a project or engine executable, presses a launch button in the Game View panel, and sees the engine game window start. NorvesEditor connects to the engine through the bridge protocol, receives status/log messages, and can send basic runtime commands.

Minimum happy path:

```text
1. Start NorvesEditor.
2. Open or create a local workspace.
3. Configure a C++ engine executable and launch arguments.
4. Press Launch in the Game View panel.
5. NorvesEditor starts the engine process or attaches to an already running engine.
6. Engine exposes a local Bridge endpoint.
7. NorvesEditor connects over WebSocket + JSON.
8. Editor receives hello/capabilities/status/log events.
9. Game View panel shows connection state and runtime controls.
10. User can request Play / Pause / Stop or engine-defined equivalent commands.
11. User can focus or relaunch the external engine viewport window.
```

### 2.2 What “viewport” means for alpha

The alpha does **not** attempt to embed a Vulkan/DirectX/native game viewport inside Tauri's WebView. The alpha “viewport-like” surface is a Game View panel that controls and reflects an external engine viewport window.

Alpha Game View panel responsibilities:

```text
- Launch engine process.
- Show process state: not started, starting, running, exited, crashed.
- Show bridge state: disconnected, connecting, connected, reconnecting.
- Show runtime state: edit, play, paused, stopped, unknown.
- Provide Launch / Stop Process / Reconnect / Play / Pause / Stop / Focus Window controls.
- Display last known engine title, PID, endpoint, and recent status events.
- Optionally show a placeholder, screenshot, or thumbnail if the engine later provides one.
```

Native viewport embedding, shared GPU textures, frame streaming, and docked render target composition are explicitly post-alpha topics.

### 2.3 Alpha exit criteria

Alpha is accepted when all of the following are true:

```text
- NorvesEditor builds and runs as a Tauri desktop app.
- Editor UI has a stable app shell with at least Game View, Log, Connection, and Settings/Workspace panels.
- Tauri Rust backend owns engine process lifecycle and Bridge connection state.
- TypeScript frontend uses typed Tauri command wrappers and event subscriptions, not raw backend state.
- Bridge protocol has JSON Schema files and golden fixtures.
- Rust bridge client passes fixture and loopback tests.
- C++ engine SDK can host a mock engine Bridge endpoint.
- Editor can connect to the C++ mock engine over WebSocket + JSON.
- Editor can launch an external engine executable through a controlled Tauri backend path.
- Editor can display engine logs/status and send runtime commands.
- NorvesLib reference integration is either working or has a documented adapter task list with mock-engine parity already proven.
```

For an internal alpha intended to motivate further editor work, mock-engine parity is acceptable. For a public-facing alpha, NorvesLib integration should be included.

---

## 3. Non-Goals for Alpha

The alpha must not attempt to solve the whole editor problem.

Out of scope:

```text
- Fully embedded native/GPU viewport in the Tauri window.
- Full scene hierarchy editing.
- Complete inspector/property editor for all engine data types.
- Full asset browser and asset import pipeline.
- Undo/redo transaction system.
- Timeline, animation, shader graph, material graph, visual scripting.
- Multi-user editing.
- Remote Internet engine connections.
- Binary/protobuf transport as the default path.
- UDP telemetry channel.
- Norves-gRPC / online multiplayer integration.
- General-purpose “any editor, any engine” public standardization.
```

The bridge subsystem should remain clean enough to extract later, but extraction is not an alpha goal.

---

## 4. Repository Strategy

Create `SioKo-Shox3/NorvesEditor` as the main repository. Do not create a separate `NorvesBridge` repository for the alpha.

Recommended initial tree:

```text
NorvesEditor/
  AGENTS.md
  CLAUDE.md
  README.md
  docs/
    vision.md
    alpha-scope.md
    architecture.md
    technology-decisions.md
    engine-integration.md
    viewport-strategy.md
    memory-buffer-policy.md
    adr/
      0001-editor-owned-bridge-subsystem.md
      0002-tauri-rust-editor-backend.md
      0003-websocket-json-bridge-control-channel.md
      0004-cpp-engine-sdk.md
      0005-external-engine-viewport-for-alpha.md
  apps/
    editor/
      package.json
      src/
      src-tauri/
  bridge/
    spec/
      schema/
      fixtures/
      docs/
    crates/
      norves-bridge-core/
      norves-bridge-editor-client/
      norves-bridge-tools/
    ts/
      packages/
        bridge-ui/
        bridge-types/
    cpp/
      engine-sdk/
      examples/
        mock-engine/
    tools/
      bridge-inspector/
    conformance/
      fixtures/
      runners/
  scripts/
  tests/
```

`bridge/` is a separate layer, not a separate repository. Keep the API boundary strict enough that it can be extracted later if it becomes useful.

---

## 5. Technical Decisions

### 5.1 Application shell: Tauri 2 + Rust backend + TypeScript frontend

Decision:

```text
NorvesEditor app shell: Tauri 2
Backend: Rust
Frontend: TypeScript + Vite + React by default unless a later ADR changes it
```

Rationale:

```text
- Tauri lets the editor use web UI technologies while keeping native backend logic in Rust.
- Rust backend is a good fit for process management, Bridge session management, reconnect, task orchestration, and filesystem/project operations.
- TypeScript frontend is suitable for modern editor UI iteration, docking, command palette, log panels, inspector panels, and styling.
- Editor-side Bridge state should not live directly in the web UI transport layer. The Rust backend should own the connection; TypeScript should call commands and subscribe to events.
```

Relevant upstream facts:

```text
- Tauri supports frontend frameworks that compile to HTML, JavaScript, and CSS while using languages such as Rust for backend logic.
- Tauri provides JavaScript-to-Rust command invocation through `invoke`.
- Tauri allows Rust to emit events to the frontend.
- Tauri shell/sidecar facilities can run external binaries, but permissions must be scoped explicitly.
```

### 5.2 Editor backend runtime: Rust + Tokio

Decision:

```text
Use Rust for editor-side runtime crates and Tokio for async networking/process/session work.
```

Rationale:

```text
- Rust provides strong ownership, memory-safety, and thread-safety guarantees useful for long-lived editor sessions.
- Tokio provides async tasks, channels, timeouts, TCP/UDP I/O, process management, and runtime scheduling.
- Tauri already brings Rust into the application backend, so editor-side bridge runtime can be shared between app backend and command-line tools.
```

### 5.3 Transport: WebSocket + JSON control channel

Decision:

```text
Primary alpha transport: WebSocket.
Canonical alpha codec: JSON text frames.
Protocol shape: JSON-RPC inspired request / response / event envelope.
```

Rationale:

```text
- WebSocket gives a reliable, bidirectional message channel over a familiar local TCP transport.
- JSON is inspectable, fixture-friendly, easy to debug, and convenient across Rust, TypeScript, and C++.
- JSON-RPC provides useful request/response/notification ideas, but NorvesEditor needs custom engine events, roles, capabilities, session state, and future attachments; therefore use a JSON-RPC inspired envelope rather than strict JSON-RPC 2.0 compliance.
```

### 5.4 Protocol validation: JSON Schema + golden fixtures

Decision:

```text
Every Bridge message shape must be represented by JSON Schema and covered by golden fixtures.
```

Rationale:

```text
- JSON Schema provides declarative structure and constraint validation for JSON data.
- Fixtures are the cross-language contract between Rust, TypeScript, and C++.
- New protocol messages must add schema + fixture + Rust test + TypeScript test + C++ conformance test as applicable.
```

### 5.5 C++ engine-side SDK

Decision:

```text
Provide a standalone C++ engine SDK under bridge/cpp/engine-sdk.
```

Rationale:

```text
- Most target engines will expose C++ integration points.
- Engine authors should not have to embed Rust into their engine process for alpha.
- NorvesLib should consume the C++ SDK through a small adapter, not by depending on the editor UI.
- C++ SDK must not include NorvesLib headers or depend on NorvesLib container/object systems.
```

C++ SDK language policy:

```text
- C++20 minimum; C++23 allowed where toolchain support is clear.
- CMake first.
- Standard library is allowed in the standalone SDK.
- NorvesLib-specific rules apply only inside a NorvesLib adapter hosted in the NorvesLib repository.
```

C++ WebSocket implementation policy:

```text
- Do not write a custom RFC 6455 implementation unless a reviewed ADR explicitly chooses it.
- Hide the selected WebSocket library behind an internal transport interface.
- Candidate libraries include IXWebSocket, Boost.Beast, and libwebsockets.
- For alpha, prioritize CMake/vcpkg availability, Windows/MSVC support, simple server implementation, clear callback/threading behavior, and minimal public API leakage.
- The SDK public headers must not expose third-party WebSocket types.
```

### 5.6 Engine process and viewport strategy

Decision:

```text
Alpha uses an external engine viewport window controlled by NorvesEditor.
```

Rationale:

```text
- Native GPU viewport embedding inside a WebView is high-risk and not required to validate editor motivation.
- Existing engines already know how to create their own native render windows.
- NorvesEditor can provide an editor-like Game View panel that launches, connects, controls, focuses, and reports the external game viewport.
```

### 5.7 Memory and buffer ownership

Decision:

```text
Small Bridge control messages may be copied.
Large payloads must have explicit ownership, limits, and future attachment strategy.
Engine live memory must never be sent directly over the transport.
```

Rationale:

```text
- Editor connection messages are mostly small control messages.
- Avoid over-optimizing before the protocol is useful.
- Still avoid APIs that make later optimization impossible.
- Engine adapters must convert engine state into snapshots/DTOs before serialization.
```

---

## 6. Bridge Alpha Protocol Surface

The alpha protocol is intentionally narrow.

Required methods:

```text
bridge.hello
bridge.getCapabilities
engine.getStatus
engine.launchInfo
runtime.play
runtime.pause
runtime.stop
runtime.focusViewport
log.subscribe
log.unsubscribe
```

Optional but highly valuable before public alpha:

```text
scene.getTree
object.getSnapshot
object.setProperty
schema.getSnapshot
```

Required events:

```text
bridge.connected
bridge.disconnected
engine.statusChanged
engine.processExited
runtime.stateChanged
viewport.stateChanged
log.message
error.reported
```

Core message envelope fields:

```text
bridge        protocol marker, e.g. "norves.editor.bridge"
version       protocol version string
kind          request | response | event
id            request/response correlation id for requests
method        request method name
event         event name
params        method/event payload
result        response success payload
error         response error object
sessionId     optional session id after handshake
seq           optional monotonically increasing message sequence
```

Example request:

```json
{
  "bridge": "norves.editor.bridge",
  "version": "0.1",
  "kind": "request",
  "id": "req-42",
  "method": "runtime.play",
  "params": {}
}
```

Example event:

```json
{
  "bridge": "norves.editor.bridge",
  "version": "0.1",
  "kind": "event",
  "event": "log.message",
  "params": {
    "level": "info",
    "category": "Engine",
    "message": "Game started"
  }
}
```

---

## 7. Workstreams

Workstreams are planning units, not automatic implementation units. Each workstream must be decomposed into smaller phases before implementation. A phase should be independently reviewable, testable, and committable.

### A. Repository foundation

Purpose:

```text
Create the NorvesEditor repository skeleton, shared conventions, and build/test entry points.
```

Deliverables:

```text
- AGENTS.md and CLAUDE.md with identical content.
- README.md.
- docs/vision.md and docs/alpha-scope.md.
- Root package manager decision, likely pnpm.
- Rust workspace for Tauri backend and bridge crates.
- Initial app shell directory.
- Initial bridge directory.
- CI placeholder or local verification script.
```

Suggested phases:

```text
A1. Repository skeleton and documentation.
A2. Toolchain bootstrap for TypeScript/Rust/C++.
A3. Verification script that runs format/lint/test placeholders.
```

Done criteria:

```text
- Repository can be cloned and basic verification commands can run.
- AGENTS.md and CLAUDE.md match exactly.
- No NorvesLib-specific coding rules are accidentally copied into generic editor code.
```

### B. Product architecture and ADRs

Purpose:

```text
Record the product boundary and irreversible early decisions before implementation starts.
```

Deliverables:

```text
- docs/architecture.md.
- docs/viewport-strategy.md.
- docs/engine-integration.md.
- docs/memory-buffer-policy.md.
- ADRs for editor-owned bridge, Tauri backend, WebSocket/JSON, C++ engine SDK, external viewport.
```

Done criteria:

```text
- Codex can infer where UI, backend, bridge, C++ SDK, mock engine, and adapters belong.
- Alpha non-goals are explicit.
```

### C. Protocol specification and fixtures

Purpose:

```text
Define the wire contract before writing transports.
```

Deliverables:

```text
- bridge/spec/schema/*.schema.json.
- bridge/spec/fixtures/*.json.
- docs/protocol-overview.md.
- docs/message-envelope.md.
- docs/capabilities.md.
- docs/error-model.md.
```

Suggested phases:

```text
C1. Message envelope schema and base fixtures.
C2. Hello/capabilities/status/log schemas.
C3. Runtime control schemas.
C4. Optional scene/object/schema schemas.
C5. Compatibility and versioning rules.
```

Validation:

```text
- JSON Schema validation for every fixture.
- Negative fixtures for invalid messages.
```

Done criteria:

```text
- Rust, TypeScript, and C++ implementers can work from the same fixtures.
```

### D. Rust bridge core and editor client runtime

Purpose:

```text
Implement the editor-side Bridge runtime that Tauri backend will use.
```

Deliverables:

```text
- bridge/crates/norves-bridge-core.
- bridge/crates/norves-bridge-editor-client.
- Message types, status/error model, JSON codec, correlation, event stream.
- Loopback transport.
- Unit tests using fixtures.
```

Suggested phases:

```text
D1. Core message/status/value types.
D2. JSON codec and fixture round-trip.
D3. Request/response correlation and event dispatcher.
D4. Session state machine.
D5. Loopback transport for tests.
```

Done criteria:

```text
- cargo fmt, clippy, and tests pass for bridge crates.
- Runtime can talk to a loopback mock endpoint without Tauri.
```

### E. TypeScript UI-facing bridge package

Purpose:

```text
Give the UI typed access to backend commands/events without exposing transport internals.
```

Deliverables:

```text
- bridge/ts/packages/bridge-types.
- bridge/ts/packages/bridge-ui.
- Typed wrappers for Tauri commands.
- Event subscription helpers.
- Type tests or unit tests.
```

Suggested phases:

```text
E1. Shared TypeScript DTOs matching fixtures.
E2. Tauri command wrapper API.
E3. Event subscription wrapper API.
E4. UI state model for connection/process/runtime state.
```

Done criteria:

```text
- UI code never calls raw Bridge transport directly for alpha.
- TypeScript fixtures match protocol fixtures.
```

### F. C++ engine-side SDK

Purpose:

```text
Provide a standalone C++ SDK that any engine can embed to expose a NorvesEditor-compatible endpoint.
```

Deliverables:

```text
- bridge/cpp/engine-sdk/include.
- bridge/cpp/engine-sdk/src.
- CMakeLists.txt.
- BridgeEngineServer interface.
- IBridgeEngineAdapter interface.
- JSON codec.
- Buffer/ownership policy types.
- Unit tests or executable test harness.
```

Suggested phases:

```text
F1. Public API skeleton and CMake target.
F2. DTOs and JSON codec with fixtures.
F3. Engine adapter interface and dispatcher.
F4. Memory/buffer primitives and queue limits.
F5. Loopback tests.
```

Done criteria:

```text
- C++ SDK builds without NorvesLib.
- Public headers expose no third-party WebSocket or NorvesLib types.
- Fixture conformance passes.
```

### G. WebSocket transport

Purpose:

```text
Connect Rust editor client runtime to C++ engine SDK over a real local WebSocket connection.
```

Deliverables:

```text
- Rust WebSocket client transport.
- C++ WebSocket server transport.
- Transport-level reconnect/close/error handling.
- Bounded send/receive queues.
- Integration tests with mock engine.
```

Suggested phases:

```text
G1. Select C++ WebSocket library via ADR.
G2. Rust WebSocket client transport.
G3. C++ WebSocket server transport.
G4. End-to-end hello/capabilities/log/runtime control test.
G5. Disconnect/reconnect and error cases.
```

Done criteria:

```text
- Rust client can connect to C++ mock engine.
- Request/response/event messages work over WebSocket + JSON.
- Connection lifecycle is observable by tests and tools.
```

### H. Mock engine and conformance tools

Purpose:

```text
Make Bridge development possible before NorvesLib integration and before the full editor UI exists.
```

Deliverables:

```text
- bridge/cpp/examples/mock-engine.
- bridge/crates/norves-bridge-tools.
- Conformance runner.
- Protocol dump tool.
- Scripted mock responses and log events.
```

Suggested phases:

```text
H1. C++ mock engine using SDK loopback.
H2. C++ mock engine using WebSocket.
H3. Rust CLI connect/ping/log tool.
H4. Conformance runner against fixture scenarios.
```

Done criteria:

```text
- Mock engine is the default target for early NorvesEditor development.
- A failing protocol change is caught by fixture/conformance tests.
```

### I. Tauri app shell

Purpose:

```text
Create the minimal desktop shell where alpha UI will live.
```

Deliverables:

```text
- apps/editor Tauri app.
- Basic layout: top bar, side bar, panel area, status bar.
- Game View panel placeholder.
- Log panel.
- Connection panel.
- Settings/workspace panel.
```

Suggested phases:

```text
I1. Tauri/Vite/React skeleton.
I2. App layout and theme foundation.
I3. Command/event wiring to a fake backend state.
I4. Replace fake backend with Rust bridge client runtime.
```

Done criteria:

```text
- App opens and displays stable panels.
- Frontend/backend communication works through Tauri commands/events.
```

### J. Engine process lifecycle and external viewport control

Purpose:

```text
Allow NorvesEditor to launch, monitor, stop, and focus an external engine process.
```

Deliverables:

```text
- Engine profile settings.
- Launch argument builder.
- Process spawn/kill/status backend service.
- Stdout/stderr capture.
- Endpoint discovery strategy.
- Game View controls.
- External viewport focus/raise best effort implementation.
```

Suggested phases:

```text
J1. Engine profile schema and settings UI.
J2. Backend process service using scoped Tauri shell/sidecar/process facilities.
J3. Process status events to frontend.
J4. Endpoint discovery: explicit port first; stdout discovery optional.
J5. Focus/raise window best-effort path, with platform limitations documented.
```

Done criteria:

```text
- Editor can launch mock engine and show process state.
- Editor can stop mock engine cleanly.
- Editor can connect after launch.
```

### K. Game View alpha panel

Purpose:

```text
Deliver the alpha’s motivating surface: a viewport-like panel from which the game can be launched and controlled.
```

Deliverables:

```text
- Game View panel UI.
- Launch/Stop/Reconnect controls.
- Runtime Play/Pause/Stop controls.
- Runtime state display.
- External viewport status display.
- Recent log/status overlay or side area.
```

Suggested phases:

```text
K1. Static Game View UI.
K2. Bind process state.
K3. Bind Bridge connection state.
K4. Bind runtime state and commands.
K5. Error/retry UX.
```

Done criteria:

```text
- User can launch mock engine from Game View.
- User can connect and send runtime commands from Game View.
- User gets useful errors when launch/connect fails.
```

### L. NorvesLib reference adapter

Purpose:

```text
Connect the alpha to a real reference engine when practical.
```

Deliverables:

```text
- NorvesLib-side adapter plan.
- Adapter implementation in NorvesLib or a clearly isolated integration directory.
- Mapping from NorvesLib runtime/log/status to Bridge DTOs.
- Launch profile for NorvesLib Game executable.
```

Suggested phases:

```text
L1. Read NorvesLib current develop branch and identify stable adapter points.
L2. Define NorvesLib adapter boundary.
L3. Implement hello/capabilities/status/log/runtime control.
L4. Add launch profile and connection docs.
L5. Optional: schema/scene/object snapshot mapping.
```

Done criteria:

```text
- NorvesEditor can launch or attach to NorvesLib Game.
- Editor receives logs/status from NorvesLib.
- Runtime commands have meaningful behavior or explicit no-op responses.
```

### M. Documentation and developer experience

Purpose:

```text
Make the alpha buildable and understandable by future agents and humans.
```

Deliverables:

```text
- README alpha quick start.
- docs/build.md.
- docs/engine-profile.md.
- docs/protocol-debugging.md.
- docs/norveslib-integration.md.
- Troubleshooting guide.
```

Done criteria:

```text
- A fresh checkout can follow documented steps to launch editor + mock engine.
- Known limitations are documented.
```

### N. Post-alpha foundations

Purpose:

```text
Capture important future work without letting it pollute alpha scope.
```

Backlog topics:

```text
- Native viewport embedding research.
- Screenshot/thumbnail streaming.
- Scene tree and inspector expansion.
- Undo/redo transaction protocol.
- Asset browser and file watching.
- Binary codec / Protobuf.
- Remote secure connections.
- Plugin system.
- Packaging NorvesEditor with optional sidecar mock engine.
```

---

## 8. Implementation Order Recommendation

Do not start with the full UI. Do not start with NorvesLib. Start with a verifiable connection skeleton.

Recommended order:

```text
1. Repository foundation and AGENTS/CLAUDE.
2. Product architecture and ADRs.
3. Protocol spec, JSON Schema, and fixtures.
4. Rust bridge core/client with loopback tests.
5. C++ engine SDK with fixture tests.
6. C++ mock engine.
7. WebSocket transport between Rust client and C++ mock engine.
8. Tauri app shell.
9. Process lifecycle service.
10. Game View alpha panel.
11. NorvesLib reference adapter.
12. Alpha documentation and packaging notes.
```

This sequence lets NorvesEditor UI development begin only after the connection contract is proven, but still keeps all work inside the NorvesEditor repository.

---

## 9. Risks and Mitigations

### Native viewport embedding risk

Risk:

```text
Embedding an engine-owned Vulkan/DirectX window inside Tauri may require platform-specific window parenting, GPU interop, or frame streaming.
```

Mitigation:

```text
Use external engine viewport for alpha. Treat native embedding as post-alpha research.
```

### Mixed-language repo complexity

Risk:

```text
Rust, TypeScript, and C++ build systems can drift.
```

Mitigation:

```text
Add root verification scripts early. Keep each layer’s public contract fixture-driven.
```

### Protocol creep

Risk:

```text
The bridge layer expands into a full editor protocol before the alpha is motivating.
```

Mitigation:

```text
Restrict alpha to launch/connect/status/log/runtime control. Scene/object editing is optional for alpha.
```

### Engine coupling

Risk:

```text
NorvesLib adapter details leak into the generic C++ engine SDK.
```

Mitigation:

```text
Keep NorvesLib-specific code outside generic SDK. Generic SDK must build alone.
```

### Process permissions and security

Risk:

```text
Tauri shell/process permissions can become too broad.
```

Mitigation:

```text
All launch paths must be scoped, reviewed, and documented. Do not allow arbitrary command execution from UI without backend validation.
```

---

## 10. Acceptance Test Scenarios

Minimum manual alpha scenario:

```text
1. Build mock engine.
2. Start NorvesEditor.
3. Configure mock engine executable.
4. Press Launch in Game View.
5. Observe process status becomes running.
6. Observe Bridge state becomes connected.
7. Observe hello/capabilities in Connection panel.
8. Observe log messages in Log panel.
9. Press Play.
10. Observe runtime state changes to playing.
11. Press Pause.
12. Observe runtime state changes to paused.
13. Press Stop Process.
14. Observe process exited and Bridge disconnected.
```

NorvesLib alpha scenario:

```text
1. Build NorvesLib Game with editor bridge adapter enabled.
2. Start NorvesEditor.
3. Select NorvesLib Game launch profile.
4. Press Launch.
5. Engine window appears as external viewport.
6. Editor connects to NorvesLib Bridge endpoint.
7. Logs/status flow into Editor.
8. Runtime controls produce visible or logged engine behavior.
```

---

## 11. Source References for Technology Decisions

These references justify the high-level choices. Implementation work must re-check current official docs before pinning exact versions.

```text
Tauri v2 documentation: https://v2.tauri.app/start/
Tauri shell / sidecar documentation: https://v2.tauri.app/develop/sidecar/
Tauri calling Rust from frontend: https://v2.tauri.app/develop/calling-rust/
Tauri calling frontend from Rust: https://v2.tauri.app/develop/calling-frontend/
Rust official site: https://www.rust-lang.org/
Tokio docs: https://docs.rs/tokio/latest/tokio/
WebSocket RFC 6455: https://www.rfc-editor.org/rfc/rfc6455.html
JSON Schema overview: https://json-schema.org/overview/what-is-jsonschema
JSON-RPC 2.0 specification: https://www.jsonrpc.org/specification
IXWebSocket repository: https://github.com/machinezone/IXWebSocket
libwebsockets overview: https://libwebsockets.org/
```
