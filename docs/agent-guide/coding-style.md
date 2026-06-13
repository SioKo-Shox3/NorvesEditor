# コーディングスタイル共通規約

NorvesEditor agent guide — 詳細規約。要点と参照は CLAUDE.md（working agreement）にあり、本ファイルはその詳細版。

## コーディングスタイル共通規約

- 新規テキストファイルは UTF-8。行末は LF を基本とする。既存ファイルを編集する場合は既存行末を不要に変更しない。
- 例外: NorvesLib repository 側で編集する C++ ファイルは NorvesLib 側の行末/BOM 規約に従う。
- Generated files は生成元と生成手順を明記する。手編集しない。
- Public API 変更には docs または ADR の更新を伴わせる。
- TODO を残す場合は owner/context を書く。曖昧な `TODO: fix` は不可。
