# NorvesLib コンポーネント投影の結線契約

エディタ側(本リポジトリ)の `object.getSnapshot` は result に optional な `components` を
運べるようになった。この文書は、それを実際に満たすために **NorvesLib 側のアダプタ**
(`Game/Bridge/NorvesLibBridgeAdapter.cpp`)で行う変更の契約を固定する。NorvesLib は
Windows + Vulkan SDK 前提で Linux では構築できないため、実装と検証は作業機で行う。

エディタ側の対応状況(参照): スキーマと fixture、汎用 SDK のカウント、Rust の厳格パース、
TS の型と store、Property Inspector の一覧と編集は実装済み。`norves_mock_engine` が
参照実装として同じ形を返す。

## いまの状態(実測)

> **実装状況**: NorvesLib の `feature/bridge-component-projection` で実装済み。
> **作業機でのビルドとテストは未実施** — Linux では NorvesLib を構築できないため、
> 下記「検証」の PowerShell 側は誰も回していない。

- `sceneGetTree` は Entity だけを返す。コンポーネントはツリーに現れない。
- `objectGetSnapshot` / `objectSetProperty` は、`objectId` が `component:<entityObjectId>:<componentId>`
  形式のときだけコンポーネントを解決する(`ResolveBridgeObjectTarget` が `GetComponents()` を走査)。
  ただし解決後の処理は **ScriptComponent 決め打ち**で、`ScriptPath` と `ScriptClassName` の
  2 つの文字列プロパティしか読み書きできない。
- したがってエディタには、そもそもコンポーネントの id を知る手段が無い。
- `Component` は `Object` 派生で `REFLECTION_CLASS` / `PROPERTY` を持ち、
  `RuntimeSchemaProjector::BuildObjectSnapshot(const Object&, ...)` がそのまま使える。
  Entity 用の汎用経路をコンポーネントへ向けるだけでよい。

## 契約(3点)

### 1. コンポーネントのスナップショットを汎用化する

`objectGetSnapshot` の component 分岐から ScriptComponent 特例(`AsScriptComponent` +
`BuildScriptComponentSnapshot`)を外し、Entity と同じ `ProjectClass` + `BuildObjectSnapshot`
経路に統一する。`kind` はクラス名、`properties` はリフレクション投影の値。

これで CameraComponent の `FieldOfView` / `NearPlane` / `FarPlane` / `OrthoWidth` /
`OrthoHeight` / `RenderOrder` / `bIsActiveCamera`、SpringArmComponent、MeshComponent、
SkinnedMeshComponent、RigidBodyComponent、ColliderComponent などが自動的に載る。
ScriptComponent の 2 プロパティも `PROPERTY` 宣言済みなので、特例を外しても同じ値が出る。

### 2. コンポーネントへのプロパティ適用を汎用化する

`objectSetProperty` の component 分岐から `ApplyScriptComponentStringProperty` を外し、
Entity と同じ汎用プロパティ適用経路へ通す。未知プロパティ・型不一致は従来どおり
`{"accepted":false}` を返す(エラー Result にしない)。

### 3. Entity のスナップショットに components を付ける

`objectGetSnapshot` が Entity を返すとき、result へ次を足す:

```json
"components": [
  { "objectId": "component:<entityObjectId>:<componentId>", "kind": "<クラス名>" }
]
```

- `objectId` は既存の解決経路が受け付ける形と**同一**でなければならない(エディタは
  この文字列を解釈せず、そのまま `object.getSnapshot` / `object.setProperty` へ投げ返す)。
- 1 件も無ければ空配列を出す。**欄の省略と空配列は意味が違う** — 省略は「このエンジンは
  コンポーネントを投影しない」、空配列は「投影したが 0 件」。エディタは前者では節ごと出さない。
- `kind` は非空(クラス名が空なら `"Component"` にフォールバックする。`kind` は minLength:1)。
- コンポーネント自身のスナップショットには `components` を付けない(入れ子は無い)。

### やらないこと

- `object.changed` の params に `components` を**足さない**。イベント側スキーマは
  `additionalProperties: false` で欄を持たない。一覧は `object.getSnapshot` の result でだけ運ぶ。
  Entity のスナップショット生成をイベントと共用しているなら、イベント側は欄なしで綴ること。
- コンポーネントの追加・削除(`component.add` / `component.remove` 相当)。別タスク。
- `scene.getTree` へコンポーネントのノードを足すこと。ツリーのノード = Entity の意味論
  (reparent / delete / duplicate の対象)を保つ。
- capability token の追加。`object.query` / `object.edit` のままでよい(エディタは欄の有無で分岐する)。

## 受け入れ条件

1. 実 Game に対し `object.getSnapshot { objectId: "<CameraComponent を持つ Entity>" }` が
   `components` を返し、その `objectId` の 1 つを同じメソッドへ投げると、CameraComponent の
   プロパティが `properties` に載る。
2. その id 宛ての `object.setProperty { property: "FieldOfView", value: <数値> }` が
   `{"accepted":true}` を返し、直後の `object.getSnapshot` に反映される。
3. コンポーネントを 1 つも持たない Entity が `"components": []` を返す。
4. `object.setProperty` 受理後に流れる `object.changed` の params に `components` が無い。
5. 既存の Entity プロパティの読み書き、`scene.getTree`、`schema.getSnapshot` の結果が変わらない。

## 検証

作業機(Windows)で:

```powershell
# NorvesLib: 本リポジトリの bridge/cpp を埋め込んで Game と対象テストを構築する
cmake -S . -B build -G "Visual Studio 17 2022" -DNORVES_BRIDGE_SDK_DIR="<NorvesEditor>/bridge/cpp"
cmake --build build --config Debug --target Game
ctest --test-dir build -C Debug --output-on-failure

# NorvesEditor: 実 Game を相手に e2e を回す（[SKIP] を残さない）
$env:NORVES_NORVESLIB_ENGINE_PATH = "<NorvesLib>/build/Game/Debug/Game.exe"
cargo test --manifest-path apps/editor/src-tauri/Cargo.toml --test process_e2e -- --nocapture
```

エディタ側の回帰(どのプラットフォームでも走る):

```bash
python3 scripts/validate-bridge-fixtures.py          # 現在の総数を表示する（追加のたびに増える）
cargo test --workspace
cargo test --manifest-path apps/editor/src-tauri/Cargo.toml
pnpm -r --if-present typecheck
pnpm -r --if-present test
node scripts/check-protocol-names.mjs
```

## 参照

- 欄の定義: [`bridge/spec/schema/methods/object.getSnapshot.result.schema.json`](../bridge/spec/schema/methods/object.getSnapshot.result.schema.json)
- 欄の説明: [`bridge/spec/docs/message-payloads.md`](../bridge/spec/docs/message-payloads.md) §object.getSnapshot
- 参照実装(mock): `bridge/cpp/examples/mock-engine/mock_adapter.hpp` の `snapshot_text`
- アダプタ境界: [`docs/agent-guide/norveslib-adapter.md`](agent-guide/norveslib-adapter.md)
