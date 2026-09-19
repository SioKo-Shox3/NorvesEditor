# NorvesLib コンポーネント追加/削除の結線契約

エディタ側(本リポジトリ)は `component.add` / `component.remove` と、生成可能な型の広告
(`schema.getSnapshot` の `instantiable`)を実装済みで、参照実装として `norves_mock_engine` が
同じ形を返す。この文書は、それを実エンジンで満たすために **NorvesLib 側**(`Game/Bridge/NorvesLibBridgeAdapter.cpp`
と、必要になる Core 側の追加)で行う変更の契約を固定する。NorvesLib は Windows + Vulkan SDK 前提で
Linux では構築できないため、実装と検証は作業機で行う。

前提: [`docs/norveslib-component-projection-contract.md`](norveslib-component-projection-contract.md)
(コンポーネントの投影)が先に入っていること。本契約はその上に乗る。

## いまの状態(実測)

- `Entity::AddComponent(Component::Component*)` と `Entity::RemoveComponent(Component::Component*)` は
  公開されている。ただし **`AddComponent` は構築済みのポインタを取る**。
- `ClassRegistry::FindClass(Identity)` / `FindClass(uint64_t)` でクラスは引けるが、
  **`IClass` に実体を生成する入口が無い**(`CreateInstance` 相当が存在しない)。
- したがって「型名からコンポーネントを作る」経路が無い。これは `scene.createObject` の `kind` が
  無視されている理由と同じ根であり、本契約の主題はここ。

## 契約(3点)

### 1. クラス名から Component を生成する factory を新設する

`ClassRegistry` に登録済みのクラスのうち **`Component` 派生**について、型名から実体を生成できる
入口を作る。

**実測(2026-09-19)を踏まえた実装方針**: 生成そのものは `World::CreateComponent<T>(owner)` が既に
持っている(`new T()` → `Entity::AddComponent`、失敗時 null)。足りないのは**型名 → `T` の対応**だけ
なので、`Core` へ新しい公開 API を足さず、**アダプタ側(`Game/Bridge`)に「型名 → 生成関数」の
明示テーブル**を置く。アダプタは NorvesLib を知ってよい層であり、`Core` の公開 API(危険地帯)を
広げずに済む。`instantiable` の広告もこのテーブルを唯一の出典にする。

シーン/プレハブの読み込みが将来同じ対応表を必要とするなら、そのときに `Core` 側へ引き上げる。
現時点では `AddComponent` の呼び出し元は `World::CreateComponent` だけで、名前からの生成を要する
のは Bridge だけ(実測)。

制約は3つ:

- **シングルトンを作らない**(`static Instance& Get()` を足さない)。所有権は既存の
  Outer/Inner モデルに従い、生成したコンポーネントは `AddComponent` で Entity の Inner になる。
- **登録は明示的**にする。リフレクションに載っているという理由だけで、生成してよい型に
  してはならない(初期化に外部の前提を要する型が混ざる)。
- 生成に失敗したら **null を返して呼び出し側で reject** する。例外や停止に落とさない
  (`World::CreateComponent` は `new` の例外を捕らえて null を返す既存実装なので、それに乗る)。

### 2. 生成できる型だけを `instantiable: true` で広告する

`schemaGetSnapshot` の出力で、factory に登録済みの `Component` 派生クラスにだけ
`"instantiable":true` を付ける。それ以外の型には**欄そのものを出さない**(false を明示しても
よいが、既定は「言わない」)。`kind` は投影契約どおりクラス名を使う。

エディタは `kind === "component" && instantiable === true` の型だけを追加候補に出す。欄が無い型は
候補に入らないので、**広告を足すまで実エンジンでは追加 UI が出ない**(これは意図した degrade)。

### 3. `componentAdd` / `componentRemove` をアダプタに実装する

```
component.add    params { objectId: <entityObjectId>, kind: <クラス名> }
                 result { accepted: bool, componentId?: "component:<entityObjectId>:<componentId>" }
component.remove params { objectId: "component:<entityObjectId>:<componentId>" }
                 result { accepted: bool }
```

- `objectId` の解決は既存の `ResolveBridgeObjectTarget` を使う。`component.add` は Entity を、
  `component.remove` は Component を期待し、種別が違えば `accepted:false`。
- `component.add` は factory で生成 → `AddComponent` → 生成された `GetComponentId()` から
  `component:<owner>:<cid>` を組み立てて返す。**返す id は既存の解決経路がそのまま受け付ける形**で
  なければならない(エディタはこの文字列を解釈せず投げ返すだけ)。
- `component.remove` は `RemoveComponent` を呼ぶ。破棄の寿命は NorvesLib の所有権モデルに従う
  (Inner の連鎖破棄)。外した後にエディタが同じ id を投げても `accepted:false` になること。
  `Entity::RemoveComponent` は `void` で、Inner に無ければ黙って何もしない(実測)。したがって
  受理判定は**呼ぶ前に所有関係を確かめる**こと。所有者は `Component::GetOuter()`(`IUnknown*`)を
  `Entity` へ `CastTo` して得る。
- 失敗(未知の型・生成できない型・親が消えた・エンジン状態が許さない)は**すべて `accepted:false`**。
  プロトコルエラーにしない。
- **スレッド**: 生成と付け外しは GameThread で行う。Bridge のハンドラから直接 World を触らず、
  既存のコマンド marshalling に合わせる。

### 4. capability に `component.edit` を足す

`getCapabilities` の配列に `{"name":"component.edit"}` を足す。エディタはこの token が無い接続では
追加/削除の UI を出さない(メソッドを呼びもしない)。

### やらないこと

- `object.changed` に components を載せない(イベント側スキーマは `additionalProperties:false`)。
- Undo/Redo の対応。削除はプロパティ値を復元できないため、エディタ側も undo スタックに積んでいない。
- `scene.createObject` の `kind` 対応。同じ factory が土台になるが、別タスク。

## 受け入れ条件

1. `schema.getSnapshot` が、factory 登録済みの Component 派生にだけ `instantiable:true` を付けて返す。
2. 実 Game に対し `component.add { objectId: <Entity>, kind: <登録済みの型> }` が
   `{"accepted":true,"componentId":"component:<owner>:<cid>"}` を返し、その id を
   `object.getSnapshot` に投げるとそのコンポーネントのプロパティが返る。
3. 直後に同じ Entity の `object.getSnapshot` を取ると、`components` に足した id が載っている。
4. `component.remove` で外すと、同じ Entity の `components` から消え、同じ id での再削除は
   `accepted:false` になる。
5. 未登録の型名・Entity でない `objectId`・欄の欠落は、すべて `accepted:false`(エラー Result にしない)。
6. `bridge.getCapabilities` が `component.edit` を含む。
7. 既存の読み取り・プロパティ編集・`scene.getTree` の結果が変わらない。

## 検証

作業機(Windows)で:

```powershell
cmake -S . -B build -G "Visual Studio 17 2022" -DNORVES_BRIDGE_SDK_DIR="<NorvesEditor>/bridge/cpp"
cmake --build build --config Debug --target Game
ctest --test-dir build -C Debug --output-on-failure

$env:NORVES_NORVESLIB_ENGINE_PATH = "<NorvesLib>/build/Game/Debug/Game.exe"
cargo test --manifest-path apps/editor/src-tauri/Cargo.toml --test process_e2e -- --nocapture
```

エディタ側の回帰(どのプラットフォームでも走る):

```bash
python3 scripts/validate-bridge-fixtures.py          # OK: 174 fixture(s) validated.
cargo test --workspace
cargo test --manifest-path apps/editor/src-tauri/Cargo.toml
ctest --test-dir build/cpp --output-on-failure
pnpm -r --if-present typecheck
pnpm -r --if-present test
node scripts/check-protocol-names.mjs
```

## 参照

- メソッドの定義: [`bridge/spec/schema/methods/component.add.params.schema.json`](../bridge/spec/schema/methods/component.add.params.schema.json) ほか
- 欄と意味: [`bridge/spec/docs/message-payloads.md`](../bridge/spec/docs/message-payloads.md) §component.add / §component.remove
- capability: [`bridge/spec/docs/capabilities.md`](../bridge/spec/docs/capabilities.md) §component.edit
- 参照実装(mock): `bridge/cpp/examples/mock-engine/mock_adapter.hpp` の `componentAdd` / `componentRemove`
