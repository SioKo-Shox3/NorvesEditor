# コーディングスタイル共通規約

NorvesEditor agent guide — 詳細規約。要点と参照は CLAUDE.md（working agreement）にあり、本ファイルはその詳細版。

## コーディングスタイル共通規約

- 新規テキストファイルは UTF-8。行末は LF を基本とする。既存ファイルを編集する場合は既存行末を不要に変更しない。
- 例外: NorvesLib repository 側で編集する C++ ファイルは NorvesLib 側の行末/BOM 規約に従う。
- C++ Bridge SDK（`bridge/cpp/`）のソースは UTF-8・LF を採る（EOL は本リポジトリ規約に従い LF。NorvesLib の CRLF は不採用）。
  - **公開ヘッダ（`bridge/cpp/engine-sdk/include/**`）には UTF-8 BOM を付与する**（バイト列 `EF BB BF`、改行は LF のまま）。理由: 公開ヘッダは `/utf-8` を付与しない別リポジトリ（NorvesLib の Game ターゲット等）からもコンパイルされ、BOM がないと MSVC が BOM なし UTF-8 を CP932 と誤認して C4819 警告 → 日本語バイト誤デコードで C2447 等のビルド破壊を起こすため（NorvesLib が UTF-8+BOM を採るのと同じ理由）。BOM 付与は `[System.IO.File]::ReadAllBytes`/`WriteAllBytes` で行い、`Set-Content`/`Out-File` は CRLF や別 BOM を混入させ得るため使わない。
  - SDK 内部 TU（`src`/`tests`/`examples` の `.cpp`、内部ヘッダ `src/json_value_impl.hpp`・`tests/test_support.hpp`）は SDK 自身の `/utf-8` ビルド（`bridge/cpp/CMakeLists.txt` の `if(MSVC)`）でのみコンパイルされ cross-repo 消費されないため、BOM は不要（BOM なし UTF-8・LF）。`third_party/` も対象外。
- Generated files は生成元と生成手順を明記する。手編集しない。
- Public API 変更には docs または ADR の更新を伴わせる。
- TODO を残す場合は owner/context を書く。曖昧な `TODO: fix` は不可。
