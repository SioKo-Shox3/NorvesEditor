# オーケストレーション / Workstream / モデル選定

NorvesEditor agent guide — 詳細規約。要点と参照は CLAUDE.md（working agreement）にあり、本ファイルはその詳細版。

## マルチエージェント・オーケストレーション

非自明な作業はメインエージェント（オーケストレーター）が工程を分割し、サブエージェントに割り当てて進める。

### 役割分担の原則

- **オーケストレーター**が担うもの: 工程分割、設計判断、順序付け、スコープ割り当て、サブエージェント結果の統合、検証実行、ブランチ/コミット境界管理、最終受け入れ。
- オーケストレーター自身は **具体的なフェーズ計画の作成・ファイル編集・自分が監督した実装のレビューを行わない**。これらはサブエージェントに割り当てる。
- **実装担当とレビュー担当は必ず別エージェント**にする。
- 各工程は独立してレビュー・検証・コミットできる最小単位にする。
- 各工程は原則として専用ブランチまたは明確にスコープ分離された stacked branch で進める。
- 先行する依存フェーズが実装レビューと検証ゲートを通過するまで、後続フェーズの実装を始めない。

### 工程の流れ

1. **調査** — 既存コード・依存・命名規約の把握。読み取り専用。
2. **計画** — フェーズごとに具体的な実装計画を書く。
3. **計画レビュー** — API 境界、依存方向、所有権、寿命、スレッド安全性、security permission、protocol compatibility、検証コマンドを確認。
4. **実装** — 承認された計画に沿って編集。
5. **実装レビュー** — 実 diff を承認済み計画と突き合わせる。
6. **統合・検証・コミット** — 関連検証を実行し、ブランチ方針とコミット境界に沿ってコミットする。

### 計画に必ず含める項目

```text
- フェーズの目的と期待される挙動変化。
- 影響するモジュール・公開 API・具体的なファイル/ディレクトリ。
- 実装方針。
- 所有権/寿命/スレッド/async task/permission の前提。
- Protocol schema/fixture 変更の有無。
- base branch / 作業ブランチ名 / stacked branch 依存関係。
- 走らせる検証コマンド。
- リスクレベルと封じ込め方針。
- コミット可能になる条件。
```

### 実装レビューで必ず見る項目

```text
- 計画との差分。
- Public API 形状。
- Protocol schema/fixture/test の整合。
- Buffer ownership / thread affinity / async shutdown。
- Tauri permission scope。
- UI state と transport/backend state の分離。
- C++ SDK が NorvesLib や third-party WebSocket types を漏らしていないか。
- Generated/build files が混ざっていないか。
- 作業ブランチのスコープが計画と一致しているか。
- main / develop に直接作業を積んでいないか。
- 検証コマンドの実行結果。
```

---

## Workstream 運用

Workstream は実装単位ではなく、計画領域です。作業開始時は該当 Workstream をさらに小さい Phase に分割する。

現在の Workstream:

```text
A. Repository foundation
B. Product architecture and ADRs
C. Protocol specification and fixtures
D. Rust bridge core and editor client runtime
E. TypeScript UI-facing bridge package
F. C++ engine-side SDK
G. WebSocket transport
H. Mock engine and conformance tools
I. Tauri app shell
J. Engine process lifecycle and external viewport control
K. Game View alpha panel
L. NorvesLib reference adapter
M. Documentation and developer experience
N. Post-alpha foundations
```

Workstream をまたぐ変更は原則として計画レビュー必須。

---

## モデル選定

判断の質が後工程全体を左右する役割（計画・レビュー＝品質保証）に、その時点で利用可能な最上位モデルを使い、手を動かす分割タスクでコストを下げる。

| 役割 | モデル方針 |
|------|-----------|
| オーケストレーター | 最上位〜上位。判断の質を優先。 |
| 計画作成 | 必ず最上位。 |
| 計画レビュー | 必ず最上位。 |
| 実装レビュー | 必ず最上位。 |
| 実装 | 既定は下位。難所では上位〜最上位へ昇格。 |
| 調査・機械的作業 | 最安で十分。設計理解を伴う調査は中位以上。 |

昇格条件:

```text
- Protocol schema / compatibility
- Tauri process/security permissions
- Rust async task lifecycle
- WebSocket transport / reconnect
- C++ SDK public API
- Buffer ownership / memory policy
- Thread affinity / engine main-thread marshaling
- NorvesLib adapter
- Viewport strategy
```

---

## Alpha acceptance checklist

Alpha 作業の最終確認:

```text
- AGENTS.md と CLAUDE.md が一致している。
- Tauri app が起動する。
- Mock engine がビルドできる。
- Editor から mock engine を launch できる。
- Editor backend が WebSocket + JSON で接続できる。
- Game View panel が process/connection/runtime state を表示する。
- Launch / Stop Process / Reconnect / Play / Pause / Stop が操作できる。
- Log panel が log.message event を表示する。
- Protocol fixtures/conformance tests が通る。
- Tauri security permission が最小化されている。
- Known limitations と post-alpha viewport plan が docs にある。
```
