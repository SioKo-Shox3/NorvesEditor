# TASKS — NorvesEditor

M2 ループが消化する機能一覧。M1(対話設計)でユーザーと合意してから書く。1タスク = 1反復で閉じる大きさ
(計画→実装→検証→コミットが1回で終わる)。閉じないと分かったら分割して行を増やす。
`status` は `todo | doing | done | blocked`。`done` へのフリップは検証出力を開いた後でしか許されない(verify-gate)。

主題: **コンポーネント縦断** — エンジンのコンポーネント(Camera / SpringArm / Mesh / SkinnedMesh /
RigidBody / Collider / Script …)をエディタから見て直せるようにする。合意した発見経路は
`object.getSnapshot` の result への optional `components` 追加(Inspector 側に一覧、Outliner の
「ノード = Entity」意味論は変えない)。編集は既存 `object.setProperty` が `component:<entityId>:<componentId>`
を受けるので追加メソッドは作らない。

## T-001: object.getSnapshot の result に optional components を足す(spec + fixture)
- status: done
- done-when: `object.getSnapshot.result.schema.json` が optional `components:[{objectId,kind}]`(`additionalProperties:false`)を持ち、components 付き positive fixture と 未知フィールド negative fixture が追加され、fixture 総数が C++ 4 箇所・Rust 4 箇所のハードコードと一致する。`message-payloads.md` に欄と意味を書く。
- verify: `python3 scripts/validate-bridge-fixtures.py`
- verify: `cargo test -p norves-bridge-core --test fixtures_roundtrip`
- paths: bridge/spec/**, bridge/crates/norves-bridge-core/tests/**, bridge/cpp/engine-sdk/tests/fixtures_roundtrip_test.cpp
- notes: 既存 160 fixture は壊さない(additive)。components 欄が無い既存 fixture も通ること自体を回帰として残す。

## T-002: mock engine が components を返す(C++)
- status: done
- done-when: mock の `objectGetSnapshot` が、デモツリーのエンティティに対し `component:<id>:<cid>` 形式の components を返し、その component id を `objectGetSnapshot` / `objectSetProperty` に渡すと解決できる。ctest が通る。
- verify: `cmake --build build/cpp -j4`
- verify: `ctest --test-dir build/cpp --output-on-failure`
- paths: bridge/cpp/examples/mock-engine/**
- notes: mock の状態はインメモリ静的マップ・single-thread recv loop 前提(マルチスレッド化しない)。

## T-003: Rust 側で components を厳格にパースする
- status: done
- done-when: `object.getSnapshot` の DTO が optional `components` を `deny_unknown_fields` のまま受け、欠落は `None`、要素の欠けたフィールド/未知フィールドは拒否する。Tauri コマンドは wire の値をそのまま UI へ渡す。
- verify: `cargo test --workspace`
- verify: `cargo test --manifest-path apps/editor/src-tauri/Cargo.toml`
- verify: `cargo clippy --workspace --all-targets -- -D warnings`
- paths: bridge/crates/norves-bridge-editor-client/src/**, apps/editor/src-tauri/src/**, apps/editor/src-tauri/tests/**
- notes: components を持たないエンジン(0.2 の既存実装)でも接続が落ちないことを回帰で固定する。

## T-004: TS の型と store に components を通す
- status: done
- done-when: `bridge-types` に component 記述子の型があり、store が選択エンティティの components を保持し、セッション変更/切断で消える。型検査とテストが通る。
- verify: `pnpm -r --if-present typecheck`
- verify: `pnpm exec vitest run bridge/ts/packages/bridge-ui apps/editor/src/state`
- paths: bridge/ts/packages/**, apps/editor/src/state/**
- notes: 世代は既存どおり `sessionId`。Rust の内部世代番号は露出しない。

## T-005: Property Inspector にコンポーネント一覧と編集を出す
- status: done
- done-when: エンティティ選択時に Inspector がコンポーネント一覧を表示し、選択したコンポーネントのプロパティを読み、編集が `object.setProperty` に component id 付きで飛ぶ。components を返さないエンジンでは一覧セクションごと出ない(エラー表示にしない)。
- verify: `pnpm exec vitest run apps/editor/src/components apps/editor/src/hooks`
- verify: `pnpm -r --if-present typecheck`
- paths: apps/editor/src/components/**, apps/editor/src/hooks/**
- notes: 既存の Entity プロパティ編集と Undo/Redo(U2)の経路を壊さない。

## T-006: NorvesLib 側の結線契約を固定する(作業機向け)
- status: done
- done-when: `docs/norveslib-integration.md`(または新規の契約文書)に、アダプタの (1) component target の ScriptComponent 特例撤去と `BuildObjectSnapshot(const Object&)` への統一、(2) `objectSetProperty` の汎用プロパティ適用経路への統一、(3) Entity snapshot への components 付与、の3点が、受け入れ条件と検証コマンド付きで書かれている。
- verify: `node scripts/check-protocol-names.mjs`
- paths: docs/**
- notes: NorvesLib は XAudio2 / Fiber / windows.h 依存で Linux では構築できない。実装と検証は作業機(Windows)で行う。
