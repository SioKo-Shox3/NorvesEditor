# C++ Engine SDK 規約

NorvesEditor agent guide — 詳細規約。要点と参照は CLAUDE.md（working agreement）にあり、本ファイルはその詳細版。

## C++ Engine SDK 規約

対象:

```text
bridge/cpp/engine-sdk/**
bridge/cpp/examples/**
```

規則:

```text
- Standalone C++ SDK は NorvesLib に依存しない。
- C++23 を採用する（ADR 0008 で承認）。NorvesLib と言語標準を揃えるための決定。
- CMake target を明確に分ける。
- Public headers で third-party WebSocket types を露出しない。
- Public API は RAII と明示的 ownership を基本にする。
- Engine adapter callbacks は thread affinity を明示する。
- Engine state mutation は engine/main thread へ marshal できる設計にする。
- Raw engine memory を protocol/transport に直接送らない。
- Allocator / buffer pool hook は optional にし、未指定なら default allocator で動作する。
- Tests/examples 以外で `std::cout` に依存した logging をしない。SDK logging は callback or sink interface にする。
```

C++ WebSocket backend は internal implementation detail。IXWebSocket、Boost.Beast、libwebsockets 等の候補から ADR で決定する。決定前に public API に特定ライブラリの型を混ぜない。

## Naming & formatting (NorvesLib-aligned)

スタイル（整形・命名・コメント）のみを NorvesLib に揃える。**型ライブラリ規約は採用しない**: std 型・`Result<T, E>`・pImpl・no-exceptions を維持し、NorvesLib 独自の型/マクロは混入させない（ADR 0008）。

整形:

```text
- 波括弧は Allman（開き括弧を次行）。単一文の制御フローでも `{}` を必須にする。
- インデントは 4 スペース。タブは使わない。
- ポインタ/参照は型側に付ける（`Type* p` / `Type& r`）。
- ヘッダガードは `#pragma once` を使う（旧来の include guard は禁止）。
- ヘッダ内で `using namespace` を書かない。
- namespace は `Norves::Bridge`（PascalCase）を使う。NorvesLib の
  `NorvesLib::Core` などのネスト名前空間規約に合わせる（ADR 0009）。
```

命名:

```text
- メンバ変数は `m_PascalCase`（bool 用途は `m_b` 接頭辞、例 `m_bConnected`）。
- ローカル変数・引数は camelCase。
- 内部リンケージ（無名 `namespace {}` 内）の自由関数は PascalCase。
- enum 型・列挙子は PascalCase。
```

コメント:

```text
- コメントは日本語 Doxygen 形式（`@brief` / `@param` / `@return` / `@note`）。
- ただし thread affinity / ownership / lifetime / never-valueless /
  「Rust 実装の 1:1 移植」等の規範記述は、意味を一字一句保全したうえで翻訳する。
  情報を落としたり要約で済ませたりしない。
```

整形/命名ツール:

```text
- 整形は `bridge/cpp/.clang-format`、命名は `bridge/cpp/.clang-tidy` を使う
  （clang-format 15+ 必須。当開発環境は LLVM 19）。
- `bridge/cpp/third_party/` は対象外（`third_party/.clang-format` で DisableFormat）。
- `.clang-tidy` は内部識別子の命名のみを強制する。後述の凍結公開 API は対象外で、
  `m_b` 接頭辞や `I`/`T` のクラス接頭辞は clang-tidy では表現できないためレビューで担保する。
```

### Frozen public API (cross-repo contract)

公開 API シンボル名は**凍結**する（ADR 0008、オプション A）。理由: NorvesLib 側の `NorvesLibBridgeAdapter` / `BridgeServerHost` がこの SDK の公開 API を直接消費しており、シンボルを改名すると別リポジトリ（NorvesLib）へ波及して無改修ビルドが壊れるため。clang-tidy の `--fix` でこれらが改名されないよう、上記命名規則は内部識別子のみに適用する。
namespace 修飾子は本節の凍結対象に含めない。ADR 0009 で `Norves::Bridge` へ意図的に変更済みであり、本節が凍結するのは method 名・型名の綴りのみである。namespace 変更に伴う NorvesLib 側の include path / 修飾子の書き換えは、ADR 0009 が定める lockstep 手順で許容される。

凍結対象（改名・並べ替え禁止）:

```text
1. `IBridgeEngineAdapter` の基底クラス名と仮想メソッド。完全なメソッド一覧は ADR 0009 を正典とする。
2. `BridgeEngineServer` とメソッド handleFrame / emitEvent、コンストラクタのシグネチャ。
3. `ITransport` とメソッド recv / send / close。
4. 公開自由関数 make_websocket_server_transport / make_loopback_pair /
   to_wire / *_from_wire。
5. 公開メンバ関数（`Result` / `JsonValue` 等）is_ok / is_err / value / error /
   ok / err / parse / dump / is_null 等 — snake_case のまま凍結する。
6. DTO public struct のメンバ名「および順序」（wire キーと一致し、集約初期化で
   位置依存のため並べ替えも禁止）、各 `to_json`。
7. enum 型名・列挙子名、`BridgeError` 型、公開型エイリアス。
```

注記: 公開定数（`kSdkVersion` 等）の `k` 接頭辞除去は、NorvesLib から参照されていないことを `git grep` で確認した場合に限り可とする。
