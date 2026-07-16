# Asset Manifest Runtime Reload Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a capability-gated `asset.reloadManifest` path from NorvesEditor through the generic Bridge to one atomic NorvesLib texture/model runtime snapshot switch.

**Architecture:** Gate A adds the additive protocol 0.2 method, strict generic SDK/client handling, authoritative same-session capabilities, and the editor action. After that commit is pushed, Gate B pins that checkout, builds one immutable `AssetSystem` candidate on GameThread, and installs it under texture→model locking before replying. Existing handles are retained; new requests use the new generation.

**Tech Stack:** JSON Schema/draft 2020-12, C++23/CMake/CTest, Rust/Tokio/Tauri, TypeScript/React/Vitest, NorvesLib C++23/Vulkan/fake RHI.

## Global Constraints

- This is HEAVY: protocol/public SDK/concurrency/asset lifetime require independent Codex and Claude reviews.
- Implement Gate A, verify/commit/push it, record its remote hash, then begin Gate B. Do not mix repository commits.
- Wire names are exactly method `asset.reloadManifest`, capability `asset.reload`, command `asset_reload_manifest`, protocol `0.2`.
- Params must be present and exactly `{}`; missing, `null`, scalar/array, or non-empty object returns an id-bearing `ENGINE_INVALID_PARAMS` before adapter dispatch.
- Result is exactly `{ "accepted": boolean }`; there is no event, path, manifest body, retry, or existing-handle rebind.
- Append the optional SDK virtual at the end of the existing virtual list; never reorder existing virtuals.
- New C++ files use UTF-8 BOM + CRLF. Preserve every existing file's EOL. NorvesLib uses custom containers/pointers/log/stats; `std` is allowed only in `Game/Bridge` SDK-boundary code and existing file-I/O boundary code.
- No singleton, Manager, generic reload transaction, async manifest I/O, cooked-format change, or Phase 5 work.

---

## Gate A — NorvesEditor vertical slice

### Task 1: Freeze the protocol, extension error, and fixtures

**Files:**
- Create: `bridge/spec/schema/methods/asset.reloadManifest.params.schema.json`
- Create: `bridge/spec/schema/methods/asset.reloadManifest.result.schema.json`
- Create: `bridge/spec/fixtures/methods/asset.reloadManifest/positive/request-valid.json`
- Create: `bridge/spec/fixtures/methods/asset.reloadManifest/positive/response-valid.json`
- Create: `bridge/spec/fixtures/methods/asset.reloadManifest/negative/request-unknown-field.json`
- Create: `bridge/spec/fixtures/methods/asset.reloadManifest/negative/response-missing-accepted.json`
- Create: `bridge/spec/fixtures/methods/asset.reloadManifest/negative/response-accepted-not-boolean.json`
- Create: `bridge/spec/fixtures/methods/asset.reloadManifest/negative/response-unknown-field.json`
- Create: `bridge/spec/fixtures/envelope/positive/response-error-engine-invalid-params.json`
- Modify: `bridge/spec/docs/protocol-overview.md`, `message-payloads.md`, `capabilities.md`, `error-model.md`
- Modify: `docs/adr/0008-cpp23-and-norveslib-style-alignment.md`, `docs/adr/0009-cpp-bridge-namespace-pascalcase.md`
- Test: `bridge/crates/norves-bridge-core/tests/fixtures_roundtrip.rs`

**Interfaces:** schema titles use `asset.reloadManifest.params` and `.result`; the golden error has the original id, code `ENGINE_INVALID_PARAMS`, and `data.method="asset.reloadManifest"`.

- [ ] **Step 1: Add RED fixture expectations.** Extend both Rust and C++ fixed-count assertions from positive 73 to 76, keep envelope-negative 14, extend payload-negative 66 to 70, and change total 153 to 160. Assert the exact new positive/error paths before creating schemas/fixtures.
- [ ] **Step 2: Prove RED.** Run `python scripts/validate-bridge-fixtures.py` and `cargo test -p norves-bridge-core --test fixtures_roundtrip`; expect missing-schema/fixture failure and a count other than 160.
- [ ] **Step 3: Add strict schemas/fixtures and prose.** Params are `{type:"object",properties:{},required:[],additionalProperties:false}`; result requires only boolean `accepted`. Register `ENGINE_INVALID_PARAMS` as an engine extension, add its correspondence row and golden fixture in the same diff. Add `asset.reload` and the method without changing protocol 0.2.
- [ ] **Step 4: Prove GREEN and manually guard the non-enforced mapping.** Run `python scripts/validate-bridge-fixtures.py`; expect `OK: 160 fixture(s) validated.` Then run `rg -n "ENGINE_INVALID_PARAMS|response-error-engine-invalid-params" bridge/spec/docs/error-model.md bridge/spec/fixtures/envelope/positive/response-error-engine-invalid-params.json`; expect registry row, correspondence row, and fixture. The validator does not enforce that table.
- [ ] **Step 5: Verify filenames.** Run `rg --files bridge/spec | rg "asset\.reloadManifest\.(request|response)\.schema"`; expect no output. Only `.params.schema.json` and `.result.schema.json` are valid.

### Task 2: Add strict C++ SDK dispatch without breaking adapters

**Files:**
- Modify: `bridge/cpp/engine-sdk/include/Norves/Bridge/adapter.hpp`
- Modify: `bridge/cpp/engine-sdk/include/Norves/Bridge/error.hpp`
- Modify: `bridge/cpp/engine-sdk/src/server.cpp`
- Test: `bridge/cpp/engine-sdk/tests/dispatch_test.cpp`
- Test: `bridge/cpp/engine-sdk/tests/fixtures_roundtrip_test.cpp`

**Interfaces:**
```cpp
inline constexpr std::string_view ErrorEngineInvalidParams = "ENGINE_INVALID_PARAMS";
virtual Result<JsonValue, BridgeError> assetReloadManifest(const JsonValue& params)
{
    return not_supported(params);
}
```

- [ ] **Step 1: Write RED dispatch tests.** Add an override counter/result test for a present `{}`, a default-adapter `METHOD_NOT_SUPPORTED` test, and table cases for omitted/null/string/array/non-empty-object params. Every invalid case must keep the request id, return exact `ENGINE_INVALID_PARAMS`, and leave the override count zero.
- [ ] **Step 2: Prove RED.** Configure if needed with `cmake -S bridge/cpp -B build/cpp -G "Visual Studio 17 2022"`; build `cmake --build build/cpp --config Debug --target dispatch_test fixtures_roundtrip_test` and run `ctest --test-dir build/cpp -C Debug -R "dispatch_test|fixtures_roundtrip_test" --output-on-failure`; expect unknown-method/default errors because the branch/virtual is absent.
- [ ] **Step 3: Implement the minimal additive API.** Append the non-pure virtual after `viewportGetThumbnail`; add the open-registry constant. In `server.cpp`, branch on `request.params.has_value()` before assigning the existing `emptyParams` fallback, then inspect the original optional `JsonValue` through `peek(*request.params)->json`. Omitted, explicit null, non-object, or non-empty object is invalid; only a present empty object dispatches. Construct `BridgeError{std::string(ErrorEngineInvalidParams), "asset.reloadManifest params must be an empty object.", Wrap(json{{"method", "asset.reloadManifest"}})}` through the existing id-bearing path. The fallback reference alone cannot distinguish omitted params from explicit null and must not be used for this validation.
- [ ] **Step 4: Prove GREEN and declaration order.** Rebuild `dispatch_test fixtures_roundtrip_test` and run their exact CTest names; expect all cases pass. Run `rg -n "assetGetManifest|viewportGetThumbnail|assetReloadManifest|protected:" bridge/cpp/engine-sdk/include/Norves/Bridge/adapter.hpp`; verify all old declarations retain order and reload is last.

### Task 3: Add strict Rust result and authoritative capability discovery

**Files:**
- Create: `bridge/crates/norves-bridge-editor-client/src/capabilities.rs`
- Modify: `bridge/crates/norves-bridge-editor-client/src/asset.rs`
- Modify: `bridge/crates/norves-bridge-editor-client/src/lib.rs`
- Test: `bridge/crates/norves-bridge-editor-client/tests/loopback_roundtrip.rs`
- Modify: `apps/editor/src-tauri/src/bridge_state.rs`, `dto.rs`, `protocol_names.rs`, `lib.rs`
- Test: `apps/editor/src-tauri/tests/process_e2e.rs`

**Interfaces:**
```rust
pub struct AssetReloadManifestResult { pub accepted: bool }
pub fn parse_asset_reload_manifest_result(value: &Value)
    -> Result<AssetReloadManifestResult, AssetError>;
pub struct CapabilitiesResult { pub capabilities: Vec<CapabilityDescriptor> }
pub fn parse_capabilities_result(value: &Value)
    -> Result<CapabilitiesResult, CapabilityError>;
```
`ConnectionStatePayload` and private `LiveConnection` gain `capabilities: Vec<CapabilityDescriptor>`; `ConnectionStatePayload::connected(session_id: String, server_name: String, endpoint: String, capabilities: Vec<CapabilityDescriptor>)` requires that vector.

- [ ] **Step 1: Write RED parser/connection tests.** Accept `accepted:true/false`; reject missing/non-boolean/extra fields. Strict capabilities accept the fixture shape and reject missing/non-array/invalid token/version/descriptor extra fields/result extra fields. Loopback must observe `bridge.hello` followed on the same handle by `bridge.getCapabilities` with `{}` before ready; malformed/error response tears down.
- [ ] **Step 2: Prove RED.** Run `cargo test -p norves-bridge-editor-client asset_reload`; expect missing reload symbols. Separately run `cargo test -p norves-bridge-editor-client capabilities`; expect missing capability parser symbols. Run `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml connection_capabilities`; expect missing DTO fields.
- [ ] **Step 3: Implement parsers and same-session hard gate.** Reuse `norves_bridge_core::CapabilityDescriptor` (no duplicate descriptor/dependency) plus an outer `deny_unknown_fields` DTO. In `run_connect_flow`, complete hello, send `bridge.getCapabilities` on the same `DispatchHandle`, strictly parse it, and only then construct `LiveConnection`; all failures call `tear_down_partial`.
- [ ] **Step 4: Add the Tauri command.** Mirror the existing live `asset_get_manifest` command path, not offline `asset_read_manifest`: define `protocol_names::commands::ASSET_RELOAD_MANIFEST="asset_reload_manifest"`; register `asset_reload_manifest`; send method `asset.reloadManifest` with `Some(Map::new())`, validate with the parser, and forward the original wire `Value`.
- [ ] **Step 5: Prove existing engines are strict before relying on the hard gate.** Extend loopback and process E2E assertions so the existing mock and NorvesLib `bridge.getCapabilities` values parse strictly. Run the mock process test with `$env:NORVES_ENGINE_PATH=(Resolve-Path build/cpp/examples/mock-engine/Debug/norves_mock_engine.exe); cargo test --manifest-path apps/editor/src-tauri/Cargo.toml --test process_e2e engine_capabilities_contract -- --nocapture`; expect `[PASS]`, not `[SKIP]`.

### Task 4: Add TypeScript IPC names and generation-bound store state

**Files:**
- Modify: `bridge/ts/packages/bridge-types/src/methods.ts`, `index.ts`
- Modify: `bridge/ts/packages/bridge-ui/src/ipc-types.ts`, `commands.ts`, `index.ts`
- Test: `bridge/ts/packages/bridge-ui/src/__tests__/names.test.ts`, `wrappers.test.ts`
- Modify: `apps/editor/src/state/store.ts`
- Test: `apps/editor/src/state/__tests__/store.test.ts`

**Interfaces:**
```ts
export interface AssetReloadManifestResult { accepted: boolean }
export async function assetReloadManifest(): Promise<AssetReloadManifestResult>;
```
`ConnectionStatePayload.capabilities?: CapabilityDescriptor[]`; store connection adds `capabilityNames?: ReadonlySet<string>`. Add separate `assetReloadError?: BackendError` and `assetReloadUnsupported: boolean`; update the `assetError` comment to say it is offline manifest read/parse only, and never reuse/clear it for runtime reload.

- [ ] **Step 1: Write RED name/wrapper/reducer tests.** Assert exact command string, zero-argument invoke (`{}` or no Tauri args), typed forwarding, capability set creation, same-session informational payload without capabilities preserving the set, changed session/disconnect clearing it, and reload success/failure/unsupported actions leaving `assetError` byte-equivalent.
- [ ] **Step 2: Prove RED.** Run `pnpm exec vitest run bridge/ts/packages/bridge-ui/src/__tests__/names.test.ts bridge/ts/packages/bridge-ui/src/__tests__/wrappers.test.ts apps/editor/src/state/__tests__/store.test.ts`; expect missing names/actions.
- [ ] **Step 3: Implement types/state.** Derive the authoritative token set only from the Rust connection payload. Frontend generation is `sessionId`; Rust's internal numeric generation is not exposed. A `bridge.connected` informational event that lacks capabilities cannot clear the same-session authoritative set. Reset unsupported/error verdicts on session change/disconnect.
- [ ] **Step 4: Prove GREEN.** Rerun the three Vitest files and `pnpm -r --if-present typecheck`; expect all pass.

### Task 5: Add the capability-gated hook and Asset Browser control

**Files:**
- Modify: `apps/editor/src/hooks/useBridge.ts`
- Test: `apps/editor/src/hooks/__tests__/useBridge.test.ts`, `useBridge.lifecycle.test.tsx`
- Modify: `apps/editor/src/components/AssetBrowserPanel.tsx`
- Test: `apps/editor/src/components/__tests__/AssetBrowserPanel.test.tsx`

**Interfaces:** `BridgeActions.reloadAssetRuntime(): Promise<void>` calls `assetReloadManifest()` only when connected, current session is unchanged, `asset.reload` is present, and that session has not degraded to unsupported.

- [ ] **Step 1: Write RED lifecycle/UI tests.** Cover disabled accessible button when disconnected/capability-absent, one call on click, accepted true clearing only `assetReloadError`, false showing a runtime reload banner while preserving an offline parse banner, `METHOD_NOT_SUPPORTED` disabling only this connection, other errors in reload banner, late old-session result ignored, reconnect enabling from fresh capabilities, and no offline file read after success.
- [ ] **Step 2: Prove RED.** Run `pnpm exec vitest run apps/editor/src/hooks/__tests__/useBridge.test.ts apps/editor/src/hooks/__tests__/useBridge.lifecycle.test.tsx apps/editor/src/components/__tests__/AssetBrowserPanel.test.tsx`; expect missing action/button.
- [ ] **Step 3: Implement minimal UI behavior.** Place `Reload Runtime` beside Load/Clear with `aria-label="Reload runtime asset manifest"`; no path input is sent. Keep offline and runtime error banners independently dismissible. `METHOD_NOT_SUPPORTED` is a connection-local verdict and never changes connection status.
- [ ] **Step 4: Prove GREEN.** Rerun the focused Vitest files; expect all pass and no calls to `assetReadManifest` from the reload action.

### Gate A verification, review, commit, push

- [ ] Run `python scripts/validate-bridge-fixtures.py` (exactly 160), C++ configure/build/`ctest --test-dir build/cpp -C Debug --output-on-failure`, root Rust fmt/clippy/test, src-tauri fmt/clippy/test, the non-skipped mock process E2E, focused Vitest, and `pnpm -r --if-present typecheck`.
- [ ] Run `./scripts/verify.ps1 -Cpp`; C++ must say build+CTest `[OK]`, never `build/cpp does not exist`.
- [ ] Compare `git diff --numstat` with `git diff --ignore-cr-at-eol --numstat`; verify new C++ BOM/CRLF and `git diff --no-index -- AGENTS.md CLAUDE.md` is empty.
- [ ] Complete one integrated scope check, independent Codex+Claude reviews (max two rounds), then commit one Japanese imperative protocol/UI vertical-slice commit and push `feature/asset-manifest-reload`. Record the pushed hash with `git rev-parse HEAD` and `git ls-remote origin refs/heads/feature/asset-manifest-reload`; hashes must match.

---

## Gate B — NorvesLib runtime and real Bridge integration

### Task 6: Add atomic RenderResources snapshot replacement

**Files:**
- Modify: `Library/Core/Public/Rendering/RenderResources.h`
- Modify: `Library/Core/Private/Rendering/RenderResources.cpp`
- Modify: `Library/Core/Private/Rendering/TextureAssetRuntime.h`, `TextureAssetRuntime.cpp`
- Modify: `Library/Core/Private/Rendering/TextureAssetResolver.h`, `TextureAssetResolver.cpp`
- Modify: `Library/Core/Private/Resource/ModelAssetRuntime.h`, `ModelAssetRuntime.cpp`
- Create/Test: `Test/Core/Rendering/AssetRuntimeSnapshotReloadTest.cpp`
- Create/Test: `Test/Core/Rendering/TextureAssetReloadAdmissionTest.cpp`
- Create/Test: `Test/Core/Rendering/AssetManifestReloadBridgeLoopbackTest.cpp`
- Modify: `Test/Core/Rendering/CMakeLists.txt`

**Interfaces:**
```cpp
bool RenderResources::ReloadAssetRuntimeSnapshot(
    const Container::String& assetRoot,
    Container::TSharedPtr<const Asset::AssetSystem> candidate);
```
Private friend-only helpers are `TextureAssetRuntime::{CanReloadSnapshotLocked,ApplyReloadSnapshotLocked}`, `TextureAssetResolver::ReplaceSnapshot`, and `ModelAssetRuntime::{CanReloadSnapshotLocked,ApplyReloadSnapshotLocked,ReleaseRetiredAfterReload}`. Apply returns retired model handles for release only after both locks are dropped.

- [ ] **Step 1: Write RED fake-RHI tests.** Install A, retain a texture handle and model lease, switch to B, assert the same candidate address and candidate root are observed by both plans, and compare the candidate's parsed references with direct `AssetSystem(root)+LoadManifestFromJsonText` parsing. Assert each generation +1, B-only texture/model resolve, old texture valid via `GpuResourceStore` despite map clear, and old model lease valid while retired/unleased models release. Null/uninitialized/shutdown/texture pending+active/model pending+active reject with snapshots/generations/cache unchanged.
- [ ] **Step 2: Write RED deterministic admission tests.** A private friend test barrier pauses `LoadTextureAsync` after plan construction while it still owns `m_TextureAssetMutex`: reload blocks, admission enqueues A, then reload observes busy and rejects. Inverse ordering installs B before the waiting request constructs/enqueues B. Callback assertion proves cache-hit callback occurs after unlock.
- [ ] **Step 3: Prove RED.** Build/run `AssetRuntimeSnapshotReloadTest` and `TextureAssetReloadAdmissionTest`; expect missing public method/test seam.
- [ ] **Step 4: Implement the commit point.** `RenderResources` locks texture asset then model asset, checks initialized/not-shutting-down/bound/accepting/no pending-or-active-flush/non-null, then applies the same pointer and advances each generation exactly once. No reverse model→texture nesting/callback exists. Log one stable rejected reason or accepted generations.
- [ ] **Step 5: Close the texture admission window.** Hold `m_TextureAssetMutex` across bound/queue setup, plan, cache lookup, duplicate append, request/task creation, and enqueue/submit. Copy cache-hit handle/callback while locked and invoke after unlock, mirroring `ModelAssetRuntime::LoadModelAsync`. Preserve asset→queue→cache order.
- [ ] **Step 6: Preserve stale-worker cleanup.** Keep both texture/model pre-GPU and pre-publish generation checks; post-upload stale texture releases its new handle, post-register stale model calls `ReleaseModelUnmanaged`.
- [ ] **Step 7: Prove GREEN and deadlock refutation.** Run `ctest --test-dir build -C Debug -R "AssetRuntimeSnapshotReloadTest|TextureAssetReloadAdmissionTest" --repeat until-fail:20 --output-on-failure`. Run `rg -n "m_AssetMutex|m_TextureAssetMutex" Library/Core/Private/Rendering/{RenderResources,TextureAssetRuntime}.cpp Library/Core/Private/Resource/ModelAssetRuntime.cpp` and manually record that texture→model is the sole dual-lock path and no model-held callback calls texture code.

### Task 7: Build once on GameThread and expose the exact current snapshot

**Files:**
- Modify: `Game/GameApplicationHandler.h`, `GameApplicationHandler.cpp`
- Modify: `Game/Bridge/NorvesLibBridgeAdapter.h`, `NorvesLibBridgeAdapter.cpp`

**Interfaces:**
```cpp
bool GameApplicationHandler::ReloadConfiguredAssetManifest();
Container::TSharedPtr<const Asset::AssetSystem> GetAssetSystemSnapshot() const;
Result<JsonValue, BridgeError> NorvesLibBridgeAdapter::assetReloadManifest(
    const JsonValue& params) override;
```

- [ ] **Step 1: Add RED integration assertions to Task 8 before implementation.** Expect `asset.reload` advertised and reload dispatch accepted only with valid configured files.
- [ ] **Step 2: Build the candidate before mutation.** On GameThread validate configured root/file, read synchronously, construct one `MakeShared<AssetSystem>(root)`, parse fully, cast/store as `TSharedPtr<const AssetSystem>`, then call the unified entry. Invalid/read/parse/busy returns false without replacing `m_AssetSystemSnapshot`.
- [ ] **Step 3: Unify startup.** `OnPostInitialize` calls `ReloadConfiguredAssetManifest`; startup failure still requests exit. Remove the old separate texture root/manifest mutation. On success retain the exact candidate for Bridge read methods. Add a startup baseline case proving initial configuration installs both texture and model snapshots before any reload.
- [ ] **Step 4: Wire the adapter.** Append the override, return only `{accepted:boolean}`, and add `asset.reload` to strict capabilities. Change `asset.resolve`/`asset.getManifest` to borrow the retained immutable snapshot rather than reparsing disk, so post-command wire reads prove the committed runtime generation. Keep `std` confined to this SDK/file boundary.

### Task 8: Prove real SDK dispatch and A→B behavior

**Files:**
- Modify/Test: `apps/editor/src-tauri/tests/process_e2e.rs` only in Gate A (the test lands there before the SDK hash is frozen; Gate B merely runs it)
- Create/Test: `Test/Core/Rendering/AssetManifestReloadBridgeLoopbackTest.cpp`
- Create/Test: `Test/Core/Rendering/AssetManifestReloadBridgeLoopback.cmake.in`
- Modify: `Test/Core/Rendering/CMakeLists.txt`
- Modify: `Game/CMakeLists.txt` only if a reusable Game-source list is required by the test target; do not add a revision field.

**Interfaces:** the process test accepts `NORVES_NORVESLIB_ENGINE_PATH`, `NORVES_ASSET_RELOAD_ROOT`, `NORVES_ASSET_RELOAD_LIVE_MANIFEST`, and `NORVES_ASSET_RELOAD_MANIFEST_B`; its spawn helper appends existing `--texture-asset-root`/`--texture-asset-manifest` args.

- [ ] **Step 1: Configure against the pushed SDK.** Set `$NorvesEditorRoot='C:\Users\KINGkawamura\Documents\NorvesEditor'`; record `git -C $NorvesEditorRoot rev-parse HEAD` and require it equals `git -C $NorvesEditorRoot ls-remote origin refs/heads/feature/asset-manifest-reload`. Configure with `cmake -S . -B build -G "Visual Studio 17 2022" -DNORVES_BRIDGE_SDK_DIR="$NorvesEditorRoot/bridge/cpp"`.
- [ ] **Step 2: Generate A/B fixtures.** The CMake test script uses the existing `AssetCook` target/fixture generators to produce A-only and B-only cooked texture+model references, copies A to the live manifest, then invokes the Rust process test with all four env vars.
- [ ] **Step 3: Add the in-process real-runtime loopback target.** Under `if(NORVES_BRIDGE_SDK_DIR)`, `AssetManifestReloadBridgeLoopbackTest` compiles its test plus `Game/GameApplicationHandler.cpp`, `Game/Bridge/{NorvesLibBridgeAdapter,BridgeServerHost,BridgeLogSink,ReadyHandshake}.cpp`, `Game/GameModes/MemoryAgingTest/MemoryAgingTestRoutine.cpp`, and `Game/GameModes/Rendering3DTest/{DirectionalLightEditSubRoutine,Rendering3DTestDebugDraw,Rendering3DTestDebugInput,Rendering3DTestRoutine}.cpp`; link `Core`, `NorvesModule_Dummy`, and `norves_bridge_engine_sdk`, define `NORVES_BRIDGE_ENABLED`, include repository/Game/Core-private paths, depend on `AssetCook`, and register one CTest. Do not substitute a direct handler-only test.
- [ ] **Step 4: Prove real SDK dispatch and runtime generation B.** On one test GameThread, instantiate `Core::Engine`, save/set/restore global `GEngine` with RAII, initialize `RenderResources` with fake RHI, configure the actual `GameApplicationHandler` with root/live manifest A, and load/retain valid A texture and model-runtime lease. Replace the live file with B, send a real `BridgeEngineServer` frame to the actual `NorvesLibBridgeAdapter`, assert id-bearing `{accepted:true}`, then call public `Textures().LoadTexture` and `MegaGeometry().LoadModelAsync`+flush for B-only cooked assets. Assert valid B handles, old A texture remains in `GpuResourceStore`, old A model lease remains valid until explicit release, and both generations advanced. This test must not use `asset.resolve` as runtime proof.
- [ ] **Step 5: Retain complementary process/network proof.** The Rust process test launches real `Game`, performs hello then strict capabilities, asserts `asset.reload`, overwrites the configured manifest, sends `asset.reloadManifest {}`, receives `accepted:true`, and checks B-only wire metadata. It proves network/UI wiring, while the C++ loopback proves actual texture/model runtime loads. Neither sends a path/body.
- [ ] **Step 6: Prove rejection preservation.** Internal fake-RHI tests hold texture/model pending/active work and assert reload false plus A resolution. The process E2E additionally launches without valid config and expects `accepted:false`; this distinguishes adapter dispatch from `METHOD_NOT_SUPPORTED`.
- [ ] **Step 7: Build/run.** Build `Game AssetCook AssetRuntimeSnapshotReloadTest TextureAssetReloadAdmissionTest AssetManifestReloadBridgeLoopbackTest`; run the three focused CTests and `AssetManifestReloadBridgeLoopback` process script with output. The C++ loopback must report real SDK dispatch plus valid generation-B texture/model handles; the process test must report strict capabilities and no `[SKIP]`.

### Gate B verification, review, commit, push

- [ ] Build Bridge-enabled `Game` and all focused targets; run focused tests, then `ctest --test-dir build -C Debug --output-on-failure` for the full suite.
- [ ] Re-run the NorvesEditor process E2E directly with `NORVES_NORVESLIB_ENGINE_PATH` and fixture env vars; require `[PASS]` and no `[SKIP]`.
- [ ] Run EOL/BOM comparison and both repository mirror diffs. Confirm NorvesEditor is clean at the recorded SDK hash and NorvesLib changes match only the declared Gate B paths.
- [ ] Complete one integrated scope check, independent Codex+Claude implementation reviews (max two rounds), commit one Japanese imperative NorvesLib runtime/Bridge change with an asset-lifetime/threading body, push `codex/m4-mesh-cook`, and inspect remote branch/CI state.

## Review checklist and rollback

- [ ] Recheck Claude design points: strict existing capability responses; sole texture→model dual lock; callback after unlock; texture map/GpuResourceStore versus model retired-lease ownership; manual error-table mapping; sessionId frontend generation and informational-event preservation; exact schema filenames.
- [ ] Confirm offline `assetError` survives every runtime reload result; only `assetReloadError` changes.
- [ ] Confirm busy/invalid paths mutate neither runtime and successful apply publishes the identical candidate to both before unlock.
- [ ] Roll back in reverse order: revert/push NorvesLib Gate B first, then NorvesEditor Gate A. Additive old engines remain usable because capability omission/default virtual yields `METHOD_NOT_SUPPORTED`.

## Declared implementation scope for the orchestrator

Use these exact paths as the later five-line declarations/check-scope inputs. Build output is verification evidence and is never staged.

**Gate A write scope — NorvesEditor:**

```text
bridge/spec/schema/methods/asset.reloadManifest.params.schema.json
bridge/spec/schema/methods/asset.reloadManifest.result.schema.json
bridge/spec/fixtures/methods/asset.reloadManifest/**
bridge/spec/fixtures/envelope/positive/response-error-engine-invalid-params.json
bridge/spec/docs/protocol-overview.md
bridge/spec/docs/message-payloads.md
bridge/spec/docs/capabilities.md
bridge/spec/docs/error-model.md
docs/adr/0008-cpp23-and-norveslib-style-alignment.md
docs/adr/0009-cpp-bridge-namespace-pascalcase.md
bridge/cpp/engine-sdk/include/Norves/Bridge/adapter.hpp
bridge/cpp/engine-sdk/include/Norves/Bridge/error.hpp
bridge/cpp/engine-sdk/src/server.cpp
bridge/cpp/engine-sdk/tests/dispatch_test.cpp
bridge/cpp/engine-sdk/tests/fixtures_roundtrip_test.cpp
bridge/crates/norves-bridge-core/tests/fixtures_roundtrip.rs
bridge/crates/norves-bridge-editor-client/src/asset.rs
bridge/crates/norves-bridge-editor-client/src/capabilities.rs
bridge/crates/norves-bridge-editor-client/src/lib.rs
bridge/crates/norves-bridge-editor-client/tests/loopback_roundtrip.rs
apps/editor/src-tauri/src/bridge_state.rs
apps/editor/src-tauri/src/dto.rs
apps/editor/src-tauri/src/protocol_names.rs
apps/editor/src-tauri/src/lib.rs
apps/editor/src-tauri/tests/process_e2e.rs
bridge/ts/packages/bridge-types/src/methods.ts
bridge/ts/packages/bridge-types/src/index.ts
bridge/ts/packages/bridge-ui/src/ipc-types.ts
bridge/ts/packages/bridge-ui/src/commands.ts
bridge/ts/packages/bridge-ui/src/index.ts
bridge/ts/packages/bridge-ui/src/__tests__/names.test.ts
bridge/ts/packages/bridge-ui/src/__tests__/wrappers.test.ts
apps/editor/src/state/store.ts
apps/editor/src/state/__tests__/store.test.ts
apps/editor/src/hooks/useBridge.ts
apps/editor/src/hooks/__tests__/useBridge.test.ts
apps/editor/src/hooks/__tests__/useBridge.lifecycle.test.tsx
apps/editor/src/components/AssetBrowserPanel.tsx
apps/editor/src/components/__tests__/AssetBrowserPanel.test.tsx
```

**Gate B write scope — NorvesLib:**

```text
Library/Core/Public/Rendering/RenderResources.h
Library/Core/Private/Rendering/RenderResources.cpp
Library/Core/Private/Rendering/TextureAssetRuntime.h
Library/Core/Private/Rendering/TextureAssetRuntime.cpp
Library/Core/Private/Rendering/TextureAssetResolver.h
Library/Core/Private/Rendering/TextureAssetResolver.cpp
Library/Core/Private/Resource/ModelAssetRuntime.h
Library/Core/Private/Resource/ModelAssetRuntime.cpp
Game/GameApplicationHandler.h
Game/GameApplicationHandler.cpp
Game/Bridge/NorvesLibBridgeAdapter.h
Game/Bridge/NorvesLibBridgeAdapter.cpp
Game/CMakeLists.txt
Test/Core/Rendering/AssetRuntimeSnapshotReloadTest.cpp
Test/Core/Rendering/TextureAssetReloadAdmissionTest.cpp
Test/Core/Rendering/AssetManifestReloadBridgeLoopbackTest.cpp
Test/Core/Rendering/AssetManifestReloadBridgeLoopback.cmake.in
Test/Core/Rendering/CMakeLists.txt
```

**Gate B read-only cross-repository evidence:**

```text
C:\Users\KINGkawamura\Documents\NorvesEditor\apps\editor\src-tauri\tests\process_e2e.rs
C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\cpp\**
```

## Exact final evidence bundle

- [ ] NorvesEditor: `git status --short --branch`, `git rev-parse HEAD`, remote hash, fixture `160`, all aggregate gates, and mock process E2E `[PASS]`/no `[SKIP]`.
- [ ] NorvesLib: configured SDK hash, six focused build targets, three focused CTest contracts, full CTest count/pass, actual Game process E2E `[PASS]`/no `[SKIP]`, and remote hash.
- [ ] Both repos: byte-identical governance mirrors, EOL numstat parity, clean worktrees, branch pushed, and CI/workflow state explicitly reported (including absence of workflow files).
- [ ] Acceptance trace: wire invalid params never dispatch; unsupported is connection-local; invalid/busy is no-mutation; success changes both generations; old texture/model references survive; B-only texture/model resolve after real SDK dispatch.
