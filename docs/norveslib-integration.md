# NorvesLib Integration

This document describes how to build NorvesLib with the Bridge SDK embedded and
connect it to NorvesEditor as the reference engine.

For the generic connection contract (launch sequence, session, verified methods
and events) see [`docs/engine-integration.md`](engine-integration.md). For
adapter boundary rules (generic SDK vs. NorvesLib-specific adapter) see
[`docs/engine-integration.md` §NorvesLib Adapter](engine-integration.md#norveslib-adapter) and
[`docs/agent-guide/norveslib-adapter.md`](agent-guide/norveslib-adapter.md).

---

## Prerequisites

- **NorvesLib repository** checked out separately (not inside NorvesEditor).
  NorvesLib is not a submodule of NorvesEditor.
- **Windows** — NorvesLib builds and runs on Windows only.
- **Vulkan SDK** installed and on `PATH` / `VULKAN_SDK` env var set.
  The mock engine does **not** require Vulkan; only NorvesLib does.
- **Visual Studio 2022** (C++ workload, MSVC toolchain).
- **CMake 3.21+**.
- Network access for the first CMake configure of NorvesEditor's C++ layer
  (`libwebsockets` is fetched via FetchContent on first run; see
  [Known Limitations](#known-limitations)).

---

## Step 1 — Build the NorvesEditor C++ Bridge SDK

NorvesLib links against the C++ engine-side SDK that lives in
`bridge/cpp/engine-sdk` inside NorvesEditor. You do not need to build the full
NorvesEditor C++ tree; you only need the SDK headers and the CMake target it
exports.

No separate build step is required: NorvesLib's CMake will consume the SDK
in-source via `-DNORVES_BRIDGE_SDK_DIR`.

> The CMake flag `NORVES_BRIDGE_SDK_DIR` tells NorvesLib where to find the
> NorvesEditor `bridge/cpp` tree. The SDK is fetched as a CMake subdirectory
> (`add_subdirectory`), so `libwebsockets` is downloaded during this configure
> step (network required on first run).

---

## Step 2 — Build NorvesLib with Bridge SDK Embedded

Run the following commands inside the **NorvesLib repository** (not inside
NorvesEditor):

```powershell
# Configure — point at the NorvesEditor bridge/cpp tree
cmake -B build -S . -DNORVES_BRIDGE_SDK_DIR="<absolute path to NorvesEditor root>/bridge/cpp"

# Build the Game target (Vulkan SDK required)
cmake --build build --config Debug --target Game
```

The resulting executable:

```
build/Game/Debug/Game.exe
```

> Do **not** commit `build/` output. CMake build directories are not tracked by
> version control.

---

## Step 3 — Launch NorvesLib from NorvesEditor

NorvesEditor を起動し、Settings ウィンドウの「エンジン」欄の「参照…」で `Game.exe` を選ぶ。
選んだパスは保存され、次回以降の起動でも使われる。環境変数で指定することもできる:

```powershell
# 環境変数で指定する場合(保存したパスより優先される)
$env:NORVES_ENGINE_PATH = "<absolute path>\build\Game\Debug\Game.exe"
cd apps/editor
pnpm tauri dev
```

エディタが起動するエンジンのパスは、次の順に最初に見つかったものを使う:

1. 環境変数 `NORVES_ENGINE_PATH`(絶対パス)。
2. Settings ウィンドウの「エンジン」欄で保存したパス(「参照…」で選ぶ。
   アプリの設定ディレクトリの `engine-settings.json` に保存される)。
3. 既定値 `norves_mock_engine`(拡張子なしの名前。作業ディレクトリ基準で解決)。

環境変数が設定されているときは保存したパスより優先され、「エンジン」欄にもその旨が表示される。
同じ欄で起動引数(1 行 1 引数)も保存でき、`--bridge-port <port>` より前にシェルを介さず渡される。
`NORVES_NORVESLIB_ENGINE_PATH` はテスト専用の変数で、エディタ本体は読まない(Step 4 を参照)。

> `pnpm tauri dev` is the documented dev-mode launch command. Confirm it works
> on your machine before running the full integration scenario; local Vulkan /
> Tauri environment differences may require additional setup.

---

## Step 4 — Run env-gated e2e Tests Against NorvesLib

`apps/editor/src-tauri` is a **separate Cargo workspace** from the repository
root. Run these tests from inside that directory:

```powershell
# Set the NorvesLib engine path
$env:NORVES_NORVESLIB_ENGINE_PATH = "<absolute path>\build\Game\Debug\Game.exe"

# Run the integration test suite
cd apps/editor/src-tauri
cargo test --test process_e2e
```

- When `NORVES_NORVESLIB_ENGINE_PATH` is set, the following contracts are
  exercised: `engine_runtime_control_contract`,
  `engine_launch_info_schema_compliance_contract`, and
  `engine_event_streaming_contract`.
- When the variable is **not** set, each test function prints a `[SKIP]` line
  and returns immediately; the suite still passes. CI uses this opt-in pattern.

Use `cargo test --test process_e2e` (integration test by filename), not
`cargo test -p <crate>`, because the crate lives in an excluded workspace and
must be addressed by changing into its directory first.

---

## Schema Compliance: NorvesLib vs. Mock Engine

The `engine.launchInfo` and `log.subscribe` response schemas use
`additionalProperties: false`. NorvesLib and the reference mock engine differ
in compliance:

| Method | NorvesLib adapter (compliant) | `norves_mock_engine` (non-compliant) |
|---|---|---|
| `engine.launchInfo` | `{ "pid": <int ≥ 0>, "title": "<string>" }` | `{ "launched": true }` |
| `log.subscribe` | `{ "subscriptionId": "<non-empty string>" }` | `{ "subscribed": true }` |

The `engine_launch_info_schema_compliance_contract` e2e test asserts:

- `pid` is present and `>= 0`.
- `title` is present and non-empty.
- The `launched` key is absent (confirming `additionalProperties: false`
  compliance).

The `engine_event_streaming_contract` e2e test exercises `log.subscribe` and
`runtime.stateChanged` event delivery against the live NorvesLib adapter.

For the full table of verified methods and events see
[`docs/engine-integration.md` §Verified Methods](engine-integration.md#verified-methods-alpha).

---

## Adapter Boundary

The generic C++ Bridge SDK (`bridge/cpp/engine-sdk`) must not contain NorvesLib
headers or NorvesLib-specific logic. The NorvesLib adapter (which lives in the
NorvesLib repository) is responsible for:

- Mapping NorvesLib runtime/log/status into Bridge DTOs.
- Marshaling Bridge runtime commands onto the safe NorvesLib thread/context.
- Avoiding direct transport of NorvesLib live object memory.
- Keeping NorvesLib-specific containers and object rules out of the generic SDK.

See [`docs/engine-integration.md` §Generic Boundary](engine-integration.md#generic-boundary)
and [`docs/agent-guide/norveslib-adapter.md`](agent-guide/norveslib-adapter.md)
for the full boundary contract.

コンポーネント投影(`object.getSnapshot` の `components` とコンポーネント単位のプロパティ
読み書き)を NorvesLib 側で結線する手順と受け入れ条件は
[`docs/norveslib-component-projection-contract.md`](norveslib-component-projection-contract.md)。
その上に乗るコンポーネントの追加/削除(`component.add` / `component.remove` と型名からの生成)は
[`docs/norveslib-component-edit-contract.md`](norveslib-component-edit-contract.md)。

---

## Known Limitations

1. **Windows + Vulkan SDK required.** NorvesLib builds and runs on Windows only
   and requires a Vulkan SDK. The reference mock engine (`norves_mock_engine`)
   does not require Vulkan and can be used for protocol development on any
   supported platform.

2. **Network required on first CMake configure.** The C++ Bridge SDK uses
   CMake `FetchContent` to download `libwebsockets` v4.3.3. Subsequent builds
   use the cached download.

3. **シーン・オブジェクト・スキーマの操作は、エンジンが実装している範囲だけ使える。**
   NorvesLib アダプタ(`Game/Bridge/NorvesLibBridgeAdapter`)は `scene.getTree`、
   `scene.createObject` / `deleteObject` / `reparentObject` / `duplicateObject`、
   `object.getSnapshot` / `setProperty`、`schema.getSnapshot` を実装している。
   エンジンが実装していないメソッドには `not_supported` が返り、エディタはその接続の間、
   その操作を使えないものとして扱う(編集操作なら無効化する)。

4. **No native viewport embedding.** The engine runs its own native window;
   NorvesEditor does not embed GPU output inside the Tauri WebView. See
   [`docs/viewport-strategy.md`](viewport-strategy.md) for the alpha viewport
   approach and post-alpha research directions.

5. **エンジンのパスと起動引数は Settings の「エンジン」欄で保存できる。** パスは
   環境変数 `NORVES_ENGINE_PATH` > 保存した設定 > 既定値 の順に決まる(Step 3)。
   パスはダイアログで選んだファイルだけを保存でき、`.bat` / `.cmd` は起動しない
   (Windows ではバッチファイルが cmd.exe 経由で起動され、引数がシェルを通るため)。

6. **エディタの強制終了時にエンジンが残る可能性。** Windows では、起動したエンジンを
   `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` 付きの Job に入れる。Job への割り当てに成功した
   あとなら、エディタが強制終了してもエンジンは終わる。次の場合はエンジンが残りうる:
   - Job の作成か割り当てに失敗したとき(警告をバックエンドのログに残し、起動はそのまま続ける)
   - 起動から Job への割り当てまでの短い間に、エディタが終了したとき
   - 起動から割り当てまでの間に、エンジンが子孫プロセスを起動したとき(その子孫は Job の外)
   - Windows 以外の OS

7. **localhost only.** The Bridge transport binds to `ws://127.0.0.1:<port>`.
   Remote or cross-machine connections are not supported.
