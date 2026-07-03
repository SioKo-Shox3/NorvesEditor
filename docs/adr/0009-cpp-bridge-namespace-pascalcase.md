# 0009: C++ Bridge Namespace PascalCase Alignment

Status: Accepted for alpha

## Context

ADR 0008 section 4 kept the SDK namespace as `norves::bridge` (lower-case). That decision no longer matches the first reference engine's nested namespace style, where NorvesLib uses PascalCase namespaces such as `NorvesLib::Core`.

This ADR intentionally supersedes only the namespace-related part of ADR 0008:

- ADR 0008 section 4 lines 64-65 decided to keep `norves::bridge` lower-case. This ADR replaces that decision with `Norves::Bridge`.
- ADR 0008 Consequences line 89 said NorvesLib needs no changes. That statement depended on the namespace staying lower-case and is no longer true after this ADR.
- ADR 0008's other decisions still hold: C++23 adoption, frozen public API symbol names, DTO member names and order, UTF-8 BOM public headers, and LF line endings.

The namespace was part of the previously frozen public surface, but this ADR selectively unfreezes that one category. The frozen contract as a whole is not removed: method names, type names, enum names, DTO member names, and public function names remain frozen.

Namespace unfreezing is acceptable for this alpha change because:

- Namespace qualifiers and include paths can be migrated mechanically across all consuming code with find-and-replace. That is different from changing method names, which requires understanding call sites and behavior.
- NorvesEditor and NorvesLib are controlled by the same team across two repositories, and this ADR establishes the rule that both repositories must be updated in lockstep for the namespace transition.
- This is a user-approved intentional breaking change, not an unannounced compatibility break.

## Decision

1. Rename the C++ Bridge SDK namespace from `norves::bridge` to `Norves::Bridge`.
2. Rename subnamespaces:
   - `norves::bridge::dto` becomes `Norves::Bridge::Dto`.
   - `norves::bridge::detail` becomes `Norves::Bridge::Detail`.
3. Move public include folders:
   - `bridge/cpp/engine-sdk/include/norves/bridge/**` becomes `bridge/cpp/engine-sdk/include/Norves/Bridge/**`.
   - `bridge/cpp/engine-sdk/include/norves/bridge/dto/**` becomes `bridge/cpp/engine-sdk/include/Norves/Bridge/Dto/**`.
   - Header filenames remain lower-case.
4. Keep frozen public API symbol spellings unchanged. In particular, all `IBridgeEngineAdapter` virtual method names remain exactly: `hello`, `getCapabilities`, `getStatus`, `launchInfo`, `runtimePlay`, `runtimePause`, `runtimeStop`, `runtimeFocusViewport`, `logSubscribe`, `logUnsubscribe`, `sceneGetTree`, `sceneCreateObject`, `sceneDeleteObject`, `sceneReparentObject`, `sceneDuplicateObject`, `objectGetSnapshot`, `objectSetProperty`, `schemaGetSnapshot`, `assetResolve`, `assetGetManifest`, `viewportGetThumbnail`.
5. Keep the UTF-8 BOM / LF policy unchanged:
   - public headers under `bridge/cpp/engine-sdk/include/**` keep a UTF-8 BOM and LF line endings;
   - internal SDK files under `src`, `tests`, and `examples` stay BOM-less UTF-8 with LF line endings.
6. `norves::test` in `bridge/cpp/engine-sdk/tests/test_support.hpp` is not part of the Bridge SDK namespace and is intentionally out of scope.

## Consequences

- ADR 0008's statement that "NorvesLib needs no changes" no longer holds after this ADR.
- NorvesLib include directives and `norves::bridge` qualifiers intentionally stop compiling after the N1 NorvesEditor merge until the NorvesLib N2 lockstep migration is applied.
- The wire protocol and runtime behavior are unchanged. This is a source-level namespace/include-path migration only.
- New C++ Bridge SDK code must use `Norves::Bridge` / `Norves::Bridge::Dto` / `Norves::Bridge::Detail`.

## Affected workstreams

- **F (C++ SDK):** public headers, internal SDK translation units, tests, and examples adopt the PascalCase namespace and include path.
- **L (NorvesLib adapter):** must update include paths and namespace qualifiers in lockstep as the N2 follow-up.

## Verification or migration notes

N1 verification records:

```powershell
# Namespace/include/detail residual checks
# Result: 0 unexpected `norves::` residuals (only `norves::test` in
# test_support.hpp usages and the unrelated `norves::mock` example namespace
# remain, both explicitly out of scope). 0 residual `#include "norves/bridge/`.
# 0 unqualified `detail::` under engine-sdk/src or engine-sdk/include.
# bridge/cpp/third_party untouched.

# BOM/EOL byte checks
# Result: all 17 public headers keep UTF-8 BOM + LF (no CR). All 21 internal
# src/tests/examples files stay BOM-less UTF-8 + LF (no CR). No whole-file
# line-ending flips (git diff --numstat matched small per-file line counts).

# C++ build and tests
# Result: cmake --build build/cpp --config Debug -> exit 0.
# ctest --test-dir build/cpp -C Debug --output-on-failure -> 7/7 passed
# (smoke_test, fixtures_roundtrip_test, dispatch_test, bounded_queue_test,
# loopback_roundtrip_test, ws_server_transport_test, mock_engine_loopback_smoke).
```