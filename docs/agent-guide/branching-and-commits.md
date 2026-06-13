# ブランチ運用 / コミットメッセージ

NorvesEditor agent guide — 詳細規約。要点と参照は CLAUDE.md（working agreement）にあり、本ファイルはその詳細版。

## ブランチ運用

NorvesEditor では、`main` や単一の長期作業ブランチに直接作業を積み続けない。Workstream / Phase / 修正単位ごとにブランチを切り、レビュー可能な単位で統合する。

### 基本方針

```text
main:
  常にビルド可能な安定ブランチ。直接コミットしない。

integration branch:
  必要な場合だけ設ける短〜中期の統合ブランチ。
  develop 一本運用の代替として常用しない。

work branch:
  通常の作業ブランチ。実装・修正・文書化はここで行う。
```

非自明な作業では、調査・計画の後、実装に入る前に作業ブランチを作成する。小さな typo 修正や README の軽微な追記でも、リモート運用がある場合は原則として専用ブランチを使う。

### ブランチ命名

ブランチ名は英小文字の kebab-case を基本とし、作業種別と対象を含める。

```text
feature/<area>-<summary>
fix/<area>-<summary>
docs/<area>-<summary>
chore/<area>-<summary>
refactor/<area>-<summary>
spike/<area>-<summary>
```

例:

```text
feature/bridge-message-envelope
feature/editor-game-view-alpha-panel
fix/bridge-reconnect-timeout
docs/architecture-adr-bootstrap
chore/repository-foundation
spike/native-window-focus
```

Workstream に紐づく作業では、必要に応じて area に Workstream 名を反映する。

```text
feature/protocol-fixtures
feature/rust-bridge-client-runtime
feature/cpp-engine-sdk-skeleton
feature/tauri-process-lifecycle
```

### ブランチ作成・更新ルール

- 作業ブランチは最新の `main`、またはオーケストレーターが明示した統合ブランチから切る。
- 既存ブランチで別目的の作業を始めない。目的が変わったら新しいブランチを切る。
- 1 ブランチ 1 論理テーマを守る。複数 Workstream にまたがる変更は、計画レビューで分割方針を決める。
- 長期化したブランチは定期的に基点ブランチへ追従する。rebase / merge の選択は作業状況に応じて計画または PR に記録する。
- 他者または他エージェントが使っている共有ブランチを無断で rebase / reset / force-push しない。
- 個人・エージェント専用ブランチであっても force-push は `--force-with-lease` を使い、理由を記録する。
- `spike/` ブランチは検証用。採用する場合は、結果を整理して `feature/` ブランチへ移すか、明確なコミット列に整えてから統合する。

### PR / 統合ルール

GitHub を使う場合、作業ブランチは PR または Draft PR を通して統合する。

PR には最低限以下を書く。

```text
- 目的
- 対象 Workstream / Phase
- 変更内容
- 実行した検証コマンド
- 未解決リスク / 既知の制限
- UI 変更がある場合はスクリーンショットまたは確認ログ
- protocol / security / memory ownership / thread affinity に影響がある場合はその説明
```

統合前の最低条件:

```text
- 関連する format / lint / build / test が通っている。
- AGENTS.md と CLAUDE.md の差分がない。
- 生成物や build output が含まれていない。
- Workstream / Phase の Done criteria を満たしている。
- 必要なレビューが完了している。
```

### マルチエージェント時のブランチ規約

- 実装担当エージェントごとに作業ブランチまたは worktree を分ける。
- サブエージェント間で同じファイルを同時編集しない。共有ファイルは単一オーナーを決めるか、オーケストレーターが順次統合する。
- 計画レビュー・実装レビュー担当は原則 read-only で確認し、直接 push しない。
- オーケストレーターはフェーズ完了時に branch status、未コミット差分、検証結果、push 状態を確認する。
- 途中で別ブランチへ切り替える場合は、現在の差分を commit / stash / discard のどれで扱ったかを明示する。

### 禁止事項

```text
- `main` への直接コミット。
- `develop` 一本に全作業を積み続ける運用。
- 目的の違う変更を同じブランチに混ぜること。
- レビュー前の大規模一括統合。
- 他者の未統合コミットを無断で rewrite すること。
- build output / generated noise を含んだまま PR すること。
```


## コミットメッセージ

「正しいコミットメッセージとは何か」を毎回考えて書く。テンプレ的な一文で済ませない。

- **言語**: 主に日本語。命令形で「何をしたか」を書く。
- **粒度**: 1 コミット 1 論理変更。無関係な変更を混ぜない。
- **件名**: 対象と意図を具体的に表す。曖昧語（「修正」「更新」単体）で終わらせない。
- **本文**: 自明でない変更には本文を付け、なぜ必要か、何をどう変えたか、影響範囲を書く。
- 本文必須の変更:
  - protocol schema / fixture
  - Tauri security permissions
  - process launch / kill behavior
  - Bridge public API
  - C++ SDK ownership / buffer / thread rules
  - Rust async task lifecycle
  - NorvesLib adapter
  - viewport strategy
- オーケストレーション作業ではコミットを工程境界に合わせる。
- リモート設定が許せばコミット後に現在の作業ブランチをプッシュする。main / develop への直接 push は原則行わない。
