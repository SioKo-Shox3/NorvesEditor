# Asset manifest runtime reload design

**Status:** Approved design for NorvesLib M4 Phase 4
**Date:** 2026-07-16
**Scope:** NorvesEditor protocol/SDK/editor vertical slice plus the NorvesLib consumer. Phase 5 measurement is separate.

## Goal and invariant

The editor can ask a connected engine to reload the manifest path already configured at engine startup. A successful reload switches texture and model resolution to one newly parsed `TSharedPtr<const AssetSystem>` snapshot. Existing texture/model handles and leases remain valid; only requests created after the switch resolve against the new snapshot.

The operation is synchronous on NorvesLib's GameThread. Wire-invalid params return an id-bearing `ENGINE_INVALID_PARAMS` error before adapter dispatch. For a valid `{}` request, a configured-file/parse failure or any pending/active texture or model work returns `accepted:false` without changing either runtime's snapshot, cache, or generation.

## Options considered

### A. Complete cross-repository vertical slice — selected

Add the wire method and capability, generic C++ SDK dispatch, Rust/Tauri/TypeScript wrappers, minimal Asset Browser control, and the NorvesLib implementation together. This is the only option that provides an observable, capability-gated feature while keeping the generic Bridge independent of NorvesLib.

Trade-off: it touches two repositories and load-bearing protocol, C++ public API, ownership, and concurrency boundaries. It therefore requires the heavy workflow and ordered commits.

### B. Backend command only — rejected

Add the schema, SDK dispatch, and NorvesLib handler without editor UI. This reduces frontend work but leaves no supported user path and does not prove capability degradation or error presentation end to end.

### C. NorvesLib raw-frame interception or `asset.getManifest` side effect — rejected

Intercepting an unknown method before SDK dispatch, or making the read-only `asset.getManifest` mutate engine state, would bypass the generic protocol contract. It would also make old clients unpredictable and violate `asset.read` semantics. Neither technique is permitted.

## Wire contract

- Method: `asset.reloadManifest`
- Capability: `asset.reload`
- Params: an object with no properties and `additionalProperties:false`
- Result: `{ "accepted": boolean }` with `additionalProperties:false`
- Events: none
- Protocol version: remains 0.2; this is an additive optional method and capability

`accepted:true` means both runtime snapshots were switched before the response was produced. `accepted:false` means a syntactically valid request reached the adapter but runtime preconditions failed without changing state. Unsupported adapters return `METHOD_NOT_SUPPORTED`; invalid params return `ENGINE_INVALID_PARAMS`. These three outcomes are distinct.

The live SDK dispatch validates the method-specific params before calling the adapter. Only a present, non-null JSON object with zero properties is accepted. Missing params, `null`, any non-object value, or a non-empty object returns an id-bearing error response with `BridgeError{ErrorEngineInvalidParams, ...}` and never enters `assetReloadManifest`. `ENGINE_INVALID_PARAMS` is an engine-extension code in the open error registry, not a new core code.

Compatibility rules:

- An old editor never calls the method and continues using `asset.read` normally.
- An old engine omits `asset.reload`; the editor hides or disables the action and treats `METHOD_NOT_SUPPORTED` as graceful degradation.
- The new SDK virtual is appended after all existing virtual methods and has the existing non-pure `not_supported(params)` default. Existing adapter source remains compatible and no existing virtual is reordered. Consumers must rebuild against the changed C++ SDK; binary ABI compatibility is not promised.
- The editor calls only when the negotiated capability set contains `asset.reload`. The server still dispatches the method so a direct call to an adapter that uses the default yields `METHOD_NOT_SUPPORTED`.

## NorvesEditor vertical slice

### Protocol and generic C++ SDK

Add strict params/result schemas under `bridge/spec/schema/methods/asset.reloadManifest.*.schema.json`. Add positive request/response fixtures and negative fixtures for an unknown params property, missing/wrong-type `accepted`, and an unknown result property. Update the protocol overview, payload table, capability documentation, and frozen public-symbol ADR.

In the same NorvesEditor protocol commit, add `ENGINE_INVALID_PARAMS` to the engine-extension table in `bridge/spec/docs/error-model.md`, add its fixture-correspondence row, and add the mapped golden id-bearing error response fixture `bridge/spec/fixtures/envelope/positive/response-error-engine-invalid-params.json`. Its payload identifies `asset.reloadManifest` and proves the stable spelling; it does not promote the extension to the closed core set.

Append this method to `IBridgeEngineAdapter`:

```cpp
virtual Result<JsonValue, BridgeError> assetReloadManifest(const JsonValue& params)
{
    return not_supported(params);
}
```

Add `inline constexpr std::string_view ErrorEngineInvalidParams = "ENGINE_INVALID_PARAMS"` to SDK `error.hpp`. `BridgeServer` validates params on the `asset.reloadManifest` request branch before dispatch: missing, null, non-object, and non-empty values produce `BridgeError{std::string(ErrorEngineInvalidParams), ...}` through the normal id-bearing error-response construction, with no adapter call. Only `{}` dispatches to the override/default virtual. SDK tests cover successful override dispatch, default-adapter `METHOD_NOT_SUPPORTED`, and each invalid params class yielding exact `ENGINE_INVALID_PARAMS` with the original request id. Unknown methods retain `METHOD_NOT_SUPPORTED`.

### Rust/Tauri and TypeScript

The Rust editor client adds a sans-I/O `AssetReloadManifestResult` and `parse_asset_reload_manifest_result(const Value&)` equivalent following the existing asset result parsers. It accepts exactly a boolean `accepted` from the wire-shaped result. The Tauri command `asset_reload_manifest` sends `{}`, validates the result, and returns the original wire value; malformed results become the existing backend error type.

The TypeScript bridge packages add:

- `AssetReloadManifestResult { accepted: boolean }`;
- `BRIDGE_COMMANDS.assetReloadManifest = 'asset_reload_manifest'`;
- `assetReloadManifest(): Promise<AssetReloadManifestResult>`;
- exports and name/wrapper tests.

No UI layer opens a WebSocket or constructs a raw Bridge frame.

### Authoritative capability discovery

The editor does not treat the optional capability descriptors in `bridge.hello` as authoritative. After hello succeeds, the Rust connection path immediately calls `bridge.getCapabilities` on that same session and strictly parses the complete `capabilityDescriptor[]` result before reporting the connection ready. A malformed response is a protocol/backend connection error; it is not silently converted to an empty capability set.

The Rust IPC connection payload carries the parsed descriptors together with the existing `sessionId`. TypeScript/store derives a token set and records it against that connection generation. `useBridge` and the Asset Browser read `asset.reload` only from this generation-bound set. Disconnect or a changed `sessionId` clears it; reconnect performs a fresh `bridge.getCapabilities` call. NorvesLib adds `asset.reload` to its advertised list beside `asset.read` only when the new override is built.

### Asset Browser UI

Add one `Reload Runtime` button beside the existing manifest controls. It has no path input and never sends the editor's offline manifest path. The action invokes `assetReloadManifest()` only for the active connection and only when `asset.reload` is advertised.

- Disconnected or capability-absent: hidden or disabled with an accessible label; no invocation.
- `accepted:true`: clear any previous reload error and leave the offline Asset Browser list unchanged.
- `accepted:false`: surface a generic asset-runtime reload error in the existing asset error banner.
- `METHOD_NOT_SUPPORTED`: mark reload unsupported for that connection and degrade without changing connection state.
- Other transport/backend failures: surface as an asset error, not a workspace or connection mutation.
- Reconnect resets the per-connection capability/reload verdict so the next engine is evaluated independently.

After `accepted:true`, the editor does **not** automatically reload its offline manifest. The engine's configured manifest path and the editor text field can name different files; coupling them would create a false consistency claim.

## NorvesLib runtime design

### GameThread entry and candidate construction

`NorvesLibBridgeAdapter::assetReloadManifest` is appended as an override and delegates to `GameApplicationHandler::ReloadConfiguredAssetManifest()`. `GameApplicationHandler::OnUpdate()` calls `DrainInbound()`, so the callback and all state changes occur synchronously on GameThread. The adapter returns only `{accepted:true|false}` and does not retain params or engine pointers.

The handler accepts no path from the request. It reads the root and manifest path already established by `--texture-asset-root` and `--texture-asset-manifest`. It validates the configured files, reads the manifest synchronously, constructs a candidate `TSharedPtr<AssetSystem>` with the configured root, and parses the complete JSON before touching render runtime state. Failure returns false and leaves the current snapshot intact.

Startup uses this same handler and unified runtime entry. `OnPostInitialize()` no longer configures texture root and manifest separately; it constructs the candidate and performs the same texture+model installation used by Bridge reload.

### Unified RenderResources switch

Add one focused public entry on `RenderResources`, conceptually:

```cpp
bool ReloadAssetRuntimeSnapshot(
    const Container::String& assetRoot,
    Container::TSharedPtr<const Asset::AssetSystem> candidate);
```

The exact name may follow adjacent naming, but the contract is fixed: one non-null candidate and its matching root are applied to both `TextureAssetRuntime` and `ModelAssetRuntime`, or neither is changed. No singleton or new Manager is introduced.

The implementation uses runtime-specific private preflight/apply helpers. `RenderResources` acquires the texture asset mutex and then the model asset mutex in that fixed order; this is the only operation that holds both. While both locks are held it checks:

- `RenderResources` is initialized and not shutting down;
- both runtimes are bound/accepting and not closing;
- neither async queue has pending work or an active flush/callback;
- the candidate is non-null.

Texture async admission is tightened as part of this phase: `TextureAssetRuntime::LoadTextureAsync` holds `m_TextureAssetMutex` continuously through plan construction, cache lookup, duplicate registration, request/task creation, and queue enqueue/submit. A cache-hit handle and callback are copied while locked, but callback invocation is deferred until after unlock. No callback runs under the asset mutex. This follows the existing asset→async queue→cache lock discipline wherever locks nest; implementation review must verify the concrete cache/queue order against their current methods.

The serialization outcome is fixed. If a texture request acquires the mutex first, it is completely enqueued in its original generation and the subsequent reload sees pending work and returns `accepted:false`. If reload acquires both runtime mutexes first, it installs the new generation and the waiting request then builds its plan from the new snapshot. A plan built before reload can no longer be enqueued after reload. Model admission already keeps its generation selection and queue admission inside its asset-mutex critical section and must retain that property.

Therefore no new request can cross the preflight/apply window. If any preflight check fails, both locks are released with no mutation. If all checks pass, the same candidate pointer is installed into the texture resolver and model runtime before either lock is released. This is a purpose-built two-runtime critical section, not a reusable transaction framework.

Apply semantics under the locks:

- Texture resolver replaces root+snapshot together, advances texture generation exactly once, and retires current cache mappings without destroying handles still owned externally.
- Model runtime installs the same snapshot, advances model generation exactly once, and retires current cache entries. Unleased retired models are released after the two-runtime critical section; active leases/handles remain valid.
- Old worker plans retain their `TSharedPtr<const AssetSystem>` but fail generation checks before GPU work and again before publish/cache. They cannot publish into the new generation.
- No existing texture or model handle is rebound in place.

The existing standalone texture/model configuration APIs remain source-compatible but startup and Bridge reload use only the unified entry.

### Failure semantics and logging

`accepted:false` covers missing configuration, path/type/read failure, invalid manifest, uninitialized/shutting-down rendering, and busy texture/model runtime. Each failure emits one structured NorvesLib log with operation `asset_manifest_reload`, outcome `rejected`, and a stable reason such as `config_missing`, `manifest_invalid`, `texture_busy`, or `model_busy`. Success logs `outcome=accepted` and the new texture/model generations. Paths are logged only through the existing logger and are never returned over the wire.

No Bridge event is emitted. The editor receives the synchronous result; engine logs remain the diagnostic detail channel.

## Ownership, threading, and security

- The candidate is created locally and published as `TSharedPtr<const AssetSystem>`; workers hold immutable snapshots, never references to mutable manifest storage.
- File I/O and parsing happen on GameThread before the commit point. Async file I/O is deliberately excluded from this phase.
- The request carries no path, so a remote editor cannot select arbitrary files or escape the configured asset root.
- Lock order is texture asset mutex, model asset mutex, then each runtime's existing queue/cache order. Texture cache-hit callbacks and every completion callback are deferred until the relevant asset mutex is released.
- Active flush/callback state is a hard reject. The editor may retry explicitly after work drains; there is no hidden retry loop.
- Generation checks and snapshot ownership prevent stale-worker publication and use-after-free. Cache retirement preserves externally owned handles.

## Tests and acceptance

### NorvesEditor

1. Fixture validation covers the strict params/result payloads plus the `ENGINE_INVALID_PARAMS` registry row, correspondence mapping, and golden id-bearing response. Live SDK tests independently prove missing/null/non-object/non-empty params return that exact extension error before adapter invocation; only `{}` dispatches.
2. Rust parser tests accept `{accepted:true|false}` and reject missing, non-boolean, or extra fields; Tauri command tests prove method name, empty params, wire validation, and backend error mapping.
3. C++ SDK tests prove override dispatch and default `METHOD_NOT_SUPPORTED`, without reordering existing virtual declarations.
4. TypeScript name/wrapper tests prove `asset_reload_manifest`, `{}` invocation, and typed result forwarding.
5. Connection/UI tests prove strict `bridge.getCapabilities` discovery, capability omission disablement, generation-bound propagation, reconnect reset/fresh discovery, and NorvesLib advertisement of `asset.reload`.
6. UI tests prove one invocation on click, success clearing the reload error without reading an offline file, `accepted:false` asset-banner behavior, and advertised-but-default-unsupported `METHOD_NOT_SUPPORTED` degradation.
7. Existing fixture, Rust, C++, frontend typecheck, and UI suites stay green.

### NorvesLib

1. A fake-RHI unified reload contract test installs snapshot A, seeds/acquires old texture/model handles, reloads snapshot B, and proves both generations advance once, both new resolutions use B, and old handles/leases remain valid.
2. The same test proves invalid candidate, parse failure, texture pending/active, model pending/active, and active callback all reject with both snapshots, generations, and caches unchanged.
3. A deterministic barrier test pauses texture admission after plan creation at the former unlock point: reload cannot overtake it; the request fully enqueues against A and reload rejects busy. The inverse ordering proves reload installs B before the waiting request builds/enqueues against B.
4. Stale texture/model worker completions from generation A are rejected before publish and release newly uploaded/registered resources if staleness is observed post-upload.
5. A Bridge-enabled Game target builds against the new NorvesEditor SDK commit and adapter capability output contains `asset.reload` only when the override is compiled.
6. A loopback E2E test sends `asset.reloadManifest`, receives `accepted:true`, then resolves a texture and cooked model that exist only in manifest B, proving command-to-new-generation behavior. A busy variant receives `accepted:false` and still resolves through manifest A.
7. Focused targets and full CTest pass; existing line endings are preserved and new C++ files use UTF-8 BOM + CRLF.

Acceptance is reached only when the loopback path proves the real generic SDK dispatch, NorvesLib GameThread adapter, unified switch, and both new-generation resolvers. Directly calling the handler alone is insufficient E2E evidence.

## Cross-repository sequence and rollback

1. NorvesEditor first: implement and review protocol/schema/docs, SDK, Rust/Tauri/TS/UI, tests; commit and push `feature/asset-manifest-reload`.
2. NorvesLib second: use a NorvesEditor checkout pinned at that pushed commit as `NORVES_BRIDGE_SDK_DIR`, record the verified NorvesEditor commit hash in the phase evidence, implement the adapter and unified runtime switch, run Game/fake-RHI/loopback/full tests, then commit and push its M4 branch. NorvesLib has no repository revision field to edit.
3. Do not merge the NorvesLib consumer before the SDK commit is remotely available.

Rollback is reverse-order: revert the NorvesLib consumer first, then the additive NorvesEditor slice. Old engines remain usable throughout because they omit `asset.reload` and inherit `METHOD_NOT_SUPPORTED`; old editors never invoke the method.

## Explicit exclusions

- Arbitrary path or request-body fields
- Async manifest file I/O or automatic retry
- Existing-handle in-place rebinding
- Generic reload/transaction framework
- Bridge reload event or manifest bytes over Bridge
- Reusing `asset.getManifest` as a mutation
- Runtime reclustering, cooked format changes, or Phase 5 profiling/measurement
- Automatic editor offline-manifest reload after runtime acceptance

## Design self-review

- **Placeholder scan:** no placeholder marker, unresolved option, or provisional requirement remains.
- **Internal consistency:** authoritative capability discovery, strict params handling, serialized texture admission, GameThread execution, all-or-nothing switching, and UI failure semantics agree across layers.
- **Scope:** the design contains exactly the Phase 4 vertical slice and its compatibility/tests; Phase 5 measurement and generic frameworks are excluded.
- **Ambiguity:** method/capability/payloads, connection generation, admission ordering, commit point, busy behavior, snapshot ownership, UI action, SDK checkout pin, acceptance evidence, and rollback order are explicit.
