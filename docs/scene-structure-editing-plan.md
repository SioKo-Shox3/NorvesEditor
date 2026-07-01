# シーン構造編集 実装計画 — NorvesLib 結線 + rename / duplicate / Undo-Redo

> 状態: **A / A2 / B / C 完了 = シーン構造編集(create/delete/reparent/rename/duplicate)完成**。次は Undo/Redo または名前空間リファクタ。
> - Phase A（プロトコル + 汎用 SDK not_supported フック + エディタ UI）: NorvesEditor
>   `main` にマージ済み（`3bb092b` / merge）。
> - Phase A2（NorvesLib アダプタ結線）: NorvesLib `develop` にマージ済み
>   （`2fe4e802` / merge `22a9ce8b`）。impl-reviewer **APPROVE**、ビルド(Game)/
>   ctest(EntityTreeTest 1/1)/EOL flip 検査すべて緑。
> - Phase B（rename: Entity Name + AppendEntityNode 露出、object.setProperty 再利用）:
>   NorvesLib `develop` にマージ済み（`93b4491d` / merge `ba71dd15`）。impl-reviewer
>   **APPROVE**、Game ビルド / EntitySubtreeSnapshotTest 1/1 / fixtures 147 /
>   cargo scene:: 15 passed / EOL flip 検査すべて緑。spike は GO（serialize は name 由来
>   StableId キーで back-compat 安全、Container::String は全経路対応）。
>   既知の後続: rename 後の Outliner ラベル即時更新（object.setProperty は
>   scene.treeChanged 非 emit、Inspector は即時反映）。
>
> - Phase C1（duplicate scaffolding: protocol/SDK/Rust/UI、mock not_supported）:
>   NorvesEditor `main` にマージ済み（`76fd9e1` / merge `bd5aa77`）。fixtures 147→153。
>   impl-reviewer **APPROVE**、全ゲート緑（fixtures 153 / commands 25 / cargo 両 workspace /
>   typecheck / vitest 380 / ctest 7/7）。
> - Phase C2（duplicate 実エンジン結線: prefab round-trip、sibling via GetParentEntity）:
>   NorvesLib `develop` にマージ済み（`6e1e3a47` / merge `61b11dcf`）。impl-reviewer
>   **APPROVE**、Game ビルド / EntityTreeTest 1/1 / EOL flip 検査すべて緑。
>
> 本計画は planner + plan-reviewer（別エージェント）を経たレビュー済み計画で、
> plan-reviewer 評定は **APPROVE-WITH-FIXES**（BLOCKER なし）。下記は SHOULD-FIX /
> NICE-TO-HAVE を反映済み。

対象リポジトリは 2 つ。A2 の編集は **NorvesLib 側のみ**（NorvesEditor は無変更で
E2E が閉じることを検証で示す）。名前空間 `norves::bridge` は現状維持
（`Norves::Bridge` へのリネームは最後・本計画の範囲外）。

---

## Phase A2 — createObject / deleteObject / reparentObject を NorvesLib アダプタに結線（build-ready・最優先）

### 目的
`NorvesLibBridgeAdapter` は 3 メソッドを override しておらず SDK 既定
`not_supported()`（`adapter.hpp:104-120`）で `METHOD_NOT_SUPPORTED` を返す。
エディタはこの応答で `sceneEditUnsupported` を立て 追加/削除/rootへ移動 ボタンが
凍結する。本フェーズで 3 メソッドを実装し `World` を実際に変更、`scene.treeChanged`
を emit する。結果として 3 ボタンが実エンジン上で機能する。

### 影響ファイル（NorvesLib のみ）
- `Game/Bridge/NorvesLibBridgeAdapter.h` — 3 override 宣言追加（`objectSetProperty`
  宣言直後 `.h:259-260` 付近）。署名は `IBridgeEngineAdapter` の
  `Result<JsonValue, BridgeError> sceneCreateObject/DeleteObject/ReparentObject(const JsonValue& params)`
  （非 const。`adapter.hpp:105-120`）に一致させる。
- `Game/Bridge/NorvesLibBridgeAdapter.cpp` — 3 本体 + 共有ヘルパ
  `EmitSceneTreeChanged` + `getCapabilities` の token 追加。
- `Test/Core/Object/EntityTreeTest.cpp` — World レベルのギャップ補完テスト 1 本。
- **World.h / World.cpp は編集しない**（ミューテーション API は public 実装済み。
  `World.h:56-95, 169-178`）。CMake 変更も不要（既存 `EntityTreeTest` に関数追加のみ）。

### 実装方針
共通の下敷きは `objectSetProperty`（`.cpp:1742-1902`）: `params.dump()` →
`extract_string_field`（`.cpp:670`）でフィールド読取 → `std::strtoull`（全桁数字要求）
→ `FindEntityByObjectId`（`.cpp:566-582`）で逆引き → `GEngine`（`.cpp:1785`）→
`GetWorld()` を取りゲームスレッド上で同期ミューテート。**mutex/marshalling なし**
（全コールバックは `BridgeServerHost::DrainInbound`＝ゲームスレッド同期実行。
ロックは 1 フレーム pop 区間のみで `handleFrame` はロック外。`BridgeServerHost.cpp:193-204`）。

**共有ヘルパ `EmitSceneTreeChanged(BridgeServerHost* host)`**（`EmitRuntimeStateChanged`
`.cpp:159-181` に倣う）: `host==nullptr` なら no-op、`JsonValue::parse(R"({"fullRefreshRequired":true})")`
を作り `host->EmitEvent("scene.treeChanged", value)`。payload は制御下リテラルのみ
（値コピー、live pointer 不搬送。memory-buffer-policy）。`{"fullRefreshRequired":true}`
は `scene.treeChanged.params.schema.json`（両 property optional・`additionalProperties:false`）
に適合。`EmitEvent` はキュー再ロック・`handleFrame` 再入・`DrainInbound` 再帰を
起こさない（`BridgeServerHost.cpp:275-292` で確認）ため、コールバック途中の emit は安全。

- **createObject**: `parentId`（任意）を読む。指定ありで解決不可 → `{"accepted":false}`。
  省略時は root 生成（親 = nullptr）。`kind` は **MVP では無視**（動的型 × 親付き
  生成の public API が無い。`AttachChildEntity` は private `World.h:247`）→ 常に
  `world.SpawnEntity<Entity>(parent)`。戻りが非 null → `{"accepted":true,"newId":to_string(entity->GetObjectId())}`
  （`GetObjectId()` は `uint64_t`。Entity.h:148。新 ObjectId は `AssignFreshObjectIdsRecursive`
  で採番）。null → `{"accepted":false}`（World 未初期化 `OF_Initialized` 無しで
  `SpawnEntity` が null を返すケース `World.h:61-64` もこのフォールバックが吸収）。
  成功時のみ返却前に `EmitSceneTreeChanged`（`runtimePlay` が stateChanged を Ack 前に
  emit する順序に合わせる。`.cpp:1397-1403`）。
- **deleteObject**: `objectId`（必須）解決。欠落/パース不可/GEngine null/該当なしは
  `{"accepted":false}`。`world.RemoveEntity(entity)`（子孫連鎖削除。`World.h:169-171`）の
  bool を `accepted` に反映。true のみ emit。
- **reparentObject**: `objectId`（必須）解決。`newParentId`（任意）省略時 nullptr（root へ
  移動 `World.h:175-176`）、指定ありで解決不可 → `{"accepted":false}`（親不明で root へ
  落とすのは誤挙動になりうるため reject）。`world.ReparentEntity(entity, newParent)`（cycle
  拒否は内部処理済み。`EntityTreeTest.cpp:563`）の bool を反映。true のみ emit。

**エラー方針**: 3 メソッドとも失敗系（id 不明・パース不可・GEngine null・ミューテート失敗）は
**エラー Result ではなく `{"accepted":false}` の成功 Result** を返す（`objectSetProperty` の
graceful reject `.cpp:1744-1746` と一貫。3 result schema すべてで `accepted` のみ required、
createObject は `newId` 省略可）。`METHOD_NOT_SUPPORTED` を新設しない（返すと
`sceneEditUnsupported` が latch しボタンが凍結する。`useBridge.ts:602-603`）。

### capability（plan-reviewer SHOULD-FIX #1 反映）
`getCapabilities`（`.cpp:1328-1348`）の配列に **`scene.edit`**（本フェーズが実装する
編集能力の正しい広告。`capabilities.md:39,75-81` が create/deleteObject/reparentObject 用に
既定義）**と `scene.liveUpdate`**（本フェーズで `scene.treeChanged` を実際に emit する
ため。`capabilities.md:83-91`）の 2 token を追加。`.cpp:1335-1336` の「範囲外」コメントを更新。
capability 検証は superset 方針（`.cpp:1337`）で、NorvesLib アダプタの capability セットを
exact-match で pin する golden/conformance テストは両リポに存在しない（conformance の
exact-match は mock の 8 token 対象で本計画は mock 不変）→ token 追加は build/test を壊さない。

### テスト（plan-reviewer SHOULD-FIX #2 反映）
- **World レベル**: `TestSpawnEntityThenReparentToRootAndBack` を 1 本追加
  （`SpawnEntity<Entity>(parent)` の子 → `ReparentEntity(child, nullptr)` →
  `GetRootEntities()` に現れることを明示 assert → `RemoveEntity` で消える）。既存
  `TestReparentEntity`（`EntityTreeTest.cpp:534-591`）が reparent-to-root/id 安定を網羅済みだが、
  「root 昇格後に `GetRootEntities()` 列挙へ載る」直接 assert のみ新規で、アダプタの
  create→reparent→delete シーケンスが依拠する不変条件を薄く補完する。`main()`（`:632-648`）に
  1 行追加。CMake 不要。
- **残存リスクの明記**: アダプタ glue（`extract_string_field`/`extract_json_field` の
  **optional フィールド不在・非文字列値・空 params** の解析）は anonymous namespace の
  自由関数（`.cpp:596-684`）で外部ハーネス無しには単体テスト不可。本フェーズは既存の
  実績ある `objectSetProperty` 抽出経路を完全踏襲することで担保し、**glue 回帰が現行テストで
  捕捉されない点をコミット本文にリスクとして明記**する（将来 helper を testable header へ
  切り出す低コスト改善は歓迎だが A2 必須ではない）。

### EOL / BOM
編集 3 ファイル（adapter `.h`/`.cpp`, `EntityTreeTest.cpp`）は **BOM+純 CRLF（clean）**。
per-file で BOM+CRLF を保存。混在ハザード源の World.h/.cpp は触らない。編集後
`git diff --numstat` で全行 flip でないことを確認（MEMORY の CRLF→LF flip 再発防止）。

### 検証ゲート
```bash
cmake --build C:/Users/KINGkawamura/Documents/NorvesLib/build --config Debug
ctest --test-dir C:/Users/KINGkawamura/Documents/NorvesLib/build -C Debug --output-on-failure   # EntityTreeTest 含む
git -C C:/Users/KINGkawamura/Documents/NorvesLib diff --numstat   # 全行 flip でないこと
```
```powershell
git -C C:/Users/KINGkawamura/Documents/NorvesEditor status   # A2 で NorvesEditor 無変更であること
```

### ブランチ / コミット
base `develop`、作業ブランチ `feature/bridge-scene-edit-adapter`。実装は **Opus 実装者**
（C++ 所有権/スレッド + Bridge adapter は昇格条件）。コミット本文 **必須**（ゲームスレッド同期
ミューテーション、live pointer 不搬送、kind 無視の既知制約、capability 追加、glue テスト残存リスク）。

---

## Phase B — rename（A2 後・spike-gated）
`Entity` に表示名なし。`PROPERTY(Container::String, Name)` を `Entity.h`（clean。:325-339）に
追加し、**新メソッドを作らず `object.setProperty(property="Name")` を再利用**（プロトコル拡張
ゼロ。`Container::String` は型システム側対応済み: `RuntimeSchema.h:220,279`、`TValue.h` の
serialize、アダプタ `WireJsonToSerialized` `.cpp:1002-1005` / `AppendWireValue` `.cpp:501-514`）。
`AppendEntityNode`（`.cpp:699-748`、現状 name 省略 `.cpp:691`）で PROPERTY マクロ生成の
`getName()` を用い `"name"` を出力。`objectGetSnapshot` はプロパティ列挙で自動包含（要確認）。

**spike gate（実装前に必須）**: (1) serialize が **name-keyed か index/position-keyed か**確認
（index ベースだと Name 追加が旧セーブ読込を壊す）。name-keyed なら `ApplyPrefabValue`
（serialize 側反復・クラス引当。`World.cpp:255-285`）で前方追加は無害。(2) `Container::String` の
reflected PROPERTY は前例ゼロ（既存は bool/数値/Vector/Quaternion。`Entity.h:329-339`）→
登録・投影・往復を最小 spike で実証。**Opus 実装者**。リスク: 中。

## Phase C — duplicate（spike のみ・go/no-go）
public clone API なし。選択肢: (a) `IClass::NewInstance`（`IClass.h:966`）+ 手動プロパティ
コピー、(b) `SpawnPrefab`（`World.h:108`）を使う PrefabAsset round-trip（既存
`EntitySubtreeSnapshotTest`/`PrefabRoundTripTest` が土台）、(c) Core レベル clone 新設。
**spike**: 既存 subtree-snapshot + `SpawnPrefab` で「live subtree → prefab → 新 subtree（新
ObjectId 採番）」が public API のみで閉じるか確認 → go なら (b)、no-go なら Core 変更として
alpha スコープ外へ。実装確約なし。

## Undo/Redo 基盤（設計スケッチ）
エンジンが source of truth・ObjectId はエンジン採番。**エディタ側コマンドスタック**を採用。
最小第一増分: **reparent undo**（逆 = 元 parent へ reparent。id 不変 `EntityTreeTest.cpp:560,586`）
+ **create undo**（= deleteObject。newId 既知）。**delete undo は当面除外**（`RemoveEntity` が
subtree 破棄 → 再 create で ObjectId 再採番 `AssignFreshObjectIdsRecursive` のため editor-side
逆操作が非自明。将来 snapshot ベース or エンジン側トランザクションで拡張）。承認後に別途具体計画。

---

## 順序・依存・モデル方針
1. **A2**（即 build-ready・依存なし・最高価値）— Opus 実装者。
2. **B rename** — A2 後、spike gate → 実装。Opus 実装者。
3. **C duplicate** — spike のみ（B と並行可）。go 後の実装は別計画。
4. **Undo/Redo** — 設計スケッチ、実装計画は別途。

各フェーズは独立ブランチ・独立レビュー（実装者以外）・独立コミット・独立検証ゲート。
先行フェーズが実装レビュー + ゲート通過まで後続実装を始めない。
