# アセット閲覧MVP + プロジェクト管理 実装計画

> **ステータス**: 承認済み（2026-06-29）。
> **進捗**: ✅ Phase A（ワークスペース管理、34c8ec8）/ ✅ Phase B（オフライン Asset Browser/Inspector、0efb5e9）/ ✅ Phase C（Bridge プロトコル拡張 `asset.resolve` + `asset.getManifest`、cdc6750、fixtures 124→132）/ ✅ Phase D（Bridge live 健全性オーバーレイ、cf783fb）完了・main 統合済み（いずれも実装=Codex / レビュー=Claude impl-reviewer + Codex review 二重 / 全ゲート緑、C++ ctest 含む）。**残るは Phase E（別リポ `../NorvesLib`：列挙API露出 + アダプタ override + `asset.read` token）のみ。** これでエディタ内(NorvesEditor)のアセット閲覧MVPは完結。
> **Phase D 確定事項**: resolve の一時失敗は専用 `assetResolveErrorByKey`(per-key「未確定」)に閉じ manifest バナー(`assetError`)を汚さない。レースガードは選択キー + 接続世代(sessionId)の両方。live `asset.getManifest` の UI 配線は補助用途のためスコープ外(`asset.resolve` 健全性が主)。実 cooked/loose 健全性の実機確認は Phase E 後。
> **Phase C 確定事項**: wire の `version`/`cookedVersion` は **integer**（実 NorvesLib uint32・Phase B offline と一致、Phase D で AssetEntry を offline/live 共用するため）。asset 系 IPC 型は Phase B 先例どおり `bridge-ui/ipc-types.ts` に集約（`bridge-types/asset.ts` は作らない）。mock/conformance 不変、`asset.read` token は docs 記載のみ（実広告は Phase E）。
> **Phase B D3 確定**: ランタイム manifest は `<RuntimeRoot>/manifest.json`（source の `Assets/` とは別、`cooked_package` は RuntimeRoot 相対）。固定探索でなく明示パス `asset_read_manifest(manifest_path)` を採用。manifest 実スキーマ(snake_case): `{version:1, assets:[{logical_path, kind, source_hash, variant, format, cooked_package, entry_name, entry_type, cooked_hash, cooked_version}]}`（loose は cooked 系欠落）。
> **由来**: ギャップ調査 → planner → plan-reviewer（verdict: approve-with-changes, blocker 0）→ 改訂 を経た最終版。
> 報告/計画/コミットは日本語、コード/型/API/パス/コマンドは英語（CLAUDE.md 準拠）。

## 確定事項（ユーザー承認 2026-06-29）

本計画はユーザー承認時に以下を確定した。**本文中の「任意ブロック」「D1 採用時」という条件付き記述は、すべて「採用＝スコープ確定」として読むこと。**

- **D1（確定: 含める）**: live `asset.getManifest`（エンジンがロード中の manifest 列挙）を MVP スコープに**含める**。Phase C/D/E の `asset.getManifest` 任意ブロックはすべて実施対象。スケール対策の `filter`/`page`/`pageSize`/`totalCount` を最初から protocol に持たせる。
- **D2（確定）**: capability token = `asset.read`（cross-repo 固定）。
- **D3（既定で進行可）**: `manifest.json` 探索は `<root>/Cooked/manifest.json` を既定とし、`AssetEntryDto` の cooked 由来フィールドを全 optional 化して loose 追加を additive に吸収する安全装置を採用。実際の探索規約・loose 一覧表示の最終確定は Phase B 着手時に NorvesLib の cook 出力を確認して詰める。
- **D4（確定）**: ワークスペース選択はパス文字列入力（`tauri-plugin-dialog` は非導入）。

- **制約（ユーザー明示）**: モックエンジン（`norves_mock_engine` / `mock_adapter`）はこれ以上整備しない。asset 対応は NorvesLib override + 汎用 SDK virtual（`not_supported` デフォルト）のみ。conformance runner も asset ステップ非追加。

---

# NorvesEditor 実装計画(最終版) — アセット閲覧MVP + プロジェクト/ワークスペース管理(PROJ-1)

## レビュー反映サマリ(何を直したか)

1. **[major] AssetPath 移植の誤り修正**: 実コード `AssetPath.cpp:202-235` は絶対パス(`C:/`・`/`)と `..`(root 内解決)を**受理**し、拒否するのは drive-relative(`C:foo`)と UNC(`//`)のみ。Phase A を「AssetPath を移植」から「**AssetPath を参照しつつ logical-path 専用に絶対/UNC/drive-relative/`..` を全拒否する独自バリデータを新規定義**」に変更し、AssetPath との差分を検証テスト表で固定。
2. **[major] `asset.resolve` status enum を9値全網羅に修正**: `AssetResolveResult.h:14-25` の `AssetResolveStatus` は9値(計画が落としていた `LooseReadFailed`/`CookedPackageParseFailed` を含む)。Phase C の wire enum を9値完全 superset として確定し schema に固定(0.4節)。
3. **[minor] fixture カウント行をカテゴリ別に一般化**: Rust `fixtures_roundtrip.rs` の連動 assert を実行確認し(positive=:124/:130-133/:175、payload negative=:129/:130-133/:221、envelope negative=:126/:130-133/:195)、「追加 fixture カテゴリ別の全カウント assert」として記載。
4. **[minor] verify.ps1 と src-tauri の役割分担を明記**: `verify.ps1` はルートワークスペース(bridge crates)のみ。src-tauri の新規 Rust テストは `cd apps/editor/src-tauri; cargo test` でのみ走る点を全フェーズの検証節に注記。
5. **openQuestion を「承認前に確定すべき決定」へ昇格**: OQ5(token名)/OQ7(getManifest 要否)/OQ2・3(manifest 探索・loose)を承認前決定事項として明示。未決のまま着手する場合の安全装置(DTO の cooked フィールド全 optional 化等)を各フェーズに組込み。
6. **検証ギャップ充足**: negative fixture の有無・層、Phase D のレース検証ケース、パス正規化共有テスト表の具体入力、Phase E の NorvesLib 検証コマンド/テストケースを追記。

---

## 0. 計画全体の方針・グラウンディング・源泉(source-of-truth)設計

### 0.1 裏取りで確定した既存実装の現状(推測でなく実コード)

- bridge/spec のメソッド schema は 15 種が既存、scene/object/schema/viewport は実装済み(`bridge/spec/schema/methods/`)。`asset.*` は schema/fixtures とも**不在**。
- fixture カウントは **C++ 4 assert + Rust 7 assert(カテゴリ別)** にハードコード。現状 positive=61 / envelopeRejectable=14 / payloadOnly=49 / total=124。Rust の連動 assert(実行確認済み):
  - positive: `fixtures_roundtrip.rs:124`(分類)/`:130-133`(total)/`:175`(roundtrip 件数)
  - envelopeRejectable: `:126`(分類)/`:130-133`(total)/`:195`(reject 件数)
  - payloadOnly: `:129`(分類)/`:130-133`(total)/`:221`(accept 件数)
  - C++: `fixtures_roundtrip_test.cpp:192`(positive)/`:195`(total)(他カテゴリ assert も同箇所群)
  **fixture を1件足すたび、追加カテゴリに対応する全 assert を同一コミットで更新しないと両 workspace test が壊れる(=自動検出される)。**
- `IBridgeEngineAdapter`(`adapter.hpp:99-132`)の拡張パターン: 必須=純粋仮想、オプション=**非純粋 + `not_supported(params)` デフォルト**(`:143-148`)。新メソッドは同パターンで追加し、未対応エンジンも `METHOD_NOT_SUPPORTED` を返す。
- C++ dispatch は `server.cpp:182-241` の `if (method == "...")` 直列ルーティング。新メソッドはここに分岐追加。
- Rust コマンド定型(`bridge_state.rs`): `send_method(state, "<method>", params)` → editor-client の `parse_*_result` で drift-guard 検証 → **元 wire Value を素通し**(`scene_get_tree`:604-613 が手本)。`dto.rs:1-7` は「backend が合成する DTO のみ定義、engine result は Value 素通し」が明示方針。
- editor-client parse 層は `scene.rs:63` `parse_scene_tree_result` が手本。`lib.rs:29-46` で `pub use` 公開。
- IPC名3点同期: `protocol_names.rs`(commands+events+assertion test)↔`bridge-ui/src/commands.ts`↔`events.ts`、`scripts/check-protocol-names.mjs` が強制。**コマンド名に数字禁止**規約。
- TS アクション手本: `useBridge.ts` の `getSceneTree`(:421)/`getObjectSnapshot`(:444)。`isMethodNotSupported`(:89)で graceful degradation。
- dockview パネル追加: `AppLayout.tsx:92-97` `PANEL_COMPONENTS` 登録 + `buildDefaultLayout`(:134) `api.addPanel`。Connection/Settings は別 Tauri window(`SecondaryWindowRoot`)へ移行済み(:27)。
- conformance runner(`alpha_method_sequence.json:62-65`)は `bridge.getCapabilities` を fixture と **ignore 無し厳密 exact-match**。現行 mock は 8 token 広告(`mock_adapter.hpp:72-82`)で fixture と一致。
- **NorvesLib リポ実在**(`C:\Users\KINGkawamura\Documents\NorvesLib`)。`AssetSystem`(`Library/Core/Public/Asset/AssetSystem.h`)は `m_Manifest` を private 保持、**列挙アクセサ無し**。`AssetManifest::GetReferenceCount()/GetReference(index)`(`AssetManifest.h:138-139`)は public だが `AssetSystem` 経由で到達不能。`NorvesLibBridgeAdapter` は scene/object/schema を override 済み、asset メソッド無し、capability は **6 token**(`NorvesLibBridgeAdapter.cpp:1120-1126`)。
- **`AssetResolveStatus` は9値**(`AssetResolveResult.h:14-25`、裏取り済み): `SuccessCooked`/`SuccessLoose`/`InvalidRequest`/`InvalidManifest`/`LooseReadFailed`/`CookedPackageReadFailed`/`CookedPackageParseFailed`/`CookedEntryMissing`/`CookedEntryHashMismatch`。`AssetResolveSource` は4値: `None`/`Cooked`/`Loose`/`DebugLooseFallback`。`AssetResolveResult.Blob`/`LoosePath`/`Entry` 等を保持。
- **`AssetPath::Normalize` の実挙動**(`AssetPath.cpp:192-235`、裏取り済み):
  - 拒否するのは **drive-relative(`C:foo`)と UNC(`//server`)のみ**(`:202-205`)。
  - **絶対パス(`C:/...`・`/...`)は受理**し `PathKind::Absolute` の有効値になる(`:210-222`)。
  - **`..` は拒否されず**、`NormalizeSegments` でレキシカル解決、root を越える時のみ false(`:97-104`)。
  - **結論: AssetPath をそのままコピーすると絶対パス・traversal 解決済みパスを受理してしまう。logical-path 専用バリデータとしては不適合。**
- manifest.json wire 形式は **snake_case**(`AssetManifestTest.cpp:35-47,181-184`): `{"version":1,"assets":[{"logical_path","kind","source_hash","variant","format","cooked_package","entry_name","entry_type","cooked_hash","cooked_version}]}`。
- Tauri 側に `tauri-plugin-fs`/`tauri-plugin-dialog` 未導入(`apps/editor/src-tauri/Cargo.toml`)。PROJ-1 のファイル選択/読込は**カスタム Rust コマンド + std::fs**(JS fs プラグイン不使用)。capabilities は `core:default` 中心、fs 系 permission 追加不要(バックエンド完結)。
- **`verify.ps1` の射程**(`scripts/verify.ps1:51,64`): Rust ゲートは**ルートワークスペース(bridge crates のみ)**。`apps/editor/src-tauri` は別ワークスペースとして除外され、verify.ps1 では検証されない。src-tauri の Rust テストは `cd apps/editor/src-tauri; cargo test` でのみ実行される。

### 0.2 オフライン manifest 直読み vs Bridge live の source-of-truth 設計

**推奨順序: オフライン manifest 直読み(PROJ-1 + 直読み)を先行、Bridge live を後続。** 根拠:

1. **依存ゼロで縦に通せる**: オフライン経路は NorvesLib 改修も Bridge 接続も不要。Rust backend が manifest.json をパースするだけで Asset Browser/Inspector が動く。UI/DTO/状態管理を先に固め、後続 live は「同じ DTO に別供給元を足す」だけになる。
2. **live 最大リスク(NorvesLib 列挙API欠如)を後段隔離**。先行フェーズが UI 契約を確定するので、クロスリポ作業は「確定済み wire 契約に NorvesLib を合わせる」明確なゴールになる。

**source-of-truth の関係**:
- **オフライン直読み(workspace manifest)が編集セッションの primary。** Asset Browser は「現在開いているワークスペースの manifest.json をパースした結果」を正典表示。未接続でも成立。
- **Bridge live は engine の実行時ビュー(secondary)。** `asset.getManifest` は「接続中エンジンがロード中の manifest」、`asset.resolve` は健全性(cooked/loose/hash mismatch 等)確認専用。live は閲覧の primary を置き換えない。
- **同一性キー**: NorvesLib に GUID/UUID 無し(裏取り済み)。参照同一性=`logicalPath + variant`、内容同一性=`sourceHash/cookedHash`。突き合わせは `logicalPath + variant`。
- **UI 表示モード**: 未接続=オフライン manifest のみ(健全性「未検証」)。接続中=オフライン manifest を base に `asset.resolve` の健全性をオーバーレイ。**接続有無で一覧の中身は変えず、健全性カラムの埋まり方だけが変わる**(切断でリスト点滅しない)。

### 0.3 protocol version の扱い

現行 `PROTOCOL_VERSION = "0.2"`(`bridge_state.rs:65`)。`asset.*` は **0.2 への additive 追加**で version bump 不要(`common.schema.json` の version pattern `^[0-9]+\.[0-9]+$` は制約せず、新規 fixture の envelope version は "0.2")。

### 0.4 capability token と `asset.resolve` enum の確定(取り違え防止・全網羅)

- **capability token は `asset.read` で確定(OQ5 を承認前に決定)。** 規約 `^[a-z][a-zA-Z0-9]*\.[a-zA-Z0-9]+$` を満たす。token は cross-repo 契約のため、Phase C の docs(`capabilities.md`)と Phase E の NorvesLib 実装(`NorvesLibBridgeAdapter.cpp:1120-1126`)で**同一文字列**を使う。
- token 追加先は **NorvesLib `NorvesLibBridgeAdapter.cpp:1120-1126`(現状6 token)**。NorvesEditor の mock(`mock_adapter.hpp:72-82`、8 token)は触らない(ユーザー制約)。よって **conformance runner の exact-match は変更不要**。
- **`asset.resolve` の wire status enum(9値完全 superset、`AssetResolveStatus` 全網羅)**:
  | NorvesLib enum | wire status 文字列 |
  |---|---|
  | `SuccessCooked` | `successCooked` |
  | `SuccessLoose` | `successLoose` |
  | `InvalidRequest` | `invalidRequest` |
  | `InvalidManifest` | `invalidManifest` |
  | `LooseReadFailed` | `looseReadFailed` |
  | `CookedPackageReadFailed` | `cookedPackageReadFailed` |
  | `CookedPackageParseFailed` | `cookedPackageParseFailed` |
  | `CookedEntryMissing` | `cookedEntryMissing` |
  | `CookedEntryHashMismatch` | `cookedEntryHashMismatch` |
- **`asset.resolve` の wire source enum(4値、`AssetResolveSource` 全網羅)**: `none`/`cooked`/`loose`/`debugLooseFallback`。
- enum は汎用文字列として schema に固定。Phase E で NorvesLib enum を漏れなく写像可能(マップ不能 status が出ない)。

---

## フェーズ分割と順序(各 1ブランチ1テーマ)

```
Phase A: PROJ-1 ワークスペース/アセットルート管理(Rust backend FS 所有・Bridge 非依存)
Phase B: オフライン manifest 直読み + Asset Browser/Inspector UI(健全性「未接続=未検証」)
Phase C: Bridge プロトコル拡張(asset.resolve [+ asset.getManifest] schema + fixtures、汎用 SDK virtual + Rust/TS 配線)
Phase D: Bridge live 経路の UI 統合(接続中の健全性オーバーレイ)
Phase E: 【クロスリポ・別リポ NorvesLib】列挙API露出 + アダプタ override + capability token 追加
```

依存: A → B。C は A/B と独立着手可だが D は B+C 依存。E は C 依存・D の実動確認に必要。**A→B 先行、C→E→D の順で live を重ねる**(D の実装は C 完了後、受け入れは E 後)。

---

## 承認前に確定すべき決定事項(着手前に user 回答が必要)

これらは計画の根幹を左右するため、承認時に確定する:

- **D1(=OQ7 昇格)**: live `asset.getManifest` を MVP に含めるか。**`asset.resolve` のみで MVP が成立するなら Phase C/D から `asset.getManifest`(schema・fixtures・C++ virtual・Rust/TS 配線)を落とし protocol サーフェスを半減**。本計画はデフォルトで `asset.resolve` 主体に組み、`asset.getManifest` は「含める場合のみ追加する任意ブロック」として分離記述する。
- **D2(=OQ5 昇格)**: capability token 名 = `asset.read`(本計画確定値)。cross-repo で固定。
- **D3(=OQ2/OQ3 昇格)**: manifest.json の探索規約(`<root>/Cooked/manifest.json` 等)と、loose アセットを Asset Browser に出すか。未確定の場合の安全装置: **`AssetEntryDto` の cooked 由来フィールドを全て optional 化**し、loose 追加を additive に保つ(Phase B 着手前に確定が望ましいが、optional 化で着手は可能)。
- **D4**: ワークスペース選択 UX(OQ1)= alpha はパス文字列入力(`tauri-plugin-dialog` 非導入)。

---

## Phase A — PROJ-1 ワークスペース / アセットルート管理

### 目的・期待される挙動変化
エディタにワークスペース(プロジェクトルート)を開く/記憶する機能を追加。Rust backend が**ファイルシステムを所有**し、ワークスペースルートと `Assets/` 正規化を管理。Bridge 非依存の純エディタ機能。UI からワークスペースを選択でき、**logical-path 専用バリデータ**で論理パスを検証する。

### 触るファイル(正確なパス)
- 新規 Rust: `apps/editor/src-tauri/src/workspace.rs`(状態 + コマンド `workspace_open`/`workspace_get`/`workspace_close` + **logical-path バリデータ**)
- `apps/editor/src-tauri/src/lib.rs:28-44`(`manage(WorkspaceState::default())` + `invoke_handler!` に3コマンド登録)
- `apps/editor/src-tauri/src/protocol_names.rs:19-35`(`commands::WORKSPACE_OPEN`/`WORKSPACE_GET`/`WORKSPACE_CLOSE` + 重複ガード配列 + assertion test)
- `apps/editor/src-tauri/src/dto.rs`(`WorkspacePayload { rootPath, assetsRoot, name }` camelCase Serialize)
- TS: `bridge/ts/packages/bridge-ui/src/commands.ts:15-31`(`workspaceOpen`/`workspaceGet`/`workspaceClose`)、`ipc-types.ts`(`WorkspacePayload`)+ index export
- TS 状態: `apps/editor/src/state/store.ts`(`workspace?`、`workspaceOpened`/`workspaceClosed` action + reducer + INITIAL_STATE)
- TS hook: `apps/editor/src/hooks/useBridge.ts`(`openWorkspace`/`getWorkspace`/`closeWorkspace`)
- UI: `apps/editor/src/components/SettingsPanel.tsx`(ワークスペース open/表示 UI、`SecondaryWindowRoot` 経由)
- 新規ユーティリティ: `apps/editor/src/shell/assetPath.ts`(**logical-path 専用バリデータ**: `Assets/` プレフィックス剥がし、絶対/UNC/drive-relative/`..` を**全拒否**、レキシカル正規化)
- テスト: `workspace.rs`(`#[cfg(test)]` 正規化・拒否)、`store.test.ts`、`assetPath.test.ts`、SettingsPanel レンダリング(jsdom)

### logical-path バリデータの定義(AssetPath との差分を明記)
**AssetPath をそのままコピーしない。** AssetPath は logical/absolute 両対応のファイルシステムパス正規化器であり、editor の論理パス検証要件と一致しない。本計画では AssetPath の `NormalizeSegments` ロジック(`AssetPath.cpp:85-131`)を**参照しつつ**、以下の logical-path 専用ルールを Rust(`workspace.rs`)と TS(`assetPath.ts`)で**同一仕様**として新規定義:

- 受理: `Assets/` プレフィックス(任意・剥がす)、相対 segment、`.` segment(無視)。
- **拒否(AssetPath との差分)**: 絶対パス(`C:/…`・`/…` — AssetPath は受理するが本バリデータは拒否)、`..` を含む(root 内解決可でも拒否 — AssetPath は解決して受理)、UNC(`//`)、drive-relative(`C:foo`)、空文字。
- 出力: 正規化済み logical path(forward-slash 区切り、`Assets/` 剥離済み)。

### 共有テストケース表(Rust/TS 同一・検証成果物として要求)
両側で同じ入力→同じ判定を固定する。AssetPath との差分(★)を含む:

| 入力 | 期待判定 | 期待出力/理由 |
|---|---|---|
| `Assets/textures/hero.png` | accept | `textures/hero.png` |
| `textures/hero.png` | accept | `textures/hero.png` |
| `./Assets/a/b.png` | accept | `a/b.png` |
| `Assets/a/./b.png` | accept | `a/b.png` |
| `C:/abs/x.png` | **reject** ★ | 絶対パス拒否(AssetPath は Absolute で受理) |
| `/abs/x.png` | **reject** ★ | root 絶対拒否(AssetPath は Absolute で受理) |
| `a/../b.png` | **reject** ★ | `..` 拒否(AssetPath は `b.png` に解決し受理) |
| `../escape.png` | reject | root 越え(AssetPath も false) |
| `//server/share/x` | reject | UNC(AssetPath も Invalid) |
| `C:rel.png` | reject | drive-relative(AssetPath も Invalid) |
| `` (空) | reject | 空(AssetPath も Invalid) |

### 具体的ステップ
1. ワークスペース選択(D4 確定): `workspace_open(root_path: String)` でパス文字列受領 → `std::fs::metadata` でディレクトリ存在検証 → `Assets/` サブディレクトリ存在確認 → `WorkspacePayload` 返却。ネイティブダイアログはスコープ外。
2. `WorkspaceState`(`std::sync::Mutex<Option<Workspace>>`、同期 fs のため await 跨ぎロック無し。`bridge_state.rs` の no-lock-across-await 規約と整合)。
3. logical-path バリデータを Rust/TS に同一仕様で新規定義(上記表を両側テストに焼く)。**NorvesLib コードは参照のみ・依存しない**(generic editor の NorvesLib 依存禁止境界を厳守)。
4. IPC名3点同期 → store/hook 配線 → SettingsPanel UI。

### 新規プロトコル schema+fixture
**なし。** PROJ-1 は Bridge 非依存。wire protocol 不変。

### 所有権 / 寿命 / スレッド / async / permission
- **Rust backend が FS を所有**(CLAUDE.md 境界準拠)。`WorkspaceState` は Tauri `manage` でアプリ寿命管理、`BridgeState` と完全独立(相互参照しない)。
- `std::fs` 同期I/O はコマンド内短時間。await 跨ぎロック無し。
- Tauri permission: カスタムコマンドはバックエンド完結のため fs プラグイン permission 不要。`capabilities/default.json` 変更不要(**要レビュー確認**)。
- engine live memory 非該当。

### protocol schema/fixture 変更の有無
なし。

### base / 作業ブランチ / stacked
base: `main`、work: `feature/workspace-management`。stacked 依存なし(最初に着手可)。

### 検証コマンドと期待結果
```powershell
node scripts/check-protocol-names.mjs                 # 3点同期 OK(新コマンド名に数字なし)
# src-tauri は別ワークスペース。verify.ps1 では検証されないため個別実行が必須:
cd apps/editor/src-tauri; cargo fmt --all -- --check; cargo clippy --workspace --all-targets -- -D warnings; cargo test  # workspace.rs 正規化/拒否テスト緑
pnpm -r --if-present typecheck                          # WorkspacePayload 型整合
cd apps/editor; pnpm test                              # store/assetPath(上記表)/SettingsPanel 緑
./scripts/verify.ps1                                   # fixtures(不変124) + ルートワークスペース Rust(src-tauri は含まない)
```
**verify.ps1 と src-tauri cargo test の役割分担**: verify.ps1 はルートワークスペース(bridge crates)+ fixtures のみ。本フェーズの新規 Rust(`workspace.rs`)は `cd apps/editor/src-tauri; cargo test` でのみ走る。両方を必ず実行する。
期待: 全緑。fixture カウント **124 不変**。

### リスクと封じ込め
- *logical-path バリデータの Rust/TS ズレ*(中): 上記共有テスト表を両側で同一に固定(検証成果物として要求)。AssetPath との差分(絶対/`..` 拒否)を明示テストで担保。
- *AssetPath をそのまま流用する誤実装*(中・レビュー指摘起因): 「AssetPath コピー禁止・logical 専用新規定義」を実装指示に明記。impl-review で差分表の遵守を確認。
- *ワークスペース状態と接続状態の混線*(低): 独立 managed state。

### 担当
実装=Codex 委託。レビュー=Claude + Codex 二重。**Opus エスカレーション=必要**(Tauri permission + FS 所有 + 新コマンド + パス検証セキュリティ)。

### コミット可能条件
上記ゲート全通過(verify.ps1 + src-tauri cargo test 両方)+ impl-reviewer(非実装者)承認 + `feature/workspace-management`。コミット本文必須(Tauri permission / FS 所有 / パス検証に該当)。

---

## Phase B — オフライン manifest 直読み + Asset Browser / Asset Inspector UI

### 目的・期待される挙動変化
Phase A のワークスペース内 manifest.json を Rust backend が直接パースし、Asset Browser(一覧)と Asset Inspector(個別詳細)に表示。**未接続・Bridge 非依存・NorvesLib 改修ゼロ**で一覧→詳細が動く。健全性カラムはこの段階で「未接続=未検証(unknown)」。

### 触るファイル
- 新規 Rust: `apps/editor/src-tauri/src/asset_manifest.rs`(コマンド `asset_read_workspace_manifest` → manifest.json を `std::fs` で読み、snake_case → camelCase の `AssetEntryDto` 配列へ**値コピー DTO 変換**。ファイル由来静的データのため engine live memory 非該当=memory-buffer-policy 射程外)
- `lib.rs`/`protocol_names.rs`(`ASSET_READ_WORKSPACE_MANIFEST` 登録 + assertion)
- `dto.rs`(`AssetEntryDto`、`AssetManifestPayload { version, entries }`、camelCase。**D3 安全装置: cooked 由来フィールド(`cookedHashHex`/`cookedPackage`/`entryName`/`entryType`/`cookedVersion`)は全て `Option`** にし、loose 追加を additive に保つ)
- TS: `commands.ts`/`ipc-types.ts`(`AssetManifestPayload`/`AssetEntry`、cooked フィールド optional)
- 新規 UI: `apps/editor/src/components/AssetBrowserPanel.tsx`(一覧: logicalPath/kind/variant + 健全性カラム placeholder)、`apps/editor/src/components/AssetInspectorPanel.tsx`(個別詳細: 全フィールド + hash + cooked/loose 情報)
- `AppLayout.tsx:71-97`(`PANEL_ASSET_BROWSER`/`PANEL_ASSET_INSPECTOR` 定数 + `PANEL_COMPONENTS` 登録 + 配置。選択は `selectedObjectId` と別軸の `selectedAssetKey`(logicalPath+variant)を新設)
- `store.ts`(`assetManifest?`、`selectedAssetKey?`、`assetManifestLoaded`/`assetSelected` action + reducer + INITIAL_STATE)
- `useBridge.ts`(`readWorkspaceManifest()`/`selectAsset(key)`)
- テスト: Rust(snake→camel 変換、cooked 欠落エントリ=loose 想定、不正フィールド、`version!=1` 拒否、parse 失敗の理由コード)、store/UI(一覧描画・選択・空/未ワークスペース/parse 失敗の各状態)

### 具体的ステップ
1. manifest.json 発見規則(D3 確定 or 安全装置): 確定すれば固定パス。未確定なら `<root>/Cooked/manifest.json` をデフォルト + 設定上書き、かつ DTO の cooked フィールドを全 optional 化して loose 追加を additive に保つ。
2. Rust で manifest.json をパースし `version==1` を検証(NorvesLib の `UnsupportedVersion` 規則を参照)。snake→camel は値コピー構築。
3. parse 失敗時は**理由つき構造化エラー**(NorvesLib の `AssetManifestParseStatus` に対応する最小理由コード。24種網羅はスコープ外、OQ)。
4. Asset Browser/Inspector を dock パネル登録(D=dock 採用、OQ6 はパネル配置として dock デフォルト)。選択は logicalPath+variant。
5. 健全性カラムは Phase B では `unknown` 固定。Phase D で live オーバーレイ。

### 新規プロトコル schema+fixture
**なし。** オフライン直読みは Rust backend のローカル FS 操作、wire protocol 非経由。

### 所有権 / 寿命 / スレッド / async / permission
- manifest.json はファイル由来静的データ。値コピー DTO 化(engine live memory 非該当)。
- alpha は**毎回読む**(キャッシュせず、再読込ボタンで明示リフレッシュ)=単純。
- permission: カスタムコマンド完結、fs プラグイン不要。

### protocol schema/fixture 変更の有無
なし。

### base / ブランチ / stacked
base: `feature/workspace-management` マージ済みの `main`、work: `feature/asset-browser-offline`。**Phase A 依存(stacked: A マージ後着手)**。

### 検証コマンドと期待結果
```powershell
node scripts/check-protocol-names.mjs
cd apps/editor/src-tauri; cargo fmt --all -- --check; cargo clippy --workspace --all-targets -- -D warnings; cargo test  # asset_manifest.rs パース/version 検証緑(verify.ps1 では走らない)
pnpm -r --if-present typecheck
cd apps/editor; pnpm test
./scripts/verify.ps1                                   # fixtures 124 不変 + ルートワークスペース Rust
```
**役割分担**: `asset_manifest.rs` テストは src-tauri ワークスペースのため verify.ps1 では走らない。`cd apps/editor/src-tauri; cargo test` を必ず実行。
期待: 全緑、fixture カウント **124 不変**。

### リスクと封じ込め
- *manifest.json の場所・loose 列挙が規約未定*(中): D3 確定 + cooked フィールド全 optional 化で additive 吸収。
- *スケール(数千エントリ)*(中): オフライン一覧は全件メモリ保持。alpha は数百〜数千で許容、仮想スクロール無し単純リスト(将来 GAP)。**この段階ではページング不要**(肥大化リスクは live のみ)。
- *parse 失敗の可視化深掘り*(スコープ外): 理由コード最小限。

### 担当
実装=Codex。レビュー=Claude+Codex 二重。**Opus=任意**(FS パース + DTO 変換は中リスク。version 検証/parse 失敗処理のみ慎重レビュー)。

### コミット可能条件
ゲート全通過(verify.ps1 + src-tauri cargo test)+ impl-reviewer 承認 + `feature/asset-browser-offline`。コミット本文推奨。

---

## Phase C — Bridge プロトコル拡張(`asset.resolve` [+ 任意 `asset.getManifest`])

### 目的・期待される挙動変化
汎用 Bridge protocol に**読み取り系メソッドを additive 追加**。**コア = `asset.resolve`**(単一論理パスの健全性確認)。**任意 = `asset.getManifest`**(エンジンがロード中の manifest 列挙 snapshot、**D1 で MVP 採否を決定**)。schema + fixtures + 汎用 SDK virtual default(`not_supported`) + Rust コマンド/validator + TS 型/wrapper を縦に配線。この段階では mock も NorvesLib も実装せず、汎用契約のみ確定。本計画で最もプロトコル load-bearing。

### 触るファイル
**コア(`asset.resolve`、常に実施)**:
- schema 新規(additive):
  - `bridge/spec/schema/methods/asset.resolve.params.schema.json`(`{ logicalPath(required), kind?, variant? }`)
  - `bridge/spec/schema/methods/asset.resolve.result.schema.json`(`{ status(enum 9値=0.4表), source(enum 4値), normalizedLogicalPath, requiresExplicitLog?, fallbackAction?, failureKind?, reason? }`、`additionalProperties:false`)
- `bridge/spec/schema/common.schema.json:6-`(後述 `asset.getManifest` を含める場合のみ `$defs.assetEntry` 追加。`asset.resolve` 単体なら common 変更は status/source enum を inline でも可)
- fixtures 新規(envelope version "0.2"): `asset.resolve` の positive `request-valid.json`/`response-valid.json` + **negative(下記方針)**
- C++ SDK: `adapter.hpp`(`assetResolve` を**非純粋 + `not_supported(params)` デフォルト**で追加、`:99-132` のオプションパターン踏襲)、`server.cpp:225-241` 付近(`if (method == "asset.resolve")` 分岐)、`dispatch_test.cpp`(デフォルト `METHOD_NOT_SUPPORTED` テスト、既存 `TestUnimplementedOptionalMethodIsMethodNotSupported` 踏襲)
- Rust editor-client: `bridge/crates/norves-bridge-editor-client/src/asset.rs`(`parse_asset_resolve_result`、`scene.rs:63` 踏襲)+ `lib.rs:29-46` の `pub use`
- Rust backend: `bridge_state.rs`(`asset_resolve(logical_path, kind?, variant?)` コマンド、validate-then-forward。`scene_get_tree`:604 手本)、`lib.rs`/`protocol_names.rs`(`ASSET_RESOLVE` + assertion)
- TS: `bridge-types/src/asset.ts`(`AssetResolveResult` + index export)、`commands.ts`(`assetResolve`)、`ipc-types.ts`
- docs: `bridge/spec/docs/capabilities.md`(**`asset.read` token 追記**、D2 確定文字列)、`protocol-overview.md`(Methods 追記)、`message-payloads.md`(payload 表)
- fixture カウント更新(BLOCKER、後述)

**任意ブロック(`asset.getManifest`、D1 で「含める」と決定した場合のみ)**:
- `bridge/spec/schema/methods/asset.getManifest.params.schema.json`(`{ filter?, page?, pageSize? }` — スケール対策のページング/フィルタを最初から)
- `bridge/spec/schema/methods/asset.getManifest.result.schema.json`(`{ version, entries: assetEntry[], totalCount, page?, pageSize? }`)
- `common.schema.json` に `$defs.assetEntry`(`logicalPath`(required)/`kind`/`variant`/`format`/`sourceHashHex`/`cookedHashHex`/`cookedPackage`/`entryName`/`entryType`/`cookedVersion`、`additionalProperties:false`)
- 上記 fixtures + adapter virtual(`assetGetManifest`)+ server dispatch + Rust parse(`parse_asset_manifest_result`)+ コマンド(`asset_get_manifest`)+ TS 型/wrapper
- **D1 で「不要」と決定した場合、この任意ブロックを全て落とし protocol サーフェスを半減**(後付けは additive で可能)。

### negative fixture の方針(検証ギャップ充足)
positive のみでなく、契約逸脱を固定するため negative を追加する。層と連動カウントを明示:
- **payload negative**(envelope は valid、payload が不正): `asset.resolve.params` で `logicalPath` 欠落、`asset.resolve.result` で status に enum 外値、`assetEntry`(getManifest 採用時)で `additionalProperties:false` 違反。→ **PayloadOnly カテゴリ**に分類され、Rust `:129`(分類)/`:130-133`(total)/`:221`(accept 件数)と C++ 該当カウントを更新。
- **envelope negative は追加しない**(asset 固有の envelope 不正は既存 envelope テストで十分カバー、新規追加の価値が薄い)。よって `:126`/`:195` は不変。
- positive 追加分は `:124`/`:130-133`/`:175` + C++ `:192`/`:195` を更新。

### fixture カウント更新(BLOCKER、カテゴリ別に一般化)
追加する fixture のカテゴリに応じて、**該当する全カウント assert を同一コミットで更新**:
- **positive を N 件追加** → Rust `:124`(positive)/`:130-133`(total)/`:175`(roundtrip 件数)、C++ `fixtures_roundtrip_test.cpp:192`(positive)/`:195`(total)
- **payload negative を M 件追加** → Rust `:129`(payload_only)/`:130-133`(total)/`:221`(accept 件数)、C++ 該当 payload-only + total assert
- **envelope negative を追加する場合**(本計画では非追加) → Rust `:126`/`:130-133`/`:195`
漏れると両 workspace test が即失敗(=自動検出)。

### 具体的ステップ
1. `asset.resolve` の result enum を**0.4 節の9 status + 4 source 完全 superset**で schema 確定(NorvesLib 全9 status を漏れなく写像可能に)。`requiresExplicitLog`/`fallbackAction`/`failureKind`/`reason` を健全性フィールドとして定義。
2. schema → positive + payload negative fixtures → カテゴリ別カウント assert を C++/Rust 同時更新。
3. C++ SDK に `assetResolve` virtual default + server dispatch 分岐 + デフォルト METHOD_NOT_SUPPORTED テスト。**mock は実装しない**(ユーザー制約)。
4. Rust validator(`asset.rs`)→ backend コマンド(validate-then-forward)→ IPC名3点 → TS 型/wrapper。
5. **conformance runner は変更しない**(mock が asset 非実装のため、asset ステップを足すと METHOD_NOT_SUPPORTED で exact-match 不能)。conformance 不変を明記。
6. (D1 で getManifest 採用時のみ)任意ブロックを 1-5 と同様に追加。

### 新規プロトコル schema+fixture
**あり(本計画唯一の protocol 変更点)**: `asset.resolve` の params/result schema + positive/payload-negative fixtures(+ D1 採用時 `asset.getManifest` schema + `assetEntry` $def + fixtures)。version bump なし(0.2 additive)。

### 所有権 / 寿命 / スレッド / async / permission
- backend コマンドは `send_method` 経由で no-lock-across-await。
- engine live memory はトランスポート非送出: backend は validate-then-forward で wire Value 素通し。SDK 側 adapter は値コピーで JsonValue 構築する契約(NorvesLib 実装は Phase E)。
- permission 変更なし。

### protocol schema/fixture 変更の有無
**あり。** additive(0.2)。schema version pattern 不変。conformance runner 不変。

### base / ブランチ / stacked
base: `main`、work: `feature/protocol-asset-methods`。Phase A/B と独立着手可。**Phase D/E の前提**。

### 検証コマンドと期待結果
```powershell
python scripts/validate-bridge-fixtures.py            # "OK: <N> fixture(s) validated"(positive + payload-negative 増加分)
ctest --test-dir build/cpp -C Debug --output-on-failure # C++ fixture カウント + asset.resolve default METHOD_NOT_SUPPORTED テスト緑
node scripts/check-protocol-names.mjs
cargo fmt --all -- --check; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace  # ルートワークスペース: Rust fixture カウント + asset.rs parse 緑
cd apps/editor/src-tauri; cargo fmt --all -- --check; cargo clippy --workspace --all-targets -- -D warnings; cargo test  # src-tauri: asset_resolve コマンド緑
pnpm -r --if-present typecheck
./scripts/verify.ps1 -Cpp                              # fixtures + ルート Rust + C++(src-tauri は別途上記)
```
期待: fixture カウントが C++/Rust で一致して新値へ、全テスト緑。conformance(`NORVES_MOCK_ENGINE` 駆動)は asset ステップ無しのまま緑。

### リスクと封じ込め
- *fixture カウント二重ハードコード*(BLOCKER): カテゴリ別の全 assert(上記)を同一コミットで更新。漏れは即失敗で自動検出。
- *resolve enum の網羅漏れ*(major・レビュー指摘起因): 0.4 節の9 status 完全 superset を schema に固定。Phase E でマップ不能 status が出ないことを担保。impl-review で9値全列挙を確認。
- *スケール(getManifest 採用時の列挙肥大)*(中): schema に `filter`/`page`/`pageSize` + `totalCount` を最初から。alpha の Rust backend は page 素通し、実ページングは NorvesLib(Phase E)。
- *enum 将来拡張*(低): 汎用文字列 enum 値追加は additive。
- *conformance を壊さない*: mock に asset を足さない。runner 不変を明記。

### 担当
実装=Codex。レビュー=Claude+Codex 二重。**Opus エスカレーション=必須**(protocol schema/compatibility + C++ SDK public API + fixture 整合 = 最高 load-bearing)。

### コミット可能条件
ゲート全通過 + impl-reviewer 承認 + `feature/protocol-asset-methods`。**コミット本文必須**(protocol schema/fixtures + Bridge public API)。

---

## Phase D — Bridge live 経路の UI 統合(健全性オーバーレイ)

### 目的・期待される挙動変化
接続中のエンジンに `asset.resolve`(健全性)[+ D1 採用時 `asset.getManifest`(補助)]を呼び、Phase B のオフライン一覧に**健全性をオーバーレイ**。未接続=未検証、接続中=cooked/loose/hash mismatch 等を Asset Browser 健全性カラム + Asset Inspector に表示。`METHOD_NOT_SUPPORTED`/`asset.read` token 無しのエンジンは graceful degradation(健全性「この engine は asset 照会に未対応」)。

### 触るファイル
- `useBridge.ts`(`resolveAsset(key)` [+ 採用時 `getEngineManifest()`]。`getSceneTree`:421 / `isMethodNotSupported`:89 踏襲。token/METHOD_NOT_SUPPORTED で degrade)
- `store.ts`(`assetResolveByKey?: Record<string, AssetResolveResult>`、`assetCapabilitySupported?`、[採用時 `assetManifestLive?`]、対応 action + reducer)
- `AssetBrowserPanel.tsx`/`AssetInspectorPanel.tsx`(健全性カラム/詳細を live データで描画。未接続=unknown、未対応=notice、接続中=status)
- capability 判定: `bridge.getCapabilities` 結果から `asset.read` token 有無を読む配線(既存 capability 取得経路を再利用、無ければ最小追加)
- テスト: store(オーバーレイ反映)、UI(未接続/未対応/cooked成功/hash mismatch 各表示)、**レース/点滅テスト(下記)**

### 具体的ステップ
1. **source-of-truth 維持**: 一覧 primary はオフライン manifest(Phase B)。live は `asset.resolve` の status を `logicalPath+variant` で突き合わせ、健全性カラムにマージ。**一覧の行集合は live で置き換えない**。
2. 健全性フェッチ戦略: 全件 resolve は O(n) で重い → **選択行 + 表示中の行のみ resolve**(遅延・オンデマンド)。スケール対策。
3. `asset.read` token 無し/`METHOD_NOT_SUPPORTED` は「健全性 未対応」で degrade。
4. (採用時)live `asset.getManifest` は「engine ロード中 manifest がオフラインと異なる」検出補助に留め、primary 表示は変えない。

### レース/点滅の検証ケース(検証ギャップ充足)
`object snapshot` のレースガードに倣い、以下を具体テストで固定:
- **選択切替の最新反映**: asset A 選択 → resolve in-flight 中に asset B 選択 → A の遅延レスポンスが届いても B の健全性のみ反映(stale 破棄)。選択キーを effect deps にし、レスポンスのキーと現選択キーの一致を検査。
- **接続/切断で一覧不点滅**: オフライン manifest が primary のため、接続 → 切断で行集合が不変、健全性カラムが unknown ↔ status に変わるのみ。行が消えない/再生成されないことを store 遷移テストで確認。
- **タイムアウト境界**: `bridge_state.rs` の `REQUEST_TIMEOUT`(5秒)で resolve がタイムアウトした場合、健全性は「未確定(timeout)」表示にフォールバックし、行は維持。

### 新規プロトコル schema+fixture
なし(Phase C 確定契約を消費)。

### 所有権 / 寿命 / スレッド / async / permission
- live 健全性は UI state(store)保持。engine live memory ではなく wire DTO のコピー。
- オンデマンド resolve のレース: 選択キーを deps にした effect で最新のみ反映(上記検証ケース)。
- permission 変更なし。

### protocol schema/fixture 変更の有無
なし。

### base / ブランチ / stacked
base: Phase B + Phase C マージ済みの `main`、work: `feature/asset-live-overlay`。**Phase B と C 両方に依存(stacked)**。

### 検証コマンドと期待結果
```powershell
pnpm -r --if-present typecheck
cd apps/editor; pnpm test                              # 健全性オーバーレイ各状態 + レース/点滅/timeout テスト緑
./scripts/verify.ps1                                   # fixtures 124 不変 + ルート Rust
# 実機 live 確認は Phase E の NorvesLib 実装が前提(受け入れは E 後)
```
期待: TS 全緑。**実 live ラウンドトリップ受け入れは Phase E 完了後**(mock が asset 非対応のため、D 単体では「未対応 degrade」までしか実機確認できない — これを受け入れ基準に明記)。

### リスクと封じ込め
- *D 単体で live を実機確認できない*(構造的): mock は asset 非実装(ユーザー制約)。D の自動テストは「degrade 経路」+ TS unit + レース/点滅のみ。cooked/loose 実表示確認は E 後の手動受け入れ(明記)。
- *全件 resolve の肥大*(中): オンデマンド resolve(選択+表示行のみ)で封じ込め。
- *オフライン/live 突き合わせズレ*(中): キーは `logicalPath+variant` 固定(同一性契約と整合)。

### 担当
実装=Codex。レビュー=Claude+Codex 二重。**Opus=任意**(フロント中心。capability degrade + レースガード設計のみ慎重レビュー)。

### コミット可能条件
ゲート全通過 + impl-reviewer 承認 + `feature/asset-live-overlay`。

---

## Phase E — 【クロスリポ・別リポ NorvesLib】列挙API露出 + アダプタ override + capability token

> **このフェーズは別リポジトリ `C:\Users\KINGkawamura\Documents\NorvesLib` の作業。** NorvesEditor リポには一切コミットしない。NorvesLib の `AGENTS.md`/`CLAUDE.md` の規約・ポインタ整列(west 採用: ユーザー指示メモリ)・EOL flip ハザード(CRLF/LF)に従う。NorvesLib は別 PR/別ブランチで進める。**NorvesLib には作業ツリー(`.claude/worktrees/*`)が複数存在しうるため、着手時は main ツリーの最新を base にし、worktree の途中状態を誤って base にしない。**

### 目的・期待される挙動変化
NorvesLib の `NorvesLibBridgeAdapter` に `assetResolve` [+ D1 採用時 `assetGetManifest`] を override し、Phase C 確定の汎用 wire 契約に準拠。これに必要な**列挙アクセス経路を `AssetSystem` ファサードに新設**し、`asset.read` capability token を広告。これで NorvesEditor が NorvesLib 接続時に live アセット健全性が実動する。

### 触るファイル(別リポ NorvesLib)
- `Library/Core/Public/Asset/AssetSystem.h`(列挙アクセサ追加: `GetAssetCount()`/`GetAssetReference(index)` を **`AssetSystem` ファサード経由で露出**。現状 `m_Manifest` private で到達不能。`AssetManifest`(`AssetManifest.h:138-139`)へ委譲する薄いラッパ)
- `Library/Core/Private/Asset/AssetSystem.cpp`(実装)
- `Game/Bridge/NorvesLibBridgeAdapter.h:197-260` 付近(`assetResolve` [+ 採用時 `assetGetManifest`] override 宣言、scene/object の隣)
- `Game/Bridge/NorvesLibBridgeAdapter.cpp`:
  - `assetResolve` 実装: `AssetSystem::ResolveAsset` を呼び、**`AssetResolveStatus` 全9値 + `AssetResolveSource` 全4値**を 0.4 節の wire enum 文字列へマップ。`RequiresExplicitLog`/`FallbackDecision`/`Reason` を健全性として返す。**`AssetResolveResult.Blob`/`Entry`/`LoosePath` は wire に乗せない**(バイト列/ポインタ非送出)
  - (採用時)`assetGetManifest` 実装: 列挙アクセサで全 reference 走査 → 汎用 `assetEntry` 形へ**値コピー**で JsonValue 構築 + `page`/`pageSize`/`filter`/`totalCount` 実装(O(n) 全走査を page スライス)
  - `:1120-1126` の capability 配列に `{"name":"asset.read"}` 追加(現状6→7 token、D2 確定文字列)
- NorvesLib テスト: `Test/Core/Asset/` に列挙アクセサテスト、`Game/Bridge` 配下にアダプタ override テスト(NorvesLib テスト規約に従う)

### 「楽観視できない注意点」への対処(明記)
- **列挙API欠如**: `AssetSystem` に列挙アクセサを**新規追加**して解消(「DTO化済み=実装ゼロ」ではない裏取りに対応、本フェーズの中核)。`AssetManifest` の public メソッドは既存だがファサード越し到達不能だった点を薄いラッパで橋渡し。
- **capability token の出所取り違え**: 追加先は **NorvesLib `NorvesLibBridgeAdapter.cpp:1120-1126`(6 token)**、NorvesEditor の mock(8 token)ではない。mock は触らない。token 文字列は D2 で固定した `asset.read` を両リポで同一使用。
- **resolve enum 網羅**: Phase C で確定した9 status 完全 superset を全マップ(`LooseReadFailed`/`CookedPackageParseFailed` 含む)。マップ漏れがないことをテストで確認。
- **スケール O(n)**: ページング(`page`/`pageSize`)を NorvesLib 側で実装し `totalCount` 返却。

### 具体的ステップ
1. `AssetSystem` に `[[nodiscard]] size_t GetAssetCount() const` / `const AssetCookedReference& GetAssetReference(size_t) const`(west ポインタ整列)を追加(`m_Manifest` の public メソッドへ委譲)。**スレッド契約**: `AssetSystem.h:13-18` の「manifest mutation 中の同時 Resolve 非対応」に整合させ、列挙も mutation 中は呼ばない契約を明記。
2. (採用時)ページング: Phase C の `page`/`pageSize`/`filter`/`totalCount` を NorvesLib で実装。
3. `assetResolve` override: `AssetResolveStatus`(全9)/`AssetResolveSource`(全4)/`AssetFallbackDecision` を 0.4 節 wire enum へマップ。`RequiresExplicitLog`/`FailureKind`/`Reason` を健全性として返す。
4. capability `asset.read` 追加。**superset 方針**(`NorvesLibBridgeAdapter.cpp:1117` コメント方針)に整合: 実装済み token のみ広告。

### NorvesLib 側テストケース(検証ギャップ充足)
- 列挙アクセサ: 空 manifest(count=0)、複数エントリの index アクセス、範囲外 index の契約(assert or 例外、NorvesLib 規約に従う)。
- `assetResolve` override の status マッピング網羅: 9 status それぞれが対応 wire 文字列になることを検証(`SuccessCooked`→`successCooked` … `CookedPackageParseFailed`→`cookedPackageParseFailed`)、4 source 同様。`InvalidManifest`(version!=1 等)・`CookedEntryHashMismatch`・`LooseReadFailed` の各経路を再現。
- (採用時)`assetGetManifest`: 空 manifest、page スライス境界(page=0/末尾 page/`pageSize` 超過)、`filter` 適用、`totalCount` 一致、値コピー(ポインタ非送出)。
- アダプタが `asset.read` token を広告(7 token になる)。

### 新規プロトコル schema+fixture
**なし(NorvesEditor 側)。** Phase C 確定契約に NorvesLib が準拠するのみ。NorvesLib 側に必要ならそのテスト fixture を追加(別リポ規約)。

### 所有権 / 寿命 / スレッド / async / permission
- `assetResolve` [+ 採用時 `assetGetManifest`] は `IBridgeEngineAdapter` のスレッドアフィニティ契約(`adapter.hpp:22-34`: handleFrame と同一スレッド同期呼び出し、live memory を JsonValue に入れない)を厳守。manifest mutation 中に列挙しない。
- 全 reference を**値コピー**で JsonValue 化(ポインタ/span/blob 非送出)。`AssetResolveResult.Blob`/`Entry`/`LoosePath` は健全性表示に不要なので wire 非送出。

### protocol schema/fixture 変更の有無
NorvesEditor 側なし。NorvesLib 側のテスト整備は別リポ。

### base / ブランチ / stacked
別リポ NorvesLib の base(main ツリー最新)/work ブランチ(例: `feature/bridge-asset-enumeration`)。NorvesEditor の Phase C(wire 契約)に依存。NorvesEditor リポにはブランチを作らない。

### 検証コマンドと期待結果(NorvesLib 側)
```powershell
# NorvesLib リポで(AGENTS.md のゲート規約に従う。cmake configure 後):
#   cmake 構成例(NorvesLib の規約を着手時に AGENTS.md で確認):
#   cmake -S . -B build -G "<generator>" -DNORVES_BUILD_TESTS=ON   # 実フラグは AGENTS.md 参照
#   cmake --build build --config Debug
ctest --test-dir build -C Debug --output-on-failure    # 列挙アクセサ + アダプタ override(9 status/4 source マッピング・空 manifest・ページング境界)テスト緑
# クロスリポ結合確認(手動受け入れ): NorvesEditor から NorvesLib エンジンを launch/connect し、
#   Asset Browser 健全性カラムが cooked/loose/hash mismatch を表示(Phase D の live 経路)
```
**注**: 上記 cmake コマンドは雛形。実際のフラグ/generator/test 名は着手時に NorvesLib の `AGENTS.md` の検証ゲートを引いて確定する(本計画は別リポ規約に従う)。
期待: NorvesLib ビルド/テスト緑。NorvesEditor 接続で Phase D 健全性オーバーレイが実データで動く(手動受け入れ)。

### リスクと封じ込め
- *EOL flip ハザード(CRLF→LF 全面 flip 再発)*(高・メモリ既知): numstat 検出 + difflib 再構築で修復(ユーザーメモリ運用)。差分は最小行に限定。
- *worktree 取り違え*(中・レビュー指摘起因): main ツリー最新を base にし、`.claude/worktrees/*` の途中状態を base にしない。
- *ポインタ整列*(低): NorvesLib 実コードは east 多数派だがユーザー指示は west 採用(メモリ)。新規コードは west。
- *generic bridge への NorvesLib 漏れ*(境界): 本作業は NorvesLib リポ内に閉じる。NorvesEditor の汎用 SDK/Rust/TS に NorvesLib 由来コードを入れない(Phase C の汎用契約のみ消費)。
- *manifest mutation 中の列挙レース*(中): スレッド契約で禁止を明記、Bridge は単一 recv スレッド前提。

### 担当
実装=Codex(別リポ NorvesLib)。レビュー=Claude+Codex 二重。**Opus エスカレーション=必須**(NorvesLib adapter + C++ public API(AssetSystem ファサード)+ buffer/memory ownership)。

### コミット可能条件
NorvesLib 側ゲート全通過 + impl-reviewer 承認 + 別リポ work ブランチ + クロスリポ手動受け入れ。コミット本文必須(NorvesLib adapter + 公開 API + ownership)。

---

## 今回スコープ外として明示的に切る項目

- **サムネイル(`asset.getThumbnail`)**: PNG エンコーダ未導入で重いため後回し(ユーザー確定)。`viewport.getThumbnail` は別概念で既存。
- **参照ナビ(object ↔ asset 相互ジャンプ)**: 将来項目。アセットを参照するシーンオブジェクトへの逆引きは alpha では出さない。
- **パース失敗の深掘り可視化**: `AssetManifestParseStatus`(8種)/`CookedTextureParseStatus`(24種)の詳細表示。Phase B/C は最小理由コードに留め、網羅 UI は将来。
- **シーン構造編集 / アセットの追加・削除・cook 操作**: 本計画は**閲覧 + 健全性表示のみ**。書き込みはスコープ外。
- **undo / redo**: alpha スコープ外。
- **OS ネイティブのフォルダ選択ダイアログ(`tauri-plugin-dialog`)**: Phase A はパス入力で代替(D4 確定)。
- **mock engine への asset 対応追加**: ユーザー制約により実施しない。conformance runner も asset ステップ非追加。
- **アセット一覧の仮想スクロール / 大規模最適化**: UI 仮想化は将来 GAP(protocol 側ページングは D1 採用時のみ先行導入)。

## 残存 openQuestions(承認時に user 確認、上記 D1-D4 で主要分は昇格済み)

1. **D1(=旧OQ7)**: live `asset.getManifest` を MVP に含めるか。**未確定なら `asset.resolve` のみで着手し、後から additive 追加可**(本計画の推奨)。
2. **D3(=旧OQ2/OQ3)**: manifest.json 探索規約 と loose アセット列挙の有無。**未確定でも cooked フィールド全 optional 化で Phase B 着手可**。
3. **`asset.resolve` の健全性粒度**(旧OQ4): alpha で表示する status の範囲(全9 status か絞るか)。schema は全9 superset で固定するが、UI 表示の絞り込みは別途。
4. **パネル配置**(旧OQ6): dock パネル(本計画デフォルト)か別 Tauri window か。

> D2(token=`asset.read`)・D4(パス入力)は本計画で確定済み。

## 主要参照ファイル(絶対パス)
- `C:\Users\KINGkawamura\Documents\NorvesEditor\apps\editor\src-tauri\src\bridge_state.rs`(コマンド/validate-then-forward/no-lock-across-await/REQUEST_TIMEOUT)
- `C:\Users\KINGkawamura\Documents\NorvesEditor\apps\editor\src-tauri\src\dto.rs` / `error.rs` / `lib.rs` / `protocol_names.rs`
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\crates\norves-bridge-editor-client\src\scene.rs`(parse 手本)/ `lib.rs`(pub use)
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\cpp\engine-sdk\include\norves\bridge\adapter.hpp`(virtual/not_supported パターン)
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\cpp\engine-sdk\src\server.cpp`(dispatch ルーティング)
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\cpp\engine-sdk\tests\fixtures_roundtrip_test.cpp`(C++ count :192-195)
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\crates\norves-bridge-core\tests\fixtures_roundtrip.rs`(Rust count :124/:126/:129/:130-133/:175/:195/:221)
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\cpp\examples\mock-engine\mock_adapter.hpp`(8 token・触らない)
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\conformance\runners\alpha_method_sequence.json`(getCapabilities exact-match・不変)
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\spec\schema\common.schema.json`($defs 追加先)
- `C:\Users\KINGkawamura\Documents\NorvesEditor\apps\editor\src\hooks\useBridge.ts` / `state\store.ts` / `components\AppLayout.tsx`(UI 配線)
- `C:\Users\KINGkawamura\Documents\NorvesEditor\scripts\verify.ps1`(:51,:64 = ルートワークスペースのみ・src-tauri 非対象)
- `C:\Users\KINGkawamura\Documents\NorvesLib\Library\Core\Public\Asset\AssetSystem.h`(列挙アクセサ欠如・private m_Manifest)
- `C:\Users\KINGkawamura\Documents\NorvesLib\Library\Core\Public\Asset\AssetManifest.h`(GetReferenceCount/GetReference :138-139)
- `C:\Users\KINGkawamura\Documents\NorvesLib\Library\Core\Private\Asset\AssetPath.cpp`(:192-235 実挙動=絶対/`..` 受理・UNC/drive-relative のみ拒否 = 移植ではなく参照)
- `C:\Users\KINGkawamura\Documents\NorvesLib\Library\Core\Public\Asset\AssetResolveResult.h`(:14-25 status 9値 / :27-33 source 4値)
- `C:\Users\KINGkawamura\Documents\NorvesLib\Game\Bridge\NorvesLibBridgeAdapter.cpp`(capability :1120-1126・6 token)/ `.h`(override 宣言箇所)
