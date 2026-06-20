## v2.1 変更点（レビュー反映）

レビュー指摘の全 [BLOCKER]/[SHOULD-FIX] を反映した。**最大の訂正は Phase 0 の C++ 移行記述の事実誤認の是正**: 存在しない `ws_roundtrip_test.cpp` への言及を削除し、「engine 送出 envelope version を一律 0.2 へ」という誤指示を撤回、実際に壊れるのは `smoke_test.cpp:64` と `dispatch_test.cpp:219` の **2箇所のみ**に限定した（他の C++ "0.1" リテラルは inbound フレーム/DTO テストデータにつき変更しない旨を明記）。**Rust 統合テスト群（ws_roundtrip.rs / loopback_roundtrip.rs / ws_reconnect.rs / conformance.rs）を Phase 0 Scope に追加**し「0.1 互換テストとして offer/assert を温存・一括置換禁止」を確定、加えて **0.1-only legacy engine への後方互換交渉を直接実証するテスト**と「editor の envelope.version は negotiate 値ではなく PROTOCOL_VERSION 定数固定」設計の明記を Phase 0 成果物に追加した。残りの [SHOULD-FIX]（Phase 6 の `EXPECTED_SUBSCRIPTION_COUNT` 9→11、Phase 1 のパネルテスト props→hook モック移行と localStorage 破損 purge）と [NICE-TO-HAVE]（Phase 7b の 256 KiB 根拠を WS フレーム上限へ紐付け、Phase 3 の env 変数対応表）も各 Phase に折り込んだ。

## v2 変更点（ユーザー決定の反映）

v1 で Open question として残していた4点をユーザー判断で確定し、計画に統合した。**(1) Phase 1** は静的 CSS grid 拡張をやめ、`dockview-react` v6.6.1（MIT・ゼロ外部依存・React 19 対応）の導入へ全面改訂した（AppLayout を DockviewReact へ置換、6パネルを dock 化、BridgeContext はデータ層として不変、レイアウト永続化 + reset-to-default、CSP は変更不要）。**(2) Phase 7b（サムネイル）** を「条件付き／将来送り」から **今フェーズ IN-SCOPE** へ格上げし、memory-buffer-policy 準拠の数値デフォルト（PNG / 最大 640×360 / 256 KiB ハードキャップ / pull型 `viewport.getThumbnail` / 1 fps 上限）を確定した。**(3) protocol version** を **0.1→0.2 MINOR bump** に確定し、研究の移行チェックリストに沿った専用クロスカット Phase 0 を新設、エディタは `["0.2","0.1"]` を提示して 0.1 engine と後方互換交渉する設計とし、既存 114 fixture は 0.1 互換記録として温存・0.2 専用 fixture を追加する戦略にした。**(4) デモデータ供給源** は mock engine 一本化・未接続時は空状態で確定済みとして Open question から削除した。capability token 広告は version bump と併存し引き続き必須成果物とする。

---

# NorvesEditor 次フェーズ実装計画 v2.1 — Docking / Outliner / Inspector / Live-update / Viewport / Protocol 0.2

## 0. 計画全体の方針と根拠

### 目的（Purpose）と期待される振る舞いの変化
現状の NorvesEditor は接続/プロセス制御/ログという「Bridge 制御スライス」のみを持ち、ワールド内オブジェクトを見る・選ぶ・編集する手段がなく、パネルは固定 CSS grid に固定されている。本計画は **ADDITIVE のみ** で以下を段階導入する。

1. **ドッキング UI** — `dockview-react` によるタブ/グループ/ドラッグ移動可能なパネルシステムへ移行し、レイアウトを永続化・リセット可能にする。
2. **Scene Outliner** — シーンツリーを表示しノードを選択できる。
3. **Property Inspector** — 選択オブジェクトのプロパティを read-only 表示 → 編集可能化。
4. **ライブ更新イベント** — `scene.treeChanged` / `object.changed`（NEW additive events、**version 0.2 で導入**）。
5. **Viewport 統合** — Game View の viewport 状態可視化（7a）に加え、**今回 7b の静止画サムネイル（`viewport.getThumbnail`）も実施**。native embedding は将来フェーズへ分離。
6. **protocol 0.1→0.2 MINOR bump** — 新規イベント/メソッドを version 0.2 で導入し、0.1 engine とは後方互換交渉する。

### 重要な事実（研究と実コードで裏取り済み）
- **バージョン交渉は既にプロトコル仕様として設計済み**。`HelloParams.protocol_versions: Vec<VersionString>` は「優先順の複数バージョン」を許容（`bridge/crates/norves-bridge-editor-client/src/handshake.rs:44-45`、確認済み）。C++ `NegotiateVersion()` は「クライアント提示順 × `SupportedProtocolVersions` の最初の一致」を選ぶ（`server.cpp:118-131`、確認済み）。**現状はエディタが `["0.1"]` のみ提示しているため単方向**（`bridge_state.rs:292-301`、確認済み）。
- **エンベロープ version は `SupportedProtocolVersions.front()` で固定**される（`server.cpp:50` の `EnvelopeVersion()`、確認済み）。配列を `{"0.2","0.1"}` にすると engine が送出する全エンベロープの version が "0.2" になる。**ただし C++ テストには engine 送出 envelope の version を assert している箇所は1つも無い**（後述 Phase 0 で訂正済み。レビューで grep 実証）。
- **editor 送出 envelope の version は negotiate 値ではなく `PROTOCOL_VERSION` 定数で固定**される（`build_request`、`bridge_state.rs:141`）。bump 後は editor が常に "0.2" envelope を送るが、C++ server は inbound envelope の version を検証しない（`server.cpp` handleFrame に version 照合なし、確認済み）ため、0.1-only engine に対し negotiate="0.1" でも "0.2" envelope を送って受理される。**この乖離は設計上正しい**ので `build_request` を negotiate 値に追従させてはならない。
- **schema は version 文字列を制約しない**。`envelope.schema.json:17` / `common.schema.json:8` の pattern は `^[0-9]+\.[0-9]+$` のみで "0.1" も "0.2" も妥当。**schema 変更不要**。
- **既存 fixture は 114 件すべて version "0.1"**。これらは「0.1 プロトコルでのやり取りの記録」であり**変更不要**。0.2 用は別途追加する。
- **fixture カウントは C++ と Rust の2系統がハードコード**（[BLOCKER] 級・実コードで確認）:
  - C++: `bridge/cpp/engine-sdk/tests/fixtures_roundtrip_test.cpp:192-195`（positive=55 / envelopeRejectable=14 / payloadOnly=45 / total=114）。
  - Rust: `bridge/crates/norves-bridge-core/tests/fixtures_roundtrip.rs` の `fixture_counts_are_exhaustive`（124-135行）**および** `roundtrip_positive_fixtures`(175) / `envelope_negative_rejected`(195) / `payload_negative_accepted_at_envelope_layer`(221) の3テスト。**fixture を1件でも追加したら C++ 4箇所 + Rust 4箇所を同一コミットで更新**しないと両ワークスペースの test が壊れる。
- **`PROTOCOL_VERSION` 定数は2箇所**: Rust `bridge_state.rs:58 const PROTOCOL_VERSION: &str = "0.1"`（確認済み）、C++ `version.hpp:22-24 SupportedProtocolVersions = {"0.1"}`（要素数1、確認済み）。
- **CSP は dockview / サムネイル両方を既にカバー**: `tauri.conf.json:23` = `default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'`（確認済み）。dockview の直接 DOM スタイル代入と `data:` URL サムネイルの両方が許容済み、**CSP 変更不要**。
- **dockview-react v6.6.1**: MIT、ゼロ外部 runtime 依存、peerDep `react >=16.8||17||18||19`。`apps/editor/package.json` は React 19.1.0（確認済み）で互換。`api.toJSON()/fromJSON()` でレイアウト永続化、`disableFloatingGroups`/`disableDnd` オプションあり。CSS は `import 'dockview/dist/styles/dockview.css'` の静的 import のみ。
- **`@tauri-apps/plugin-store` は未導入**（`apps/editor/package.json` 確認済み）。レイアウト永続化は **localStorage** を採用（追加依存ゼロ）。
- **FakeAdapter 二重管理**: `mock_adapter.hpp` の MockAdapter と `ws_test_server.cpp` の FakeAdapter は意図的コピー。両方同値更新しないと conformance / e2e が乖離検出で失敗。
- **conformance runner** は環境変数 **`NORVES_MOCK_ENGINE`** で駆動（`conformance.rs:302`）。**`process_e2e.rs` は別変数 `NORVES_ENGINE_PATH`**（`process_e2e.rs:469`）。両者は別物（取り違え注意）。
- **IPC名3点同期**: `protocol_names.rs` ↔ `bridge-ui/src/commands.ts` ↔ `events.ts` を `scripts/check-protocol-names.mjs` が強制。TS 抽出は `/^[a-z][a-z_]*$/`（**数字を含まない snake_case のみ**, `:56`）。**コマンド/イベント名に数字を入れない**ことを全フェーズの制約とする（version 番号は名前に入れない）。
- **2ワークスペース構成**: root（bridge crates 4本）と `apps/editor/src-tauri`（別 workspace、`--workspace` から除外）。`scripts/verify.ps1` が両方を流すため正典コマンドとする。

### シーケンス設計の根拠（なぜこの順序か）
**「protocol 0.2 土台 → UIドッキング土台 → ボトムアップ read 経路 → write 経路 → ライブ更新 → viewport」** の背骨にする。version bump（Phase 0）は新規イベント/メソッドが最初に登場する Phase 6 の**直前**に着地させ、「新規メッセージは最初から version 0.2 で導入する」原則を守る（0.1 で導入してから 0.2 へ移すと二重作業になる）。Phase 1（dockingレイアウト+選択モデル）はフロントだけで完結し即マージ可能。Phase 2/3/4 で既存 schema のまま read 経路を縦に通し、Phase 5（write）→ Phase 6（version 0.2 のライブイベント）→ Phase 7（viewport）の順でリスクを後置する。

### 1テーマ1ブランチの原則
各 Phase は独立ブランチ・独立レビュー・独立コミット・独立検証ゲートを持つ。Phase 0（protocol 0.2 bump）は**クロスカット**だが「version 文字列の bump + 後方互換交渉」という単一テーマなので1ブランチに収める。Phase 4 と 5 は同一 Inspector テーマだが read-only と write でプロトコル副作用が大きく異なるため別ブランチに分割する。

### 全エンジン接続思想の不変条件（engine-agnostic invariants — 全フェーズ厳守）

今回のフェーズは**データ源として mock engine に一本化するが、これは「開発・テスト用のデータ供給」をモックに集約する意味であり、エディタをモックに結合する意味ではない**。NorvesEditor は今後も「プロトコルを話す任意のゲームエンジン（NorvesLib を含む）」に接続できる思想を維持する。`docs/agent-guide/architecture.md` の「generic bridge は NorvesLib に依存しない」「C++ SDK が境界」を一切侵さないため、以下を全フェーズの不変条件とする。

1. **境界はプロトコル / `IBridgeEngineAdapter`**。MockAdapter は `bridge/cpp/examples/` 配下の一実装にすぎない。Rust/Tauri/TS/React は「`scene.getTree` 等の汎用メソッド/イベント」を話すだけで mock の存在を知らない。**エディタ側コードに mock 固有の分岐・名前・前提を入れない**。
2. **契約の正典は fixtures / schema（汎用）**。mock は fixtures に「合わせる」側であって、fixtures が mock に合わせるのではない。conformance の exact match は「汎用契約に mock が準拠していること」の検証。
3. **任意エンジンの劣化動作を最初から織り込む**。オプション機能（scene/object/schema/viewport.thumbnail/live-update）は C++ SDK で**非純粋仮想 + `not_supported()` デフォルト**。これを実装しないエンジンも**コンパイルでき `METHOD_NOT_SUPPORTED` を返す**。UI は capability token の有無と `METHOD_NOT_SUPPORTED` を受けて「未対応」を優雅に表示する（Phase 3(c) 等）。→ scene/object を持たないエンジンでも接続・基本制御は成立する。
4. **DTO / 型 / UI は汎用形**。Rust DTO・TS 型・React 描画は schema 準拠の一般形（任意ツリー / 任意 property bag / schema 駆動）であり、mock のデモ内容（"Camera"/"Light"/"Player" 等）に最適化したフィールドを焼き込まない。Inspector は固定フィールドではなく**プロパティ列挙 + 型駆動**で描画する。
5. **version 交渉で多世代エンジンを許容**（Phase 0）。`["0.2","0.1"]` 提示で 0.1-only エンジンも接続。将来エンジンが新版でも negotiate で吸収。
6. **イベントは engine wire 側が emit**（editor 合成ではない、Phase 6）。同じ wire イベントを送る任意エンジン（将来の NorvesLib 実エンジン）が**エディタ無改変**で動く。
7. **NorvesLib 連携は将来フェーズとして温存**。generic bridge / C++ SDK は NorvesLib に依存せず単体ビルド可能（既存境界規約）。NorvesLib は同じ `IBridgeEngineAdapter` を実装する別アダプタとして後日接続する。

これにより「mock 一本化」は今フェーズの**データ供給の都合**にとどまり、将来の NorvesLib 接続も「あらゆるエンジンと接続できる」思想も損なわれない。

---

## Phase 0 — protocol version 0.1→0.2 MINOR bump + 後方互換交渉（クロスカット）

**Goal**: ワイヤープロトコルを 0.2 へ MINOR bump し、エディタが `bridge.hello` で `["0.2","0.1"]` を優先順提示することで「0.2 engine とは 0.2、旧 0.1 engine とは 0.1」を交渉する。**既存 114 fixture は 0.1 互換記録として温存**し、0.2 専用 fixture を追加する。これは Phase 6（新規イベント）/ Phase 7b（新規メソッド）が version 0.2 で着地できる土台。

**Layer(s)**: プロトコル docs + Rust backend + C++ SDK + fixtures（schema 変更なし）。

**判断の確定（precise decisions）**:
- **エディタは `["0.2","0.1"]` を提示する**（後方互換）。0.2 engine が選べば 0.2、0.1-only engine なら 0.1 にフォールバック。交渉ロジック自体は既存で動作するため `HelloParams::new()` の呼び出し引数を変えるだけ。
- **C++ SDK は `SupportedProtocolVersions = {"0.2","0.1"}`**（要素数2、先頭が優先）にする。これで mock/SDK が 0.2 engine として動作しつつ 0.1 エディタとも交渉できる。`EnvelopeVersion()=front()="0.2"` になるため engine 送出エンベロープは全て 0.2 になる。
- **editor 送出 envelope の version は `PROTOCOL_VERSION` 定数固定**（negotiate 値に追従させない）。0.1-only engine もこれを受理する（version 非検証）。`build_request` を改修してはならない（後述 mitigation で釘刺し）。
- **既存 fixture は version "0.1" のまま温存**（0.1 互換表現）。**0.2 専用 fixture を追加**（`bridge.hello/positive/request-valid-v02.json` + `response-valid-v02.json`）。version 文字列を全 fixture で書き換える方式は採らない（114件への波及と 0.1 互換記録の喪失を避ける）。
- **capability token は version bump と併存**: version は「プロトコル世代」、token は「個別機能対応」を広告する別軸。Phase 3〜7b の token 必須化は維持する。

**Scope（実ファイル）**:
- docs:
  - `bridge/spec/docs/protocol-overview.md`（Status 行を 0.2 へ、Methods/Events リストに後続フェーズの新規を追加する受け皿コメント）
  - `bridge/spec/docs/error-model.md`（MINOR bump 注記のバージョン言及を 0.2 へ）
- C++ SDK（**変更は version.hpp + 厳密に2箇所のテストのみ**）:
  - `bridge/cpp/engine-sdk/include/norves/bridge/version.hpp:22-24`（`SupportedProtocolVersions = {"0.2","0.1"}` の要素数2配列へ）
  - `bridge/cpp/engine-sdk/tests/smoke_test.cpp:64`（`SupportedProtocolVersions[0] == "0.1"` → `"0.2"`。配列先頭=優先バージョン）
  - `bridge/cpp/engine-sdk/tests/dispatch_test.cpp:219`（`TestHelloVersionUnsupported` の `"supported":["0.1"]` → `["0.2","0.1"]`。offered=`["2.0"]`(:190) は `{"0.2","0.1"}` と交点なしで `PROTOCOL_VERSION_UNSUPPORTED` を維持。`supported` echo は SDK の実セットを反映する正しい変更）
  - **【訂正・レビュー反映】変更してはならない C++ "0.1" リテラル（明示）**: 以下は engine 送出 envelope の version assert ではなく、**クライアントが構築する inbound フレーム** か **DTO ラウンドトリップのテストデータ**であり、decoder は `^[0-9]+\.[0-9]+$` を受理するため**変更不要**:
    - `dispatch_test.cpp:125`（`RequestFrame()` ヘルパの inbound フレーム）, `:347`（`TestNonRequestFrameReturnsNullopt` のテストデータ）
    - `loopback_roundtrip_test.cpp:154, :371, :379, :434, :440, :446`（inbound フレーム / DTO ラウンドトリップ）
    - `loopback_roundtrip_test.cpp:260` の `NORVES_CHECK_EQ(r.protocolVersion, "0.1")` は **negotiate 結果**（`HelloResult.protocolVersion`）。同テストは `protocolVersions={"0.1"}`(:240) を offer しており、SDK が `{"0.2","0.1"}` でも交点は "0.1" なので **このアサートは壊れない・変更不要**。
    - **存在しないファイル**: `ws_roundtrip_test.cpp` は C++ tests に存在しない（実在は bounded_queue / dispatch / fixtures_roundtrip / loopback_roundtrip / smoke / ws_server_transport / ws_test_server の7本）。言及しない。
  - `mock_adapter.hpp:73` の capability version "0.1" は **capability 自体のバージョン（runtime.control v0.1）でプロトコルバージョンではない**。**誤って変更しない**。
- Rust backend:
  - `apps/editor/src-tauri/src/bridge_state.rs:58`（`const PROTOCOL_VERSION: &str = "0.2"`）
  - `apps/editor/src-tauri/src/bridge_state.rs:292-301`（`HelloParams::new()` の `protocol_versions` を **`["0.2","0.1"]` の2要素**へ）
  - `apps/editor/src-tauri/src/bridge_state.rs:141`（`build_request`）は **改修しない**。envelope version は `PROTOCOL_VERSION` 定数固定のまま（negotiate 値に追従させない）。
  - `apps/editor/src-tauri/src/bridge_state.rs:633`（テスト内インライン `VersionString::try_from("0.1")` は「送出するバージョン」なら "0.2"、「受信した旧 engine 応答」なら "0.1" のまま — 文脈を精査）
  - `apps/editor/src-tauri/tests/process_e2e.rs:164,219`（`hello_envelope()` / `request_envelope()` の version 引数。エディタが送る side は "0.2"）
- Rust bridge crates（**unit test + 統合テスト両方を Scope に明示**）:
  - `bridge/crates/norves-bridge-editor-client/src/handshake.rs`: `RESPONSE_VALID`(:234) と `assert_eq!(outcome.protocol_version, "0.1")`(:367) は**「0.1 engine 応答」テストとして保持**し、別途「0.2 engine 応答（version:"0.2", protocolVersion:"0.2"）」テストを追加。同モジュール内で旧/新 engine ケースを明示分離。
  - **【新規・レビュー反映】統合テスト群の扱いを確定**: `bridge/crates/norves-bridge-editor-client/tests/` 配下の以下は **0.1 互換テストとして offer/assert を "0.1" のまま温存**（C++ ws_test_server を子プロセス spawn し、テストが `["0.1"]` を offer する限り negotiate 結果は "0.1" でアサートが通る）。**安易な一括 "0.1"→"0.2" 置換を禁止**する（特に下記 `ws_roundtrip.rs:56` の `version()` ヘルパを 0.2 に変えると `:233` の `assert_eq!(outcome.protocol_version, "0.1")` が破れる）:
    - `ws_roundtrip.rs:56`（`version()` ヘルパ）, `:233`（negotiate 結果 assert）
    - `loopback_roundtrip.rs:40, :127, :221`
    - `ws_reconnect.rs:49`
    - `conformance.rs:53`
    - 0.2 交渉の正経路は**別途 0.2 offer テストを追加**して担保する（温存テストは触らない）。
  - `bridge/crates/norves-bridge-tools/src/bin/bridge_cli.rs:41`（CLI の `protocol_version()` を "0.2" へ）
- fixtures:
  - **新規追加** `bridge/spec/fixtures/methods/bridge.hello/positive/request-valid-v02.json`（envelope version:"0.2", protocolVersions:["0.2"]）+ `response-valid-v02.json`（envelope version:"0.2", result.protocolVersion:"0.2"）。既存 `request-valid.json`/`response-valid.json` は無変更。
  - **[整合] `response-version-unsupported.json`**: 現状 `"supported":["0.1","1.0"]`（架空例）。SDK が `["0.2","0.1"]` になるため `"supported":["0.2","0.1"]` へ更新し実態と一致させる。envelope version は **"0.1" のまま**（0.1 engine が 0.1 エンベロープで unsupported を返す表現）。カウント不変（既存ファイルの値変更のみ）。
  - **[BLOCKER] fixture カウント更新**: positive 2件追加 → C++ `fixtures_roundtrip_test.cpp:192`(positive 55→57) + `:195`(total 114→116)、Rust `fixtures_roundtrip.rs:124`(positive 55→57) + `:132-133`(total 114→116) + `:175`(roundtrip 55→57) を**同一コミットで全更新**。

**【新規・レビュー反映】後方互換の直接実証テスト（成果物）**:
ユーザー判断3の核心「0.1-only engine とのハンドシェイク交渉が壊れない」を**直接証明する**テストを追加する。bump 後の C++ harness は `{"0.2","0.1"}` になり「0.1-only engine」を再現できないため:
- **(a) legacy engine 模擬**: FakeAdapter（`ws_test_server.cpp`）または専用 fake で `SupportedProtocolVersions={"0.1"}` を差し替えたビルドを用意し、editor が `["0.2","0.1"]` を offer して **negotiate 結果が "0.1" にフォールバック**する経路を検証する Rust 統合テストを1本追加。
- **(b) envelope.version 乖離の明文化テスト/コメント**: 「editor の envelope.version は `PROTOCOL_VERSION`="0.2" 固定で、0.1 negotiate でも "0.2" envelope を送り、server は version 非検証で受理する」ことをテストまたはコード注記で固定し、実装者が `build_request` を negotiate 値追従に誤改修しないよう釘を刺す。

**Cross-layer wiring order**: docs → C++ version.hpp + C++ テスト 2箇所のみ更新 → 0.2 fixtures 追加 + カウント更新（C++/Rust） → Rust `PROTOCOL_VERSION` + `HelloParams` 引数 + Rust unit test 分離 + 統合テスト温存確認 → legacy 後方互換テスト追加 → bridge_cli。

**Verification gate**:
```
python scripts/validate-bridge-fixtures.py        # 116 fixture(s) validated を確認
ctest --test-dir build/cpp -C Debug --output-on-failure   # C++ version assert(2箇所) + カウント
# bridge workspace（Rust fixture カウント検証 + 統合テスト温存 + legacy 後方互換テスト含む）:
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
# apps/editor/src-tauri workspace（明示）:
cd apps/editor/src-tauri && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test
./scripts/verify.ps1 -Cpp
```

**Risks & mitigation**:
- **【訂正・最重要】C++ engine 送出 envelope version は assert されていない**: C++ テスト全体に `env.version` / `EnvelopeVersion` を assert する箇所は無い（レビューで grep 実証）。**実際に壊れて修正が必要なのは `smoke_test.cpp:64` と `dispatch_test.cpp:219` の2箇所のみ**。inbound フレーム/DTO テストデータの "0.1" リテラルは**変更しない**（変更すると `TestNonRequestFrameReturnsNullopt` 等の意味を壊す）。
- **fixture カウント二重ハードコード**: C++ 2箇所 + Rust 3箇所を同一コミットで更新（上記）。
- **editor envelope.version と negotiate 値の乖離**: 設計上正しい（server は version 非検証）。`build_request` を改修しない。後方互換テスト (b) で固定。
- **Rust 統合テストの一括置換禁止**: `ws_roundtrip.rs:56` の `version()` を 0.2 に変えると `:233` が破れる。温存テストは offer/assert ともに "0.1" のまま。
- **mock_adapter.hpp:73 の capability version "0.1"**: bump 対象外。誤変更しない。

**Dependencies**: なし（Phase 1 と並行可能）。Phase 6 / 7b は本 Phase に依存（新規メッセージは 0.2 で導入）。

**Opus escalation**: **必須**（protocol schema/compat + version 交渉 + C++ public 定数 + fixture 整合 = 最高 load-bearing）。

**コミット可能条件**: 上記ゲート全通過 + impl-reviewer 承認 + `feature/protocol-0_2-bump` ブランチ。**コミット本文必須**（protocol version 変更）。

---

## Phase 1 — dockview ドッキング UI 導入 + クロスパネル選択モデルの土台

**Goal**: AppLayout の静的 CSS grid を `dockview-react` ベースのドッキングレイアウトへ全面置換し、既存4パネル（GameView/Connection/Settings/Log）+ 新規2パネル（SceneOutliner/PropertyInspector）を dock パネルとして登録する。`selectedObjectId` をストアに導入する。レイアウトを localStorage に永続化し reset-to-default を提供する。**既存4パネルのレンダリング本体は不変**（ただしテスト/型の波及は「不変」ではない、後述）。

**Layer(s)**: TS + React のみ（プロトコル/C++/Rust に一切触れない）。dockview はレイアウト管理のみ、データ層には触れない。

**依存追加（license/dependency-review note）**:
- 追加: `pnpm --filter @norves/editor add dockview-react@^6.6.1`（**dependency** として、devDependency ではない）。
- ライセンス: **MIT**（許容）。runtime 外部依存ゼロ（`dockview-core` のみ）。`pnpm-lock.yaml` の差分が発生する → 同一コミットに含める。
- React 19.1.0 と peerDep `^19` で整合（確認済み）。

**Scope（実ファイル）**:
- `apps/editor/package.json`（dependencies に `dockview-react: ^6.6.1`）+ `pnpm-lock.yaml`
- `apps/editor/src/main.tsx` または `apps/editor/src/styles.css`（`import 'dockview/dist/styles/dockview.css'` を追加。既存 `styles.css` は温存）
- `apps/editor/src/components/AppLayout.tsx`（**全面書き換え**: `DockviewReact` ラッパー化）
- 既存4パネル（`GameViewPanel.tsx` 等）: props drilling を廃し各パネルが内部で `useBridgeState()`/`useBridge()` を直接呼ぶ形へ変更（**レンダリングロジック本体は無変更**）。`GameViewPanel` は現在 `AppLayout.tsx:58-73` で `onLaunch`/`onStopProcess`/`onReconnect` 等 **14個の props を明示注入**されており、直呼び化はインターフェース改変。**既存パネルテストの props モック → hook モックへの移行を成果物に含める**（下記 deliverable (h)）。
- `apps/editor/src/components/SceneOutlinerPanel.tsx`（新規・プレースホルダ。`IDockviewPanelProps` を受け、`useBridgeState()` でシーンデータを参照する構造）
- `apps/editor/src/components/PropertyInspectorPanel.tsx`（新規・プレースホルダ。`useBridgeState().selectedObjectId` 等を参照）
- `apps/editor/src/state/store.ts`（`BridgeState` に `selectedObjectId?: string`、`BridgeAction` に `{ type:'objectSelected'; id: string | undefined }`、reducer case、`INITIAL_STATE`）
- `apps/editor/src/hooks/useBridge.ts`（`BridgeActions` に `selectObject(id: string | undefined): void` 追加、useCallback 実装、return 追加）
- `apps/editor/src/styles.css`（`.dockview-theme-dark` スコープで `--dv-*` 変数を既存 `--color-*` トークンへマッピング。名前空間衝突しない）
- レイアウト永続化ユーティリティ（localStorage キー `norveseditor-layout-v1`）
- テスト: `store.test.ts`（`objectSelected` reducer、`undefined` deselect）、AppLayout / 新パネルの最小レンダリングテスト（jsdom、`@testing-library/react` 既導入）、既存4パネルテストの hook モック移行

**Exact deliverables**:
- **(a) DockviewReact 統合**: `AppLayout.tsx` を `<DockviewReact components={...} onReady={...} className='dockview-theme-dark' disableFloatingGroups={true} />` へ置換。`components` マップ = `{ gameView, connection, settings, log, sceneOutliner, propertyInspector }`。
- **(b) default layout**: `onReady` で `event.api.addPanel()` ×6 を呼び、v1 の視覚配置（中央 GameView、右に Connection/Settings、下に Log、左に Outliner、右に Inspector）を再現。
- **(c) レイアウト永続化 + reset**: `api.onDidLayoutChange()` → `api.toJSON()` を localStorage `norveseditor-layout-v1` へ保存。`onReady` で保存済みがあれば `api.fromJSON()` で復元。**【新規・レビュー反映】`fromJSON` が throw したら当該 localStorage キーを削除（purge）してから default layout を再構築**（破損 JSON が WebView2 localStorage に永続して毎回 catch する状態を掃除）。SettingsPanel に「レイアウトをリセット」ボタン（localStorage キー削除 → default layout 再構築）を追加。
- **(d) `disableFloatingGroups={true}`**: alpha では floating / popout を無効化（`popoutUrl` を指定しなければ popout はトリガーされない）。
- **(e) 選択モデル**: `selectObject(undefined)` で deselect。Outliner/Inspector/GameView が `useBridgeState()` で `selectedObjectId` を読み、`selectObject` で書く。**別 Context は導入しない**（既存 Context 分割を温存）。
- **(f) UI 状態規約**: プレースホルダ時点で「選択なし / 切断 / 空シーン」の3空状態コピーを用意（Phase 3/4 で実データに差し替え）。
- **(g) data source 確定（決定済み）**: デモデータは **mock engine 一本化、未接続時は空状態**を表示。TS static デモデータは併用しない（確定事項）。
- **(h)【新規・レビュー反映】既存パネルテストの移行**: props drilling 撤廃に伴い、`@testing-library/react` の既存パネルテストが props モック前提なら hook モック（`useBridge`/`useBridgeState` を vi.mock）へ移行する。これを成果物として明記し、回帰確認を verification gate に含める。「レンダリングロジック本体は無変更」だが**テスト・型の波及は不変ではない**点を実装者に明示。

**Cross-layer wiring order**: dockview install → CSS import → store/hook（選択モデル）→ AppLayout 置換 → 既存パネルの hook 直呼び化 + テスト移行 → 新パネル stub → 永続化/reset/purge（フロント内で完結）。

**Verification gate**:
```
pnpm install                                  # pnpm-lock.yaml 更新確認
pnpm -r --if-present typecheck                # IDockviewPanelProps 実装の型検証含む
cd apps/editor && pnpm test                   # vitest（jsdom）。既存パネルテストの hook モック回帰含む
cd apps/editor && pnpm build                  # dockview バンドル/CSS の build 通過
./scripts/verify.ps1
```

**Risks & mitigation**:
- *レイアウト lock-in*: `SerializedDockview` JSON はライブラリ固有。永続化キーを `-v1` で versioning。`fromJSON` 失敗時は**キー purge + default fallback**（毎回 catch する破損データの掃除を確定）。将来 dockview メジャー更新で形状が変わった場合もこの purge 経路で回復。
- *WebView2(Win) / WKWebView(mac) のドラッグ挙動差*: dockview は Pointer Events + ResizeObserver。`disableFloatingGroups` でリスク面を縮小。WKWebView 実機ドラッグは将来検証（GAP、alpha 主ターゲットは Windows）。
- *exhaustiveness guard*（`store.ts` の never guard）: 新 action 追加で reducer case 必須 → typecheck で機械検出。
- *props drilling 撤廃に伴う回帰*: パネル本体ロジックは無変更だがデータ取得元が hook 直呼びへ変わるため、**既存パネルテストの hook モック移行が必須**（deliverable (h)）。各パネルのレンダリングテストで回帰検出。
- *CSP*: 変更不要（`style-src 'self' 'unsafe-inline'` が dockview の直接 DOM スタイル代入をカバー、確認済み）。`tauri.conf.json` は触らない。

**Dependencies**: なし（最初に着手可能、Phase 0 と並行可）。

**Opus escalation**: 原則不要（フロント限定）。ただし dockview 統合は新ライブラリ導入のため、レイアウト永続化/フォールバック/purge 設計部分のみ慎重にレビュー。

**コミット可能条件**: ゲート全通過 + impl-reviewer 承認 + `feature/ui-docking-layout` ブランチ。`pnpm-lock.yaml` 差分を同一コミットに含める。

---

## Phase 2 — mock engine の scene/object/schema デモデータ実装（C++）

**Goal**: MockAdapter に `sceneGetTree`/`objectGetSnapshot`/`objectSetProperty`/`schemaGetSnapshot` をオーバーライドし、NorvesLib 無しで静的デモシーンを返す。**既存 schema は変更せず**、既存 fixtures の形に値等価で合わせる。C++ 層として ctest/conformance で自己完結検証する（UI 表示は Phase 3 で達成）。

**Layer(s)**: C++ Engine SDK（mock + FakeAdapter）。schema 変更なし。

**Scope（実ファイル）**:
- `bridge/cpp/examples/mock-engine/mock_adapter.hpp`（4メソッドのオーバーライド。静的デモツリー/プロパティ構築、`bridge/spec/fixtures/methods/*/positive/` の形に整合。状態は**インメモリ静的マップ**で保持し setProperty で更新）
- `bridge/cpp/engine-sdk/tests/ws_test_server.cpp`（FakeAdapter に**同値で**同4メソッド追加 — 二重管理ルール厳守）
- `bridge/cpp/engine-sdk/tests/dispatch_test.cpp`（必要なら scene/object dispatch 成功テスト追記。既存 `TestUnimplementedOptionalMethodIsMethodNotSupported`(278-297) は**温存**）
- `bridge/conformance/runners/alpha_method_sequence.json`（`scene.getTree`/`object.getSnapshot`/`schema.getSnapshot` ステップ追加。MockAdapter 返値と fixture を exact match）
- （任意・推奨）`bridge/cpp/engine-sdk/include/norves/bridge/dto/methods.hpp` + `dto_codec.cpp` に型付き DTO（`SceneNode`/`ObjectSnapshot` 等）を追加（memory-buffer-policy の「snapshot/DTO化」を最も明示的に満たす）

**Exact deliverables**:
- 静的デモシーン（root → "Camera" / "Light" / "Player"(→ "Weapon")。fixtures が3階層ネスト検証済みで安全圏）。
- `objectGetSnapshot` は `objectId → properties(propertyBag)` を返す。`objectSetProperty` は `{accepted:true, appliedValue:<echo>}` を返し**インメモリ静的マップを更新**。
- `schemaGetSnapshot` は型記述子（`typeName` + `properties[{name,valueType}]`）を返す。
- すべて**エンジン内部ポインタを渡さず値コピーで JsonValue を構築**（memory-buffer-policy / `adapter.hpp:29-31`）。
- **スレッド制約の明文化**: MockAdapter のデモ状態は mock の **single-thread recv loop 前提**でのみ安全。`handleFrame` がアダプタを同期・同スレッドで呼ぶため、**mock を将来もマルチスレッド化しない**ことをコメント/コミット本文に記録。

**Cross-layer wiring order**: C++（mock + FakeAdapter 同時）→ conformance runner。Rust/TS には触れない。

**Verification gate**:
```
ctest --test-dir build/cpp -C Debug --output-on-failure
python scripts/validate-bridge-fixtures.py
NORVES_MOCK_ENGINE=<mock-engine path> cargo test -p norves-bridge-editor-client -- alpha_method_sequence
./scripts/verify.ps1 -Cpp
```
※ build/cpp 未configure: `cmake -S bridge/cpp -B build/cpp -G "Visual Studio 17 2022"` → `cmake --build build/cpp --config Debug`。

**Risks & mitigation**:
- *FakeAdapter 乖離*（最重要）: MockAdapter と FakeAdapter を**同一コミットで同値追加**。conformance が乖離検出。
- *fixtures カウント*: **本フェーズで新規 fixture を追加しない（既存 positive に値を合わせる）なら C++/Rust 両カウント不変**。新規 positive を足すなら C++ 4箇所 + Rust 4箇所同時更新。
- *engine live memory 混入*: 値コピーのみ。`JsonValue` 構築箇所がポインタ/span を含まないことをレビュー確認。
- *conformance exact match 失敗*: MockAdapter 返値を fixture に合わせるか ignore リスト調整。

**Dependencies**: Phase 0（version 0.2: mock engine が `SupportedProtocolVersions={"0.2","0.1"}` で動作する前提に整合させる）。Phase 1 とは独立。

**Opus escalation**: **必要**（C++ public API 周辺 + buffer/memory ownership + conformance 整合）。

**コミット可能条件**: ゲート全通過 + impl-reviewer 承認 + `feature/mock-scene-data` ブランチ。**コミット本文必須**（C++ ownership/thread）。

---

## Phase 3 — Outliner end-to-end（`scene.getTree`: Rust コマンド + TS型 + React パネル）

**Goal**: 既存 `scene.getTree` を Rust Tauri コマンド化し、TS型・wrapper・React パネルまで縦に配線。mock のデモシーンを Outliner に表示し、ノードクリックで `selectObject` を発火。未接続/空/エラー状態を Outliner で正しく表示する。

**Layer(s)**: Rust+Tauri / TS+React（schema 既存、変更なし）。

**Scope（実ファイル）**:
- Rust:
  - `apps/editor/src-tauri/src/bridge_state.rs`（`#[tauri::command] pub async fn scene_get_tree(...) -> Result<Value, BackendError>` → `send_method(state.inner(), "scene.getTree", params)`。`handle_clone()` パターン厳守 = no-lock-across-await）
  - `apps/editor/src-tauri/src/lib.rs:28-39`（`invoke_handler![]` に登録）
  - `apps/editor/src-tauri/src/protocol_names.rs`（`commands::SCENE_GET_TREE` + 配列 + assertion test）
  - `apps/editor/src-tauri/src/dto.rs`（`SceneNodeDto`/`SceneTreePayload`、camelCase）
  - `bridge/crates/norves-bridge-editor-client/src/scene.rs`（`parse_scene_tree_result(&Value)` を `parse_status_result`(status.rs:49-67) テンプレートで実装）+ `lib.rs` の `pub use`
- TS:
  - `bridge/ts/packages/bridge-types/src/scene.ts`（`SceneTreeNode`/`SceneGetTreeResult`、`ObjectId` は `common.ts:21` 再利用）+ `index.ts` export
  - `bridge/ts/packages/bridge-ui/src/commands.ts`（`BRIDGE_COMMANDS.sceneGetTree:'scene_get_tree'`）
  - `bridge/ts/packages/bridge-ui/src/ipc-types.ts`（`SceneTreePayload`）+ index export
- React:
  - `apps/editor/src/components/SceneOutlinerPanel.tsx`（プレースホルダ実装化。ツリー再帰描画、クリック選択、空/未接続/エラー分岐）
  - `apps/editor/src/hooks/useBridge.ts`（`getSceneTree(): Promise<void>` → `invokeCommand(BRIDGE_COMMANDS.sceneGetTree)`。結果をストアへ、エラーは `lastError` 系）
  - `apps/editor/src/state/store.ts`（`sceneTree?: SceneTreeNode[]`、`sceneTreeLoaded` action、reducer、INITIAL_STATE）
- テスト: Rust（parse + コマンド）、TS（型/wrapper）、React（レンダリング・選択・空/未接続/エラー, jsdom）、`process_e2e.rs`（`NORVES_ENGINE_PATH` opt-in で `scene.getTree` ラウンドトリップ。Phase 2 前提）

**Exact deliverables（UI 状態）**:
- **(a) 未接続/切断**: 「エンジンに接続するとシーンが表示されます」空状態。
- **(b) 空シーン**: root が children 無しの「オブジェクトがありません」表示。
- **(c) `METHOD_NOT_SUPPORTED`（実 engine 時）**: `BackendError::Engine { code, message }` を受けて「この engine はシーン照会に未対応」表示（将来 NorvesLib 接続時の劣化動作）。
- **(d) deselect**: 空白クリック or 選択ノード再クリックで `selectObject(undefined)`。
- **capability token（必須）**: mock の `getCapabilities` 返値に `scene.query` トークンを含め `bridge/spec/docs/capabilities.md` に追記。UI はこのトークン有無で取得可否を判断（version 0.2 が選べない 0.1 engine への劣化判定手段としても機能）。

**Cross-layer wiring order**: （schema 既存）→ **C++(Phase 2 完了)** → Rust コマンド+validator+DTO → protocol_names → TS commands/ipc-types → bridge-types → React panel+hook+store。Rust と TS を**同一フェーズ内で同期**（`check-protocol-names.mjs`）。

**Verification gate**（**【新規・レビュー反映】env 変数の対応を明示**）:
```
node scripts/check-protocol-names.mjs
# bridge workspace:
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
# apps/editor/src-tauri workspace（明示）:
cd apps/editor/src-tauri && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test
pnpm -r --if-present typecheck && (cd apps/editor && pnpm test)
python scripts/validate-bridge-fixtures.py   # capability token 追記時
./scripts/verify.ps1
# process_e2e.rs（apps/editor/src-tauri）には NORVES_ENGINE_PATH を渡す:
NORVES_ENGINE_PATH=<mock-engine path> (cd apps/editor/src-tauri && cargo test -- --nocapture)
# 注: conformance(norves-bridge-editor-client) は別変数 NORVES_MOCK_ENGINE。取り違えないこと
```

**Risks & mitigation**:
- *no-lock-across-await 違反*: `handle_clone()` → guard drop → `send_method().await`（`bridge_state.rs:346-372`）。
- *IPC名3点ドリフト*: Rust(`protocol_names.rs`+配列+assertion) と TS(`commands.ts`) を同時更新。**`scene_get_tree` に数字を含めない**。
- *env 変数取り違え*: `process_e2e.rs`=`NORVES_ENGINE_PATH`(:469)、`conformance.rs`=`NORVES_MOCK_ENGINE`(:302)。テストごとに正しい変数を渡す。
- *REQUEST_TIMEOUT 5秒*（`bridge_state.rs:62`）: alpha デモは小規模で十分。大規模化は将来（GAP）。
- *capabilities/default.json*: 新コマンドは `core:default` 範囲内。`capabilities/default.json` 変更**不要**（プロトコル capability token とは別物）。

**Dependencies**: Phase 1（選択モデル/レイアウト/data source）、Phase 2（mock の `sceneGetTree` + capability token）。

**Opus escalation**: **必要**（新 Tauri コマンド + Rust async lifetime + DTO/validator）。

**コミット可能条件**: ゲート全通過 + impl-reviewer 承認 + `feature/outliner-scene-tree` ブランチ。**コミット本文必須**（Bridge public API）。

---

## Phase 4 — Inspector read-only（`object.getSnapshot` + `schema.getSnapshot`）

**Goal**: Outliner で選択したオブジェクトのプロパティを Inspector に read-only 表示。`schema.getSnapshot` の型記述子を併用し valueType に応じた表示。書き込みなし。未選択/空プロパティ/エラー状態を扱う。

**Layer(s)**: Rust+Tauri / TS+React（schema 既存、変更なし）。

**Scope（実ファイル）**:
- Rust: `bridge_state.rs`（`object_get_snapshot(state, object_id: String)` / `schema_get_snapshot(state)`）、`lib.rs`（2コマンド登録）、`protocol_names.rs`（`OBJECT_GET_SNAPSHOT`/`SCHEMA_GET_SNAPSHOT` + 配列 + assertion）、`dto.rs`（`ObjectSnapshotPayload`/`PropertyEntryDto`/`SchemaSnapshotPayload`/`TypeDescriptorDto`、camelCase）、`bridge/crates/norves-bridge-editor-client/src/object.rs`（`parse_object_snapshot_result`/`parse_schema_snapshot_result`）+ pub use
- TS: `bridge-types/src/object.ts` + `schema.ts`、`bridge-ui/src/commands.ts`（`objectGetSnapshot:'object_get_snapshot'`, `schemaGetSnapshot:'schema_get_snapshot'`）、`ipc-types.ts`
- React: `PropertyInspectorPanel.tsx`（実装化。選択→snapshot 取得→列挙描画、未選択/空/エラー分岐）、`useBridge.ts`（`getObjectSnapshot(id)`/`getSchemaSnapshot()`。`selectedObjectId` 変化時 fetch する副作用は hook へ）、`store.ts`（`objectSnapshot?`/`schemaTypes?`）
- テスト: Rust parse/コマンド、TS 型/wrapper、React（未選択/空/エラー, jsdom）、`process_e2e.rs`（snapshot ラウンドトリップ, `NORVES_ENGINE_PATH` opt-in）

**Exact deliverables**:
- mock の `getCapabilities` に `object.query` トークンを含め `capabilities.md` に追記（必須）。

**Cross-layer wiring order**: （schema 既存）→ C++(Phase 2 完了) → Rust → protocol_names → TS → React。

**Verification gate**:
```
node scripts/check-protocol-names.mjs
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
cd apps/editor/src-tauri && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test
pnpm -r --if-present typecheck && (cd apps/editor && pnpm test)
python scripts/validate-bridge-fixtures.py   # capability token 追記時
./scripts/verify.ps1
NORVES_ENGINE_PATH=<mock> (cd apps/editor/src-tauri && cargo test -- --nocapture)
```

**Risks & mitigation**:
- *propertyValue 無制限ネスト*: array/object 値は「JSON プレビュー文字列 / 折りたたみ」で安全描画し無制限再帰回避。
- *component-attachment 欠落*（GAP）: schema `typeName` と object `kind` を手動突き合わせ。alpha は単純マッピング、将来 schema 拡張を GAP 記録。
- *選択→fetch レース*: `selectedObjectId` を deps にした effect で最新選択のみ反映（古いレスポンス破棄ガード）。
- *no-lock-across-await*: 厳守。

**Dependencies**: Phase 1（選択）、Phase 2（mock object/schema）、Phase 3（Outliner で選択発火）。

**Opus escalation**: **必要**（Tauri コマンド + async lifetime + DTO/validator）。

**コミット可能条件**: ゲート全通過 + impl-reviewer 承認 + `feature/inspector-readonly` ブランチ。**コミット本文必須**。

---

## Phase 5 — Inspector 編集（`object.setProperty`: write 経路）

**Goal**: Inspector からプロパティ値を編集し `object.setProperty` を送信、`{accepted, appliedValue}` を反映。mock はインメモリ静的マップを更新する。

**Layer(s)**: Rust+Tauri / TS+React（schema 既存、変更なし）。**write 操作・エラーモデルが新規論点**。

**Scope（実ファイル）**:
- Rust: `bridge_state.rs`（`object_set_property(state, object_id, property, value: Value)` → `Map` 組立 → `send_method("object.setProperty", Some(params))`）、`lib.rs`/`protocol_names.rs`（`OBJECT_SET_PROPERTY` + 配列 + assertion）、`error.rs`（GAP: `BackendError::Validation { message }` を additive 追加、serde tag='kind' camelCase union を壊さず既存バリアント温存）、`dto.rs`（`SetPropertyResultPayload { accepted, appliedValue? }`）
- TS: `bridge-ui/src/commands.ts`（`objectSetProperty:'object_set_property'`）+ `ipc-types.ts`、`bridge-types`（`SetObjectPropertyResult`）
- React: `PropertyInspectorPanel.tsx`（編集可能フィールド。valueType に応じた入力 UI。送信→`appliedValue` でローカル反映）、`useBridge.ts`（`setObjectProperty(id, property, value)`）、`store.ts`（appliedValue でスナップショット更新）
- テスト: Rust（setProperty コマンド + Validation エラー）、TS、React（入力→送信→反映、エラー表示）、`process_e2e.rs`（setProperty ラウンドトリップ, `NORVES_ENGINE_PATH` opt-in）

**Exact deliverables**:
- **編集中の中間状態はコンポーネントローカル state に留め、確定時（blur / Enter）のみ `setObjectProperty` を dispatch**（`BridgeStateContext` 単一値配信による全パネル再レンダリングを編集ごとに起こさない）。
- mock の setProperty 状態更新は **single-thread recv loop 前提で安全**（Phase 2 制約と整合）。Phase 6 の `object.changed` emit は更新済みマップを同スレッドで読むため競合しない。**single-thread 前提をコミット本文に明記**。
- capability token `object.edit` を mock `getCapabilities` に追加し `capabilities.md` に追記（必須）。

**Cross-layer wiring order**: （schema 既存）→ C++(Phase 2 で setProperty 更新ロジック完了) → Rust(+error バリアント) → protocol_names → TS → React。

**Verification gate**:
```
node scripts/check-protocol-names.mjs
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
cd apps/editor/src-tauri && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test
pnpm -r --if-present typecheck && (cd apps/editor && pnpm test)
python scripts/validate-bridge-fixtures.py   # capability token 追記時
./scripts/verify.ps1
NORVES_ENGINE_PATH=<mock> (cd apps/editor/src-tauri && cargo test -- --nocapture)
```

**Risks & mitigation**:
- *巨大 appliedValue*（memory-buffer-policy）: mock は echo か小さな正規化値のみ返す。
- *write 副作用の他パネル非伝播*（GAP）: alpha は自パネルローカル反映のみ。多クライアント同期は Phase 6 のライブ更新で対応。
- *undo 不在*: alpha スコープ外（将来）。
- *BackendError union 拡張の互換*: 追加バリアントは additive。TS `BackendError`（store.ts）は `kind?: string` で寛容（裏取り済み）。
- *編集可能 valueType の範囲*: scalar（string/number/boolean）中心を推奨。array/object 編集は Open question 1 でユーザー確認。

**Dependencies**: Phase 4（read-only Inspector が前提）。

**Opus escalation**: **必要**（write 経路 + BackendError モデル + Rust async lifetime + memory ownership）。

**コミット可能条件**: ゲート全通過 + impl-reviewer 承認 + `feature/inspector-edit` ブランチ。**コミット本文必須**。

---

## Phase 6 — ライブ更新イベント（NEW additive @ version 0.2: `scene.treeChanged` / `object.changed`）

**Goal**: poll-only から脱却し、シーン/オブジェクトの変化を push 通知してパネルを自動更新。**完全新規の additive イベント2種を version 0.2 で導入**し、schema+fixtures+validator+全層配線。**本計画で最もプロトコルリスクが高い**。

**NEW protocol events（additive・MINOR=0.2・要 schema+fixtures+validator+tests）**:
1. **`scene.treeChanged`** — `bridge/spec/schema/events/scene.treeChanged.params.schema.json`。params = `{ changedNodes?: sceneNode[], fullRefreshRequired?: boolean }`。`common.schema.json` の `sceneNode` $def 再利用。`additionalProperties:false`。
2. **`object.changed`** — `bridge/spec/schema/events/object.changed.params.schema.json`。params = `{ objectId(required), properties(propertyBag), name?, kind? }`。`objectId`/`propertyBag` $def 再利用。

**送出元の確定**: 両イベントとも **engine wire 側（mock C++ engine）が emit**。`object.setProperty` 受理時の editor backend 合成は採用しない（単一真実源 = engine 状態。将来の実 engine が同じ wire イベントを送れば editor 無改変で動作）。fixture 命名は **`event-engine-valid.json`** で固定。送出元決定は後変更で additive 違反になりうるため本フェーズで凍結。

**version の確定**: 新規イベントは **envelope version "0.2"** で導入。Phase 0 で bump 済みのため新規 fixture の envelope version は "0.2"。schema は version を制約しないため schema 側に追加作業なし。

**Scope（実ファイル）**:
- プロトコル:
  - 上記2 schema 新規
  - fixtures（**envelope version:"0.2"**）: `bridge/spec/fixtures/events/scene.treeChanged/positive/event-engine-valid.json` + `negative/*`、`object.changed/positive/event-engine-valid.json` + `negative/*`
  - `bridge/spec/docs/message-payloads.md`（Events 表）+ `protocol-overview.md`（Events リスト更新 + 0.2 で追加された旨）
  - `bridge/spec/docs/capabilities.md` に `scene.liveUpdate` token 追記（必須。0.1 engine がライブ更新非対応の劣化判定手段）
- C++:
  - `mock_adapter.hpp`（`emit_log_burst` と同じ atomic フラグパターンで「setProperty / 任意トリガ後に object.changed / scene.treeChanged を emit」。**setProperty 更新済みマップを single-thread 同期で読む**ため競合なし）
  - `mock-engine/main.cpp:257-287`（recv ループに ack 後イベント送信追加。`server.emitEvent("object.changed", ...)` / `"scene.treeChanged"`。SendCap=256 を超えないバースト制御）
  - `ws_test_server.cpp` FakeAdapter も同値更新（二重管理）
  - **[BLOCKER] fixtures カウント全更新**（fixture 追加に伴い同一コミットで）:
    - C++: `fixtures_roundtrip_test.cpp:192,195`（positive / payloadOnly / total）
    - Rust: `fixtures_roundtrip.rs` の `fixture_counts_are_exhaustive`（124-135: positive / payload_only / total）**および** `roundtrip_positive_fixtures`(175) / `payload_negative_accepted_at_envelope_layer`(221)。positive 2件 + negative 2件追加なら positive 57→59、payload_only 47→49、total 116→120（**Phase 0 で 116 起点**。実数は追加 fixture 数で確定）。`envelope_negative_rejected`(195) は envelope negative を足さない限り不変。
- Rust:
  - `events_map.rs:14-24`（`"scene.treeChanged" => Some(events::SCENE_TREE_CHANGED)`, `"object.changed" => Some(events::OBJECT_CHANGED)` + テスト）
  - `protocol_names.rs`（`events::SCENE_TREE_CHANGED:'bridge:scene-tree-changed'`, `OBJECT_CHANGED:'bridge:object-changed'` + `events_no_duplicates` 配列 + assertion）
  - relay 本体（`spawn_relay`）は**変更不要**（`ui_channel_for_event` を呼ぶだけ）
- TS/React:
  - `bridge-ui/src/events.ts`（`BRIDGE_EVENTS.sceneTreeChanged`/`objectChanged`）
  - `bridge-types`（event payload 型）
  - `useBridge.ts:108-198`（Promise.all に2購読追加）。**【新規・レビュー反映】`EXPECTED_SUBSCRIPTION_COUNT` を 9→11 へ更新。定義は `useBridge.lifecycle.test.tsx:42`（参照は :81/:82）。`useBridge.test.ts:131` の `toHaveLength(2)` は別テストにつき取り違えない**。
  - `store.ts`（受信時に sceneTree/objectSnapshot を更新する action/reducer）

**Cross-layer wiring order（厳守）**: プロトコル schema+fixtures(v0.2) → validator 通過 → C++ emit + FakeAdapter + roundtrip カウント(C++2+Rust3) → Rust events_map+protocol_names → check-protocol-names → TS events.ts+types+購読数(9→11) → React useBridge + store。

**Verification gate**:
```
python scripts/validate-bridge-fixtures.py        # NEW schema/fixtures(v0.2) + token、合計件数確認
node scripts/check-protocol-names.mjs             # NEW event 名3点同期
ctest --test-dir build/cpp -C Debug --output-on-failure   # C++ roundtrip カウント
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
cd apps/editor/src-tauri && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test
pnpm -r --if-present typecheck && (cd apps/editor && pnpm test)   # EXPECTED_SUBSCRIPTION_COUNT=11 含む
./scripts/verify.ps1 -Cpp
```

**Risks & mitigation**:
- *[BLOCKER] fixtures カウント*: C++ 2箇所 + Rust 3箇所を**同一コミットで更新**（Phase 0 で 116 起点）。実装者チェックリストの最重要項目。
- *`EXPECTED_SUBSCRIPTION_COUNT` 取り違え*: 9→11 は `useBridge.lifecycle.test.tsx:42` の定義1箇所。`useBridge.test.ts:131` の `toHaveLength(2)` とは無関係。
- *envelope `additionalProperties:false`*: イベントは `event` フィールド既定義で envelope 変更不要。params schema の `additionalProperties:false` のみ negative fixture で確認。
- *broadcast Lagged の自動回復は実現しない*: `EVENT_BROADCAST_CAPACITY=256`。Lagged は `spawn_relay` 内で warn+continue され UI 非通知。alpha の保証は「接続確立時 + 選択時に必ず1回 fetch（Phase 3/4）」までとし、ライブイベントは best-effort。mock の treeChanged/object.changed を低頻度に抑えて取りこぼし実質回避。自動再同期は将来フェーズ。
- *FakeAdapter 乖離*: emit 側も同値で。

**Dependencies**: **Phase 0（version 0.2 が前提）**、Phase 3/4（更新対象パネル/ストア）、Phase 5（setProperty→object.changed の連鎖デモ）。

**Opus escalation**: **必須**（NEW protocol schema/compat + C++ public emit + Rust relay + fixtures カウント整合 = 最高 load-bearing）。

**コミット可能条件**: ゲート全通過 + impl-reviewer 承認 + `feature/live-update-events` ブランチ。**コミット本文必須**。fixture 追加のため **C++ 2箇所 + Rust 3箇所のカウント更新がゲート通過の必要条件**。

---

## Phase 7 — Viewport 統合（7a 状態可視化 + 7b サムネイル、両方 IN-SCOPE）

viewport は **「今回やる分」と「将来研究」を厳密に分離**（`docs/viewport-strategy.md` が native embedding / frame streaming を post-alpha と明示）。**今回は 7a + 7b 両方を実施**（ユーザー決定）。

### Phase 7a — Game View の viewport 状態可視化（プロトコル変更なし・即実現）

**Goal**: 既存 `viewportState`（store.ts "No panel renders this yet"）を GameViewPanel に可視化し、状態バッジ（focused/visible/hidden/minimized/unknown）を表示。Outliner 選択と GameView を視覚連動。

**Layer(s)**: React のみ。プロトコル/C++/Rust 変更なし。

**Scope**: `apps/editor/src/components/GameViewPanel.tsx`（`viewportState` を受けて状態表示。dockview 統合後は hook 直呼びで取得）、必要なら placeholder 領域の CSS。

**Verification gate**: `pnpm -r --if-present typecheck` / `cd apps/editor && pnpm test` / `./scripts/verify.ps1`

**Risks**: ほぼなし（表示のみ）。**Opus escalation**: 不要。

**Dependencies**: Phase 1。

**コミット可能条件**: ゲート通過 + impl-reviewer 承認 + `feature/viewport-state-viz` ブランチ。

### Phase 7b — サムネイル（NEW additive method @ version 0.2・要 policy 文書化）— **IN-SCOPE（決定済み）**

**Goal**: engine が低頻度で提供する静止画サムネイルを Game View に表示。**version 0.2 の additive メソッド** `viewport.getThumbnail`（pull型）で実装。memory-buffer-policy の large-payload 戦略を**先に文書化**してから着手。

**確定したデフォルト（memory-buffer-policy 準拠の推奨値）**:
- **取得方式: pull型 `viewport.getThumbnail`**（push の `viewport.frame` イベントは broadcast リングバッファを溢れさせるため不採用）。
- **画像フォーマット: PNG**（無圧縮ロスレスでデモ画質が安定。mock は小さなテスト用 PNG を返す。JPEG は将来オプションとして `mimeType` で切替可能にしておく）。
- **最大解像度: 640×360**（16:9 のサムネイル相当。これ以上は engine 側で縮小）。
- **ハードバイト上限: 256 KiB（base64 後で約 342 KiB）**。**【新規・レビュー反映】根拠は WS フレーム / JSON envelope の実上限**に対して安全であること（pull 型レスポンスは broadcast リングを通らないため `EVENT_BROADCAST_CAPACITY=256`(=リング**段数**であってバイト数ではない)との比較は無関係。混同しない）。超過時は engine が縮小 or エラー。
- **取得頻度: 最大 1 fps（pull 間隔 ≥ 1000ms）**。UI はユーザーがパネルを見ている間のみ低頻度ポーリング。連続ストリーミングはしない（native embedding の領域）。
- これらの数値は **`docs/memory-buffer-policy.md` に追記してからでないと着手不可**（前提条件、後述）。

**前提条件（着手の必要条件）**: `docs/memory-buffer-policy.md` に上記の数値上限（最大解像度 640×360、ハードキャップ 256 KiB と**その WS フレーム/envelope 実上限に対する正当化**、頻度 ≤1 fps、base64 attachment 戦略、pull型の理由）を**先に記述**する。これを満たすまで Phase 7b の実装に入らない。

**NEW protocol（additive @ 0.2・要 schema+fixtures+validator+tests + policy doc + token）**:
- メソッド `viewport.getThumbnail`。params = `{ maxWidth?: number, maxHeight?: number }`。result = `{ imageBase64: string, mimeType: string, width?: number, height?: number }`。base64 は propertyValue=string として envelope 互換（envelope 変更不要）。
- **命名制約**: Tauri コマンド名は `viewport_get_thumbnail`（数字を含めない、`/^[a-z][a-z_]*$/`）。
- capability token `viewport.thumbnail` を `capabilities.md` に追記（必須）。

**Scope（実ファイル）**:
- `docs/memory-buffer-policy.md`（上記数値・WS フレーム上限根拠・attachment 戦略を**先に**記述）
- プロトコル: `bridge/spec/schema/methods/viewport.getThumbnail.{params,result}.schema.json` + fixtures（**envelope version:"0.2"**, positive/negative）+ `capabilities.md`
- **[BLOCKER] fixtures カウント全更新**: method fixture 追加に伴い C++ `fixtures_roundtrip_test.cpp:192-195` + Rust `fixtures_roundtrip.rs`（124-135 / 175 / 221、negative を足すなら 195 も）を**同一コミットで全更新**（Phase 6 完了時点の件数を起点）。
- C++: `adapter.hpp` にオプションメソッド `viewportGetThumbnail`（**非純粋仮想 + `not_supported()` デフォルト**で追加 → 既存アダプタ非破壊、`adapter.hpp:93-120` のオプションブロックに倣う）、mock は小さなテスト用 PNG の base64（256 KiB 上限内）を返す、FakeAdapter 同値、dispatch ルーティング追記
- Rust: `viewport_get_thumbnail` コマンド + protocol_names + DTO + validator
- TS/React: wrapper + 型 + GameViewPanel で `<img src={`data:${mime};base64,${b64}`} />`（低頻度・useMemo で再描画抑制）
- `tauri.conf.json` CSP: 現状 `img-src 'self' data:` を**既に許可済み**（確認済み）→ data URL 表示は **CSP 変更不要**。

**Verification gate**:
```
python scripts/validate-bridge-fixtures.py
node scripts/check-protocol-names.mjs
ctest --test-dir build/cpp -C Debug --output-on-failure
cargo test --workspace
cd apps/editor/src-tauri && cargo test
pnpm -r --if-present typecheck && (cd apps/editor && pnpm test)
./scripts/verify.ps1 -Cpp
```

**Risks & mitigation**:
- *large payload / live memory*: pull型 + 256 KiB ハードキャップ + base64 snapshot で memory-buffer-policy 準拠。**policy 文書化を前提条件**にする。256 KiB の正当化は **WS フレーム/JSON envelope の実上限**に紐付け、broadcast capacity（段数）との混同を避ける。
- *fixtures カウント*: C++/Rust 両系統更新必須。
- *native embedding 誤混入*: HWND reparenting / 共有テクスチャ / 高頻度フレームストリーミングは**含めない**（境界違反）。
- *adapter.hpp 公開 API 追加*: 非純粋仮想 + デフォルトで既存非破壊。

**Dependencies**: **Phase 0（version 0.2）**、Phase 7a。

**Opus escalation**: **必須**（NEW protocol + C++ public API + buffer/memory ownership + policy）。

**コミット可能条件**: ゲート全通過 + impl-reviewer 承認 + `feature/viewport-thumbnail` ブランチ。**コミット本文必須**。fixture 追加のため C++ 4箇所 + Rust 4箇所のカウント更新が必要条件。**`docs/memory-buffer-policy.md` の数値文書化が着手前の必要条件**。

---

## フェーズ依存関係まとめ

```
Phase 0 (protocol 0.1→0.2 bump + 後方互換交渉) ────────────────────────┐
                                                                       │ (新規メッセージは 0.2 で導入)
Phase 1 (docking UI + selection + data-source) ──┬─→ Phase 3 (Outliner) ──→ Phase 4 (Inspector RO) ──→ Phase 5 (Inspector edit)
                                                 │        ▲                        ▲                          │
Phase 2 (mock C++ data + caps token) ────────────┴────────┘────────────────────────┘                          │
                                                                                                              ▼
                                              Phase 6 (live-update events @ 0.2: scene.treeChanged / object.changed)  ← Phase 0
Phase 1 ──→ Phase 7a (viewport state viz) ──→ Phase 7b (thumbnail @ 0.2: viewport.getThumbnail)  ← Phase 0
```
Phase 0 / Phase 1 / Phase 2 は並行着手可能。Phase 3 で 1+2 が合流。Phase 6 と Phase 7b は Phase 0 完了（version 0.2）が前提。

---

## 将来フェーズ（今回スコープ外・明示的にパーク）

- **NorvesLib Bridge アダプタ統合** — generic bridge / C++ SDK は NorvesLib 非依存を維持したまま、将来 NorvesLib を実 engine としてシーン/オブジェクトデータ（および Phase 6 と同じ wire イベント）を供給する adapter 実装。今回は mock engine のみをデータソースとする。
- **Native viewport embedding** — HWND reparenting / 共有 GPU テクスチャ / フルフィデリティのフレームストリーミング / docked render target composition（`docs/viewport-strategy.md` post-alpha）。Phase 7b の静止画サムネイルとは別物。
- **dockview floating / popout window** — Tauri で別ウィンドウ popout が動作する見込みが立った段階で `disableFloatingGroups` を解除。WKWebView ドラッグ挙動の実機検証も含む。
- **`@tauri-apps/plugin-store` によるレイアウト永続化の格上げ** — 今回は localStorage。セッション跨ぎ/複数ウィンドウ要件が出たら plugin-store へ移行。
- **Undo/Redo transaction protocol** — `object.setProperty` の取り消し/トランザクション（alpha 非対応）。
- **Multi-select / gizmos / トランスフォーム操作** — 複数選択モデル、3D ギズモ。今回は単一 `selectedObjectId` のみ。
- **Asset browser** — アセット一覧パネル。
- **大規模シーン対応** — `scene.getTree` の pagination / total node count、コマンド個別タイムアウト、broadcast 容量チューニング、live memory zero-copy。
- **ライブ更新の自動再同期** — broadcast Lagged 取りこぼし時の UI 自律再取得（シーケンス番号/再同期プロトコル）。現状は接続時+選択時の手動 fetch のみ。
- **schema の component-attachment 情報** — どの type がどの object に付くかの記述子（Inspector の型駆動 UI 強化）。
- **viewport push frame streaming (`viewport.frame` event)** — 高頻度フレームを push する設計。今回は pull型 `viewport.getThumbnail` のみ。

---

## ユーザーへの Open questions（product 判断が**まだ**必要な点のみ）

1. **`object.setProperty` の編集対象範囲**: Inspector でどの valueType まで編集 UI を出すか。scalar（string/number/boolean）のみか、array/object も編集可能にするか。**推奨: alpha は scalar 中心**。

> 注: v1 の Open questions のうち、(1) パネルレイアウト = **dockview 導入で確定**、(2) サムネイル = **7b IN-SCOPE で確定**（PNG / 640×360 / 256 KiB / pull型 / ≤1 fps をデフォルト採用）、(3) protocol version = **0.2 MINOR bump で確定**、(5) デモデータ供給源 = **mock engine 一本化・未接続時空状態で確定**。残るのは編集対象 valueType のみ。

---

### 関連実ファイル（絶対パス）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\apps\editor\package.json`（dockview-react 追加）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\apps\editor\src\components\AppLayout.tsx`（DockviewReact へ全面置換、GameViewPanel への 14 props 注入 :58-73）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\apps\editor\src\state\store.ts`
- `C:\Users\KINGkawamura\Documents\NorvesEditor\apps\editor\src\hooks\useBridge.ts`（購読 :108-198）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\apps\editor\src\hooks\useBridge.lifecycle.test.tsx`（EXPECTED_SUBSCRIPTION_COUNT :42 / 参照 :81,:82）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\apps\editor\src\hooks\useBridge.test.ts`（toHaveLength(2) :131、別テスト）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\apps\editor\src-tauri\src\bridge_state.rs`（PROTOCOL_VERSION:58 / build_request:141 / HelloParams:292-301 / インラインテスト:633）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\apps\editor\src-tauri\src\protocol_names.rs`
- `C:\Users\KINGkawamura\Documents\NorvesEditor\apps\editor\src-tauri\src\events_map.rs`（:14-24）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\apps\editor\src-tauri\tests\process_e2e.rs`（version :164,:219 / NORVES_ENGINE_PATH :469）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\apps\editor\src-tauri\tauri.conf.json`（CSP:23、変更不要）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\cpp\engine-sdk\include\norves\bridge\version.hpp`（SupportedProtocolVersions:22-24）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\cpp\engine-sdk\src\server.cpp`（EnvelopeVersion:50 / NegotiateVersion:118-131、handleFrame は version 非検証）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\cpp\examples\mock-engine\mock_adapter.hpp`（capability version :73 は bump 対象外）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\cpp\examples\mock-engine\main.cpp`（:257-287）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\cpp\engine-sdk\tests\ws_test_server.cpp`（FakeAdapter）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\cpp\engine-sdk\tests\fixtures_roundtrip_test.cpp`（カウント 192-195）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\cpp\engine-sdk\tests\smoke_test.cpp`（:64 = 実変更対象）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\cpp\engine-sdk\tests\dispatch_test.cpp`（:219 = 実変更対象 / :125,:347 = 変更しない inbound）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\cpp\engine-sdk\tests\loopback_roundtrip_test.cpp`（:154,:260,:371,:379,:434,:440,:446 = 変更しない inbound/negotiate）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\crates\norves-bridge-core\tests\fixtures_roundtrip.rs`（カウント 124-135 / 175 / 195 / 221）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\crates\norves-bridge-editor-client\src\handshake.rs`（protocol_versions:44-45 / RESPONSE_VALID:234 / assert:367）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\crates\norves-bridge-editor-client\tests\ws_roundtrip.rs`（version() :56 / assert :233 = 温存）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\crates\norves-bridge-editor-client\tests\loopback_roundtrip.rs`（:40,:127,:221 = 温存）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\crates\norves-bridge-editor-client\tests\ws_reconnect.rs`（:49 = 温存）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\crates\norves-bridge-editor-client\tests\conformance.rs`（:53 = 温存 / NORVES_MOCK_ENGINE :302）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\crates\norves-bridge-tools\src\bin\bridge_cli.rs`（:41）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\conformance\runners\alpha_method_sequence.json`
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\spec\fixtures\methods\bridge.hello\positive\`（NEW: request-valid-v02.json / response-valid-v02.json、response-version-unsupported.json 値更新）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\spec\schema\events\`（NEW: scene.treeChanged / object.changed）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\spec\schema\methods\`（NEW: viewport.getThumbnail）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\bridge\spec\docs\protocol-overview.md` / `error-model.md` / `capabilities.md` / `message-payloads.md`
- `C:\Users\KINGkawamura\Documents\NorvesEditor\scripts\check-protocol-names.mjs`（命名正規表現 :56）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\docs\memory-buffer-policy.md`（Phase 7b 前提・数値追記）
- `C:\Users\KINGkawamura\Documents\NorvesEditor\docs\viewport-strategy.md`

**コミット可能条件（全 Phase 共通）**: 該当検証ゲートが実出力付きで全通過し、impl-reviewer（実装者以外）が承認し、`feature/<area>-<summary>` ブランチ上であること。プロトコル schema/fixtures・C++ public API・Tauri permissions・Bridge public API・Rust async lifecycle・protocol version に触れる Phase（0,2,3,4,5,6,7b）は**コミット本文必須**＋`Co-Authored-By` トレーラ必須。フィクスチャを追加/変更する Phase（0, 6, 7b）は **C++ + Rust のカウント更新がゲート通過の必要条件**（Phase 0 で 116 起点、以降累積）。dockview 導入の Phase 1 は `pnpm-lock.yaml` 差分を同一コミットに含める。`main` への直接コミット禁止。