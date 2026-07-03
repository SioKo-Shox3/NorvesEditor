# オーケストレーション / Workstream / モデル選定

NorvesEditor agent guide — 詳細規約。要点と参照は CLAUDE.md（working agreement）にあり、本ファイルはその詳細版。

## マルチエージェント・オーケストレーション

非自明な作業はメインエージェント（オーケストレーター）が工程を分割し、サブエージェントに割り当てて進める。

### 役割分担の原則

- **オーケストレーター**が担うもの: 工程分割、設計判断、順序付け、スコープ割り当て、サブエージェント結果の統合、検証実行、ブランチ/コミット境界管理、最終受け入れ。
- オーケストレーター自身は **具体的なフェーズ計画の作成・ファイル編集・自分が監督した実装のレビューを行わない**。これらはサブエージェントに割り当てる。
- **実装担当とレビュー担当は必ず別エージェント**にする。
- **実装は既定で Codex に委譲する**（codex プラグイン / `codex:rescue`。Codex は使用量に余裕があるため、実装・機械的作業は積極的に Codex へ振り分け、Claude の枠は判断とレビューに温存する）。Claude の `implementer` はフォールバック（Codex 不調・引き継ぎコストが勝る小修正）。
- **レビューは Claude＋Codex のダブルチェック**: 計画・実装とも一次＝最上位 Claude（実装者と別エージェント）、二次＝独立した Codex。自己レビュー禁止（実装した本人に自分の変更をレビューさせない）。
- 各工程は独立してレビュー・検証・コミットできる最小単位にする。
- 各工程は原則として専用ブランチまたは明確にスコープ分離された stacked branch で進める。
- 先行する依存フェーズが実装レビューと検証ゲートを通過するまで、後続フェーズの実装を始めない。

### 工程の流れ

1. **調査** — 既存コード・依存・命名規約の把握。読み取り専用。
2. **計画** — フェーズごとに具体的な実装計画を書く。
3. **計画レビュー** — API 境界、依存方向、所有権、寿命、スレッド安全性、security permission、protocol compatibility、検証コマンドを確認。一次（最上位 Claude）＋独立した Codex 二次のダブルチェック。
4. **実装** — 承認された計画に沿って編集。**既定で Codex に委譲**（対象フェーズ・目的・許可/禁止の書き込みパス・レイヤー規約・期待する報告形式を明示して渡す）。Codex 出力は提案物として `git diff --stat` → 実 diff の順で検査してから受け入れる（スコープ逸脱に注意）。
5. **実装レビュー** — 実 diff を承認済み計画と突き合わせる。一次（最上位 Claude、実装者と別）＋クリーン文脈の Codex 二次。両者の指摘を突き合わせてから次へ。
6. **統合・検証・コミット** — 関連検証を実行し、ブランチ方針とコミット境界に沿ってコミットする。

### サブエージェントの起動方法（定義と実行）

6 つの役割は `.claude/agents/` に定義する（frontmatter の `name` / `description` /
`tools` / `model` ＋ 役割本文）:

```text
researcher / planner / plan-reviewer / implementer / impl-reviewer / verifier
```

起動は次の優先順で行う:

1. **標準パス**: Agent ツールの `subagent_type` に役割名（`researcher` 等）を渡す。
   ランタイムが project の `.claude/agents/` を subagent type として公開している場合
   はこれを使う。`model` は定義ファイルの frontmatter が適用される。
2. **フォールバック**: ランタイムが `.claude/agents/` を subagent type として公開せず
   `subagent_type: <役割>` が "Agent type not found" になる場合は、`general-purpose`
   を起動し、プロンプト冒頭で「まず `.claude/agents/<役割>.md` を読み、その役割定義を
   完全に引き受けよ」と指示する。あわせて Agent ツールの `model` パラメータを
   下記モデル方針どおりに明示指定する（定義ファイルの `model` が効かないため）。

どちらの経路でも **実装担当とレビュー担当・計画担当と計画レビュー担当は必ず別エージェント**
にする原則は変わらない。read-only 役割（researcher/planner/plan-reviewer/impl-reviewer/
verifier）には編集系ツールを渡さない。

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

### ハーネスによる強制（2026-07-03 展開）

- `.claude/hooks/enforce-codex-impl.mjs`（PreToolUse）が、メインスレッドによる実装ソース（`.rs/.ts/.tsx/.js/.cpp/.h` 等）の `Edit`/`Write` を物理的にブロックする。実装は Codex／サブエージェントへ。文書・設定・protocol fixtures はブロックされない。
- `.claude/hooks/session-start-reminder.mjs`（SessionStart）が毎セッション冒頭にこの方針を注入する。
- 意図的な一時解除（ユーザー承認必須・1 セッション限り）: 環境変数 `NORVESEDITOR_ALLOW_DIRECT_EDIT=1` を設定して再起動。
- `.claude/settings.local.json` はマシンローカル（フック配線を含む）。別マシンではリポジトリ外の `../claude-workflow-template/deploy.ps1` で再展開する。

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

モデルはバージョン付きIDで固定しない（2026-07-04 設定）: 品質役 = `model: inherit`（メインセッションに追従。メインは `/model` で常に最上位を選ぶ）、作業役 = `sonnet`・`haiku` エイリアス。同一フェーズで反証・手戻りが2回続いたら回数を重ねず、上位モデルへ昇格するかユーザーに相談する。

| 役割 | モデル方針 |
|------|-----------|
| オーケストレーター | 最上位〜上位。判断の質を優先。 |
| 計画作成 | 必ず最上位。 |
| 計画レビュー | 必ず最上位。 |
| 実装レビュー | 必ず最上位。 |
| レビュー二次（ダブルチェック） | Codex（独立した別エージェント）。 |
| 実装 | **既定で Codex に委譲**（使用量に余裕）。Claude 実装時は下位、難所では上位〜最上位へ昇格。 |
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
