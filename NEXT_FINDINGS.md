# NEXT_FINDINGS — NorvesEditor

評価者(別文脈)の NEEDS_WORK 指摘。次の反復がタスクより先に処理する。処理したら行を消し、
対応コミットを PROGRESS.md へ書く。

## F-001 [blocking] mock の object.changed が n-1 で properties を空にする(回帰)
- 場所: `bridge/cpp/examples/mock-engine/mock_adapter.hpp` の `object_changed_params` /
  `snapshot_text`。n-1 の本文は `objectGetSnapshot` 側に残ったままなので、`snapshot_text` は
  n-1 を「未知 id」に落として空の propertyBag を返す。
- 影響: n-1 を1回編集すると `object.changed` の params が `{"objectId":"n-1","properties":[]}` に
  なり、store の `objectChangedLive` が丸ごと置換するので Inspector が「プロパティがありません」へ落ちる。
- 直し方: n-1 の本文(可変 fieldOfView 込み)を `snapshot_text` へ移し、smoke に
  「n-1 の setProperty 後の object.changed が fieldOfView=75 を含む」検査を足す。

## F-002 [blocking] 切断・プロセス終了でコンポーネント選択が消えない
- 場所: `apps/editor/src/state/store.ts` の `connectionStateChanged` / `engineProcessExited` /
  `objectSnapshotUnsupported`。`objectSnapshot` は落とすが `selectedComponentId` /
  `componentSnapshot` に触れない。
- 影響: T-004 の done-when(セッション変更/切断で消える)が未達。前セッションの component id が残る。

## F-003 プロパティが空のエンティティでコンポーネント一覧が出ない
- 場所: `apps/editor/src/components/PropertyInspectorPanel.tsx` の「空の propertyBag」分岐が
  `ComponentList` より先に勝つ。
- 影響: `properties: []` かつ `components: [...]`(契約上は合法)のエンティティからコンポーネントへ辿れない。

## F-004 遅延レスポンスの破棄が片側だけ
- 場所: `store.ts` の `componentSnapshotLoaded`。選択が `undefined` のときに遅れて届いた
  スナップショットを格納してしまう。描画では隠れるが `objectPropertyApplied` が書き換え続ける。

## F-005 コンポーネント宛ての object.changed が store に届かない
- 場所: `store.ts` の `objectChangedLive` がエンティティの id としか照合しない。
- 判断: 照合をコンポーネント側へも広げるか、契約に「ライブ反映しない」と明記するかを決める。
