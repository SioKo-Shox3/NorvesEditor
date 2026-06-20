# NorvesEditor UI シェル刷新 指示書（次フェーズ / UI overhaul brief）

> これは**要件指示書**であり、確定実装計画ではない。次セッションは CLAUDE.md の
> オーケストレーション規約（research → plan → plan-review → **ユーザー承認** →
> implement → impl-review → verify → commit）に従い、本書を入力として
> **自分で phase plan を作成し、ユーザー承認を得てから実装**すること。
> 本書の「オープンクエスチョン」は plan 提示時にユーザーへ確認する。

## 1. 背景・目的

Phase 0–7 で Outliner / Inspector / Viewport(サムネイル) / ライブ更新 / protocol 0.2 が
実装され、Editor を単体起動できるようになった。実際に起動したところ、UX/見た目に複数の
不満が判明したため、**エディタの「シェル」（ウィンドウ枠・ツールバー・パネル配置・
マルチウィンドウ）を Unreal Engine ライクに刷新する**。

機能（Bridge 経由のデータ取得・編集）は完成しているので、本フェーズは原則
**フロントエンド + Tauri ウィンドウ/権限設定のみ**。プロトコル・C++ SDK・Bridge の
振る舞いは変更しない（後述の制約）。

## 2. 現状（実装済み・変更前）

- 単一ウィンドウ。`apps/editor/src-tauri/tauri.conf.json` の window は
  `decorations` 既定（= OS 標準タイトルバーが出る）、1280x800。
- レイアウトは `apps/editor/src/components/AppLayout.tsx` の **dockview-react**。
  既定レイアウト = 左:Scene Outliner / 中央:Game View / 右:Connection・Settings・
  Property Inspector / 下:Log。`disableFloatingGroups: true`。
  localStorage キー `norveseditor-layout-v1` に永続化 + 破損時 purge + リセットボタン
  （SettingsPanel）。
- パネルは全て常時表示。**ツールバー無し**。Game View が中央だが大きくない。
- Tauri capability（`apps/editor/src-tauri/capabilities/default.json`）は
  `windows: ["main"]`、`permissions: ["core:default", "core:event:default"]`。
  → **ウィンドウ操作（最小化/最大化/閉じる/ドラッグ）や複数ウィンドウ生成の権限は未付与**。
- 各パネルは `useBridgeState()` / `useBridgeActions()`（`hooks/useBridge.ts`）から
  状態とコマンドを取得（Phase 1 で props drilling 撤廃済み）。Rust backend が Bridge
  接続を所有し、イベントを全ウィンドウへ relay できる構造（マルチウィンドウの土台）。

## 3. 要件（ユーザー指示）

### R1. カスタムタイトルバー（OS 既定をやめ、画面と一体のデザインに）
- `decorations: false`（または overlay 方式）にし、**アプリのテーマに合わせた独自タイトルバー**を実装。
- タイトル/アプリ名・ウィンドウ操作（最小化/最大化・復元/閉じる）を独自ボタンで提供。
- ドラッグ移動領域（`data-tauri-drag-region`）。ダブルクリックで最大化トグル等の標準挙動も。

### R2. ツールバー（UE ライク）
- タイトルバー直下に**メインツールバー**を設置。主要アクションを集約:
  - エンジン制御: Launch / Stop Process / Reconnect、Play / Pause / Stop、Focus Viewport
  - 接続状態の簡易インジケータ（Connection ウィンドウを開くボタン）
  - 表示トグル（Log の展開、各ウィンドウ/パネルの表示）、レイアウトのリセット
  - Settings ウィンドウを開くボタン
- 現状これらは GameViewPanel / ConnectionPanel に散在 → **ツールバーへ集約**し直す
  （アクションは `useBridgeActions()` 経由のまま。engine-agnostic を維持）。

### R3. レイアウト再設計（画面要素の取捨選択とデザイン）
- **Game View が主役**。中央かつ**最大**。最初に目に入る最大領域にする。
- **Scene Outliner は画面右**（UE ライク。現状の左から右へ）。
- **Property Inspector は右**（Outliner の下など）。かつ**アウトライナーで
  オブジェクトを選択した時だけ表示**（未選択時は出さない/最小化。`selectedObjectId`
  駆動で表示制御。Phase 1 の選択モデルを利用）。
- **Log は常時表示しない**。画面下に格納し、**マウスホバーでポップアップ**、または
  **任意でウィンドウ/ドックに展開**できる形に。
- **Connection は別ウィンドウ**（常時は出さない。セットアップ/再接続時に開く）。
- **Settings は常時表示しない**（滅多に触らない）。別ウィンドウ or メニューから。
- 「一画面に全部詰める」のをやめ、**画面に映す要素のデザインを練り直す**。

### R4. マルチウィンドウ（UE ライク）
- UE のように**複数ウィンドウを作れる仕組み**。パネルを別ウィンドウに切り出せる
  （tear-off / 別 OS ウィンドウ）。最低限 Connection / Settings は独立ウィンドウ。
- メインウィンドウは Game View 中心の主作業領域に保つ。

### R5. 画面要素のデザイン再考
- 情報過多・視認性の低さを解消。余白・階層・コントラスト・タイポグラフィを整理。
- 既存のダークテーマ/`--color-*` トークン（`styles.css`）と dockview テーマを踏襲しつつ統一感を出す。

## 4. 技術的考慮（plan で具体化すること）

### カスタムタイトルバー
- `tauri.conf.json`: window に `"decorations": false`。
- 独自タイトルバー: `@tauri-apps/api/window` の `getCurrentWindow().minimize() /
  toggleMaximize() / close()`、ドラッグは `data-tauri-drag-region`。
- **Tauri 権限（least privilege）**: capability に `core:window:allow-start-dragging`,
  `core:window:allow-minimize`, `core:window:allow-maximize` /
  `allow-unmaximize` / `allow-toggle-maximize`, `core:window:allow-close` 等を追加。
  tauri-security 規約（`docs/agent-guide/tauri-security.md`）に従い**必要最小限**に絞る。

### マルチウィンドウ（2 案。plan で選択・ユーザー確認）
- **案 A: Tauri WebviewWindow（推奨候補）** — Connection / Settings 等を独立した
  `WebviewWindow`（`@tauri-apps/api/webviewWindow`）として生成。各ウィンドウはアプリの
  ルート（例 `#/connection`）をロードし、**Rust backend が relay する Bridge イベントを
  各ウィンドウが購読**して同期（backend が単一の真実源。state 共有は backend イベント経由で
  成立）。権限: `core:webview:*`（window 生成）+ 新ウィンドウ用の capability エントリ
  （`windows` に追加 or 専用 capability）。
- **案 B: dockview popout groups** — dockview のグループを popout（`window.open` 由来の
  別ウィンドウ）に切り出す。Tauri で `window.open` を許可・橋渡しする設定が要る。
  dockview のモデルに統一できるが Tauri 連携の難度あり。
- どちらでも、**マルチウィンドウ間の状態同期は「backend イベントを各ウィンドウが購読」**で
  成立する点を活かす（各ウィンドウは独自 React/store を持つが、接続は backend が単一所有）。
  二重購読・二重コマンドにならないこと、ウィンドウ破棄時の unlisten を確認。

### レイアウト/dockview
- 既定レイアウト変更に伴い **localStorage キーを bump（`-v1` → `-v2`）**。旧 `-v1` の
  保存レイアウトが新既定を上書きしないようにする（移行時の落とし穴）。リセット導線も更新。
- メインウィンドウ内 tear-off を許すなら `disableFloatingGroups` を見直す。
- Inspector の「選択時のみ表示」は dockview パネルの動的 add/remove か可視制御で実現
  （`selectedObjectId` を監視）。Log の「ホバー/展開」も同様に可視制御 or 別ウィンドウ。

### Viewport を主役にする
- Game View（`GameViewPanel.tsx`）が最大面積になる既定配置。サムネイル表示領域
  （Phase 7、`viewport.getThumbnail`）と viewportState バッジを主役として見やすく。
  ※サムネイル自動更新の恒常エラー時バックオフ（既存フォローアップ task_745bec6c）と整合させる。

## 5. 制約（厳守）

- **engine-agnostic 不変条件**（`docs/phase2-editor-evolution-plan.md` 参照）を維持。
  UI シェルに mock 固有の前提を焼き込まない。
- **Bridge プロトコル / C++ SDK / fixtures / Rust dispatcher の振る舞いは変更しない**。
  本フェーズは frontend + Tauri ウィンドウ/権限（tauri.conf.json + capabilities）に限定。
  新規 Tauri コマンドが必要なら最小限・least privilege で（tauri-security レビュー）。
- UI は raw WebSocket を触らない（従来どおり Tauri command/event 経由）。
- 既存機能（Outliner/Inspector/編集/ライブ更新/サムネイル/接続・プロセス制御）は
  **回帰させない**。既存パネルのデータ取得経路（useBridgeState/useBridgeActions）は維持。
- CLAUDE.md のワークフロー（サブエージェント分業、実装は別エージェント、レビューは
  実装者以外）と検証ゲート（`scripts/verify.ps1`、`pnpm typecheck`/`vitest`、
  該当すれば Rust/C++）を厳守。日本語で報告・コミット。
- `main` 直コミット禁止。テーマ別 feature ブランチ。Tauri 権限/タイトルバー/
  マルチウィンドウに触れるコミットは本文必須 + `Co-Authored-By` トレーラ。
- アクセシビリティ/キーボード操作（タイトルバー・ツールバー・パネル）と、jsdom での
  ウィンドウ API モックを考慮したテストを用意。

## 6. スコープ外（将来）

- 完全なテーマ切替システム（ライト/カスタムテーマ）、フルドッキング（全パネルの
  任意 tear-off + レイアウトプリセット多数）、メニューバーの全面実装は将来。
  本フェーズは「Game View 主役 + 右 Outliner/Inspector + 下 Log 格納 + Connection/
  Settings 別ウィンドウ + カスタムタイトルバー + ツールバー + 基本的なマルチウィンドウ」に集中。
- ネイティブ viewport 埋め込み（引き続き post-alpha）。

## 7. 受け入れ基準（Acceptance）

1. OS 既定タイトルバーが消え、アプリテーマと一体の**カスタムタイトルバー**（最小化/最大化/
   閉じる/ドラッグ可）が出る。
2. タイトルバー直下に**ツールバー**があり、主要アクション（Play/Pause/Stop/Launch/
   Reconnect/Focus、Connection・Settings・Log の表示制御）に届く。
3. **Game View が最大・中央**で主役。
4. **Scene Outliner が右**。
5. **Property Inspector はオブジェクト選択時のみ表示**（未選択で出ない/最小化）。
6. **Log は常時表示でなく**、下に格納＋ホバー/任意展開（or 別ウィンドウ）。
7. **Connection / Settings が別ウィンドウ**で開ける（常時はメイン画面に出ない）。
8. **複数ウィンドウを作れる**（最低 Connection/Settings 独立。可能なら任意パネルの tear-off）。
   別ウィンドウでも接続/状態が backend イベントで同期する。
9. 既存機能の回帰なし。全検証ゲート緑。impl-reviewer 承認。

## 8. ユーザーへのオープンクエスチョン（plan 提示時に確認）

1. **マルチウィンドウ方式**: 案 A（Tauri WebviewWindow）か案 B（dockview popout）か。
   推奨は案 A（backend イベント同期と相性が良く、Connection/Settings の独立ウィンドウ化が素直）。
2. **Log の挙動**: 下部バーのホバーでポップアップ / クリックで下ドック展開 / 別ウィンドウ展開 —
   どれを既定にするか（複数併用可）。
3. **Inspector の出し方**: 選択時に右ドックへ動的に追加（非選択で消す）か、常設だが選択時のみ
   中身表示（UE の Details に近い）か。ユーザーは「選択時だけ表示」を希望 → 前者寄り。
4. **ツールバーの項目**と並び、アイコンの有無（テキスト/アイコン）。
5. **タイトルバー**: 完全自前（decorations:false）か overlay 方式か。アプリ名/メニューの有無。
6. dockview を継続利用するか、レイアウト機構を見直すか（既存資産を活かすなら継続が無難）。

## 9. 着手の足場（次セッションが最初に読む実ファイル）

- `apps/editor/src/components/AppLayout.tsx`（dockview 既定レイアウト・永続化）
- `apps/editor/src/components/{GameViewPanel,ConnectionPanel,SettingsPanel,LogPanel,SceneOutlinerPanel,PropertyInspectorPanel}.tsx`
- `apps/editor/src/hooks/useBridge.ts`（useBridgeSubscriptions / useBridgeActions）、`apps/editor/src/state/store.ts`、`state/BridgeContext.tsx`
- `apps/editor/src/styles.css`（テーマトークン・dockview テーマ）、`apps/editor/src/App.tsx`（BridgeRoot）
- `apps/editor/src-tauri/tauri.conf.json`（window decorations / CSP）、`apps/editor/src-tauri/capabilities/default.json`（権限・windows）
- `docs/agent-guide/tauri-security.md`、`docs/agent-guide/typescript.md`、`docs/phase2-editor-evolution-plan.md`（engine-agnostic 不変条件）
- 参考: dockview 公式（floating/popout groups）、Tauri 2（custom titlebar / multiwindow / capabilities）の最新ドキュメントを research で確認すること。
