/**
 * SceneOutlinerPanel — scene hierarchy browser (Phase 3: scene.getTree wired).
 *
 * Reads the scene snapshot (root SceneNode) from the store, renders it as a
 * recursive tree, and lets the user select a node (writing selectedObjectId via
 * actions.selectObject). Clicking the selected node again, or the empty area,
 * deselects (selectObject(undefined)).
 *
 * Data flow:
 *  - On (re)connect the panel fetches the tree once (actions.getSceneTree).
 *  - A manual "更新 / Refresh" button re-fetches on demand.
 *  - A scene.treeChanged live event with fullRefreshRequired:true sets
 *    store.sceneRefreshRequired; a consume effect here issues getSceneTree()
 *    exactly once per set flag. The resulting sceneTreeLoaded/sceneTreeUnsupported
 *    reducer clears the flag, so the consume cannot loop.
 *
 * Engine-agnostic degradation (no mock-specific assumptions):
 *  (a) disconnected            → "エンジンに接続するとシーンが表示されます"
 *  (b) empty scene (no children) → "オブジェクトがありません"
 *  (c) METHOD_NOT_SUPPORTED     → "この engine はシーン照会に未対応"
 *      (driven by store.sceneUnsupported, set when scene.getTree answers
 *       METHOD_NOT_SUPPORTED — works for any engine, not just the mock).
 */

import { useEffect, useRef, useState } from 'react';
import type React from 'react';
import type { IDockviewPanelProps } from 'dockview-react';
import type { SceneNode } from '@norves/bridge-ui';
import { normalizeOldParentId } from '../state/store.js';
import { useBridgeState } from '../state/BridgeContext.js';
import { useBridgeActions } from '../hooks/useBridge.js';

/**
 * パネルを離れても残す表示の状態。store には載せない — 他のパネルへ配る必要が無く、
 * 1 文字ごとの dispatch で全パネルが再描画される。dockview はタブを離れるとパネルを
 * アンマウントするので、モジュールスコープに置いて次に開いたときの初期値にする。
 * セッション内だけの記憶で、永続化はしない。
 */
let rememberedFilter = '';
let rememberedCollapsed: ReadonlySet<string> = new Set<string>();

/** 絞り込み中に渡す空集合（毎描画で新しい Set を作らない）。 */
const EMPTY_COLLAPSED: ReadonlySet<string> = new Set<string>();

/** テスト用: パネルをまたいで残る表示状態を初期化する。 */
export function __resetOutlinerMemory(): void {
  rememberedFilter = '';
  rememberedCollapsed = new Set<string>();
}

// IDockviewPanelProps is accepted but not currently used for data.
// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export function SceneOutlinerPanel(_props: IDockviewPanelProps): React.JSX.Element {
  const state = useBridgeState();
  const actions = useBridgeActions();

  const isConnected = state.connection.status === 'connected';
  const sceneTree = state.sceneTree;
  const sceneUnsupported = state.sceneUnsupported === true;
  const sceneEditUnsupported = state.sceneEditUnsupported === true;
  const selectedObjectId = state.selectedObjectId;
  const sceneRefreshRequired = state.sceneRefreshRequired === true;

  // -----------------------------------------------------------------------
  // Fetch the tree once each time we (re)enter the connected state. A ref
  // tracks the previous connection status so we only fetch on the
  // disconnected/connecting -> connected edge, not on every re-render. The
  // store clears sceneTree on disconnect, so this re-probes a fresh engine.
  // -----------------------------------------------------------------------
  const wasConnectedRef = useRef(false);
  useEffect(() => {
    if (isConnected && !wasConnectedRef.current) {
      void actions.getSceneTree();
    }
    wasConnectedRef.current = isConnected;
  }, [isConnected, actions]);

  // -----------------------------------------------------------------------
  // Consume a live-refresh request. A scene.treeChanged event with
  // fullRefreshRequired:true sets store.sceneRefreshRequired; here we issue one
  // getSceneTree() while connected. The resulting sceneTreeLoaded/
  // sceneTreeUnsupported reducer clears the flag (-> false), so a single fetch is
  // issued per set flag and the effect cannot loop. A ref guards against firing a
  // second fetch in the render(s) between dispatch and the flag clearing.
  // -----------------------------------------------------------------------
  const refreshInFlightRef = useRef(false);
  useEffect(() => {
    if (isConnected && sceneRefreshRequired && !refreshInFlightRef.current) {
      refreshInFlightRef.current = true;
      void actions.getSceneTree();
    } else if (!sceneRefreshRequired) {
      // Flag was consumed (or never set): re-arm for the next live request.
      refreshInFlightRef.current = false;
    }
  }, [isConnected, sceneRefreshRequired, actions]);

  const handleRefresh = (): void => {
    void actions.getSceneTree();
  };

  const handleCreate = (): void => {
    void actions.createObject(selectedObjectId, undefined);
  };

  const handleDelete = (): void => {
    if (selectedObjectId !== undefined) {
      void actions.deleteObject(selectedObjectId);
    }
  };

  const handleReparentToRoot = (): void => {
    if (selectedObjectId !== undefined) {
      void actions.reparentObject(selectedObjectId, undefined);
    }
  };

  const handleDuplicate = (): void => {
    if (selectedObjectId !== undefined) {
      void actions.duplicateObject(selectedObjectId, undefined);
    }
  };

  // Clicking a node toggles selection: re-clicking the selected node deselects.
  const handleSelect = (id: string): void => {
    actions.selectObject(id === selectedObjectId ? undefined : id);
  };

  // Clicking empty body area deselects (only when something is selected).
  const handleBodyClick = (): void => {
    if (selectedObjectId !== undefined) {
      actions.selectObject(undefined);
    }
  };

  // The filter text is panel-local: it changes nothing outside this view, and a
  // dispatch per keystroke would re-render every panel through the shared
  // context (same reason the Inspector keeps edit drafts local). It is kept in
  // module scope so leaving the panel (dockview unmounts the tab) and coming
  // back restores what the user was looking at.
  const [filter, setFilterState] = useState(rememberedFilter);
  function setFilter(next: string): void {
    rememberedFilter = next;
    setFilterState(next);
  }

  // 折りたたんだノードの id。既定は展開。絞り込み中は無視して全部見せる — 絞り込みの結果が
  // 畳まれた親の下に隠れると、探しているものが見つからない。
  const [collapsed, setCollapsedState] = useState<ReadonlySet<string>>(rememberedCollapsed);
  function toggleCollapsed(id: string): void {
    const next = new Set(collapsed);
    if (!next.delete(id)) {
      next.add(id);
    }
    rememberedCollapsed = next;
    setCollapsedState(next);
  }

  const hasTree = sceneTree !== undefined;
  const editDisabled = !isConnected || sceneEditUnsupported;
  const selectionRequiredDisabled = editDisabled || selectedObjectId === undefined;
  // "Empty scene" = a root with no children (root itself is still selectable).
  const isEmptyScene = hasTree && (sceneTree.children?.length ?? 0) === 0;
  // Filtering is display-only: it never touches the selection or re-fetches.
  const visibleTree = hasTree ? filterSceneTree(sceneTree, filter) : undefined;
  const filtering = filter.trim() !== '';
  // 絞り込み中は折りたたみを無視するので、キー操作が歩く並びも同じ集合で作る。
  const effectiveCollapsed = filtering ? EMPTY_COLLAPSED : collapsed;

  // -----------------------------------------------------------------------
  // ドラッグで親を付け替える。掴んでいる id は ref に持ち、dataTransfer には載せない —
  // 同じアプリの中で完結する操作で、dataTransfer は WebView やテスト環境ごとに扱いが違う。
  // 受けるかどうかの判定は**絞り込み前の木**に対して行う。いまの絞り込みは一致ノードの
  // 部分木と祖先を残すので、見えている範囲でも親子関係は保たれている — だが「見えない子孫は
  // 子孫でない」に依存した判定にはしない。絞り込みの規則が変われば輪ができる。
  // -----------------------------------------------------------------------
  const draggedIdRef = useRef<string | undefined>(undefined);
  const [dropTargetId, setDropTargetId] = useState<string | undefined>(undefined);

  function decideDrop(targetId: string, event: React.DragEvent): DropDecision {
    const dragged = draggedIdRef.current;
    if (dragged === undefined || sceneTree === undefined || editDisabled) {
      return { accepted: false };
    }
    if (isForeignDrag(event)) {
      return { accepted: false };
    }
    return resolveDropTarget(sceneTree, dragged, targetId);
  }

  const handleDragStart = (id: string, event: React.DragEvent): void => {
    draggedIdRef.current = id;
    // 印だけ置く。中身は使わない（付け替える id は ref が持つ）。ドラッグ中に行が作り直されて
    // dragend を取りこぼしても、外から来たドラッグを古い id で処理しないための目印。
    event.dataTransfer?.setData(INTERNAL_DRAG_TYPE, id);
  };

  const handleDragEnd = (): void => {
    draggedIdRef.current = undefined;
    setDropTargetId(undefined);
  };

  const handleDragOver = (id: string, event: React.DragEvent): void => {
    if (!decideDrop(id, event).accepted) {
      return;
    }
    // preventDefault を呼んだ要素だけがドロップを受け付ける（HTML の約束）。
    event.preventDefault();
    if (dropTargetId !== id) {
      setDropTargetId(id);
    }
  };

  const handleDragLeave = (id: string): void => {
    if (dropTargetId === id) {
      setDropTargetId(undefined);
    }
  };

  const handleDrop = (id: string, event: React.DragEvent): void => {
    const draggedId = draggedIdRef.current;
    const decision = decideDrop(id, event);
    draggedIdRef.current = undefined;
    setDropTargetId(undefined);
    if (draggedId === undefined || !decision.accepted) {
      return;
    }
    event.preventDefault();
    // 動かすのは掴んでいたノード。落とし先は新しい親になる（最上段はルート = undefined）。
    void actions.reparentObject(draggedId, decision.newParentId);
  };

  // -----------------------------------------------------------------------
  // キーボード操作。上下で行を移り、左右で開閉する（ツリーの一般的な約束）。
  // 選択は既存の selectObject に流し、焦点は移った行のボタンへ移す — 選択だけ動いて
  // 焦点が置き去りになると、次の矢印が効かなくなる。
  // -----------------------------------------------------------------------
  const treeRef = useRef<HTMLUListElement>(null);

  function focusRow(id: string): void {
    // セレクタを組み立てない。id は engine が決める任意の文字列で（`component:12:3` のように
    // 記号を含む）、属性セレクタへ埋めるとエスケープが要る。走査して比べるほうが安全で、
    // 木の行数はいつも小さい。
    const rows = treeRef.current?.querySelectorAll<HTMLButtonElement>(
      'button.scene-node__row[data-node-id]',
    ) ?? [];
    for (const row of rows) {
      if (row.dataset.nodeId === id) {
        row.focus();
        return;
      }
    }
  }

  function moveTo(id: string): void {
    actions.selectObject(id);
    focusRow(id);
  }

  function handleTreeKeyDown(event: React.KeyboardEvent<HTMLUListElement>): void {
    if (visibleTree === undefined) {
      return;
    }
    if (
      event.key !== 'ArrowDown' &&
      event.key !== 'ArrowUp' &&
      event.key !== 'ArrowLeft' &&
      event.key !== 'ArrowRight'
    ) {
      return;
    }
    // ツリーが受け取る矢印は、行が動かない場合でも既定動作を止める。止めないとパネル本文が
    // 裏でスクロールして、選択は動いていないのに画面だけ動く（Asset Browser と同じ扱い）。
    event.preventDefault();

    const rows = flattenVisibleRows(visibleTree, effectiveCollapsed);
    // 焦点がトグルにあっても、基準はその行。選択中の別の行を開閉してはならない。
    const active = document.activeElement as HTMLElement | null;
    const focusedId =
      active?.closest<HTMLElement>('[data-node-id]')?.dataset.nodeId ?? selectedObjectId;
    const index = rows.findIndex((row) => row.id === focusedId);
    if (index < 0) {
      // どの行にも居ないときは、下方向のキーで先頭へ入る。
      if (event.key === 'ArrowDown' && rows.length > 0) {
        moveTo(rows[0].id);
      }
      return;
    }
    const row = rows[index];

    switch (event.key) {
      case 'ArrowDown':
        if (index + 1 < rows.length) {
          moveTo(rows[index + 1].id);
        }
        return;
      case 'ArrowUp':
        if (index > 0) {
          moveTo(rows[index - 1].id);
        }
        return;
      case 'ArrowRight':
        // 畳んでいれば開く。開いていれば最初の子へ。
        if (row.hasChildren && row.isCollapsed) {
          toggleCollapsed(row.id);
        } else if (row.hasChildren && index + 1 < rows.length) {
          moveTo(rows[index + 1].id);
        }
        return;
      case 'ArrowLeft':
        // 開いていれば畳む。畳んでいる（または葉）なら親へ。
        if (row.hasChildren && !row.isCollapsed) {
          toggleCollapsed(row.id);
        } else if (row.parentId !== undefined) {
          moveTo(row.parentId);
        }
        return;
      default:
        return;
    }
  }

  return (
    <div className="panel">
      <div className="panel__header">
        <span>Scene Outliner</span>
      </div>

      {/*
        操作ボタンはヘッダではなく本文側のツールバーに置く。ヘッダは高さが固定で折り返せず、
        パネルを 345px 未満へ狭めるとボタンが右へはみ出して押せなくなっていた（dockview は
        いくらでも狭められる）。ツールバーなら折り返せる。
      */}
      <div className="panel__toolbar">
        <button
          className="btn panel__toolbar-btn"
          type="button"
          onClick={handleCreate}
          disabled={editDisabled}
          title="Create a scene object"
        >
          追加
        </button>
        <button
          className="btn panel__toolbar-btn"
          type="button"
          onClick={handleDelete}
          disabled={selectionRequiredDisabled}
          title="Delete the selected scene object"
        >
          削除
        </button>
        <button
          className="btn panel__toolbar-btn"
          type="button"
          onClick={handleReparentToRoot}
          disabled={selectionRequiredDisabled}
          title="Move the selected scene object to root"
        >
          rootへ移動
        </button>
        <button
          className="btn panel__toolbar-btn"
          type="button"
          onClick={handleDuplicate}
          disabled={selectionRequiredDisabled}
          title="Duplicate the selected scene object"
        >
          複製
        </button>
        {isConnected && (
          <button
            className="btn panel__toolbar-btn"
            type="button"
            onClick={handleRefresh}
            title="Re-fetch the scene tree (scene.getTree)"
          >
            更新
          </button>
        )}
      </div>

      {isConnected && !sceneUnsupported && (
        <div className="panel__filter">
          <input
            type="search"
            aria-label="シーンを絞り込む"
            placeholder="名前 / 種別 / id で絞り込む"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          />
        </div>
      )}

      {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions, jsx-a11y/click-events-have-key-events */}
      <div className="panel__body col" onClick={handleBodyClick}>
        {!isConnected ? (
          /* (a) Disconnected — engine not attached */
          <div className="placeholder-box" style={{ flex: 1 }}>
            <span className="placeholder-box__title">Scene Outliner</span>
            <span>エンジンに接続するとシーンが表示されます。</span>
            <span style={{ fontSize: 11 }}>Connect to an engine to view the scene hierarchy.</span>
          </div>
        ) : sceneUnsupported ? (
          /* (c) Engine does not implement scene query (METHOD_NOT_SUPPORTED) */
          <div className="placeholder-box" style={{ flex: 1 }}>
            <span className="placeholder-box__title">シーン照会に未対応</span>
            <span>この engine はシーン照会に未対応です。</span>
            <span style={{ fontSize: 11 }}>This engine does not support scene queries.</span>
          </div>
        ) : !hasTree ? (
          /* Connected but the tree has not arrived yet (initial fetch pending) */
          <div className="placeholder-box" style={{ flex: 1 }}>
            <span className="placeholder-box__title">読み込み中</span>
            <span>シーンを取得しています...</span>
            <span style={{ fontSize: 11 }}>Loading scene...</span>
          </div>
        ) : isEmptyScene ? (
          /* (b) Empty scene — root present but no children */
          <div className="placeholder-box" style={{ flex: 1 }}>
            <span className="placeholder-box__title">空のシーン</span>
            <span>オブジェクトがありません。</span>
            <span style={{ fontSize: 11 }}>The scene has no objects.</span>
          </div>
        ) : visibleTree === undefined ? (
          /* Filtered down to nothing — the tree itself is fine, the query is not */
          <div className="placeholder-box" style={{ flex: 1 }}>
            <span className="placeholder-box__title">一致なし</span>
            <span>一致するオブジェクトがありません。</span>
            <span style={{ fontSize: 11 }}>No object matches the filter.</span>
          </div>
        ) : (
          <ul className="scene-tree" ref={treeRef} onKeyDown={handleTreeKeyDown}>
            <SceneTreeNode
              node={visibleTree}
              selectedId={selectedObjectId}
              onSelect={handleSelect}
              collapsed={effectiveCollapsed}
              onToggleCollapsed={toggleCollapsed}
              drag={{
                enabled: !editDisabled,
                dropTargetId,
                onDragStart: handleDragStart,
                onDragEnd: handleDragEnd,
                onDragOver: handleDragOver,
                onDragLeave: handleDragLeave,
                onDrop: handleDrop,
              }}
            />
          </ul>
        )}
      </div>
    </div>
  );
}

// -------------------------------------------------------------------------
// Drag and drop reparenting
// -------------------------------------------------------------------------

/**
 * 自前のドラッグに付ける印。中身は使わず、有無だけを見る。
 * これが無いドラッグ（OS のファイルなど）は、掴んだ id が残っていても処理しない。
 */
const INTERNAL_DRAG_TYPE = 'application/x-norves-scene-node';

/**
 * 外から来たドラッグか。
 *
 * `dataTransfer.types` が読めない環境（jsdom や一部の WebView）では判定しない — 読めないことを
 * 「外から来た」の証拠にすると、まともなドラッグまで拒否してしまう。読めるときだけ印を確かめる。
 *
 * @param event ドラッグのイベント
 * @returns 印の無いドラッグなら true
 */
function isForeignDrag(event: React.DragEvent): boolean {
  const types = event.dataTransfer?.types;
  if (types === undefined || types.length === 0) {
    return false;
  }
  return !Array.from(types).includes(INTERNAL_DRAG_TYPE);
}

/** ドロップを受けるかどうかと、受けるなら渡す親。 */
export type DropDecision =
  | { accepted: false }
  | { accepted: true; newParentId: string | undefined };

/** `node` の部分木に `id` が含まれるか（自分自身を含む）。 */
function subtreeContains(node: SceneNode, id: string): boolean {
  if (node.id === id) {
    return true;
  }
  return (node.children ?? []).some((child) => subtreeContains(child, id));
}

/**
 * `draggedId` を持つノードの**いずれか**の部分木に `targetId` が入っているか。
 *
 * 木の中の id は一意だと決まっていない（`sceneNode.id` の契約は「非空の不透明な文字列」だけ）。
 * 最初に見つかった 1 個だけで判定すると、同じ id の別のノードの下へ落とせてしまい輪ができる。
 */
function anySubtreeContains(node: SceneNode, draggedId: string, targetId: string): boolean {
  if (node.id === draggedId && subtreeContains(node, targetId)) {
    return true;
  }
  return (node.children ?? []).some((child) => anySubtreeContains(child, draggedId, targetId));
}

/** `root` の部分木から `id` のノードを探す。 */
function findNode(node: SceneNode, id: string): SceneNode | undefined {
  if (node.id === id) {
    return node;
  }
  for (const child of node.children ?? []) {
    const found = findNode(child, id);
    if (found !== undefined) {
      return found;
    }
  }
  return undefined;
}

/**
 * `draggedId` を `targetId` の上に落としたときに、エンジンへ何を送るかを決める。
 *
 * 判定は**絞り込み前の木**に対して行う。絞り込みで隠れているだけの子孫も子孫であり、
 * 見えていないからといって親子関係を作ってよいわけではない。
 *
 * 受けないのは: 同じノード / シーンルート自身を動かす / 自分の部分木の中へ入れる（輪ができる）/
 * いまと同じ親へ入れる（何も変わらない往復を作らない）/ 木に無い id。
 *
 * ツリーの最上段はエンジンが返す合成ルートなので、そこへ落とすときは親を `undefined` に
 * する（エンジンの nullptr=root の経路。id をそのまま渡さない）。
 *
 * @param root シーンツリーの根（絞り込み前）
 * @param draggedId 掴んでいるノードの id
 * @param targetId 落とし先のノードの id
 * @returns 送る内容、または受けない
 */
export function resolveDropTarget(
  root: SceneNode,
  draggedId: string,
  targetId: string,
): DropDecision {
  if (draggedId === targetId || draggedId === root.id) {
    return { accepted: false };
  }

  if (findNode(root, draggedId) === undefined || findNode(root, targetId) === undefined) {
    return { accepted: false };
  }

  // 自分の中へは入れられない（輪ができる）。同じ id のノードが複数あっても取りこぼさない。
  if (anySubtreeContains(root, draggedId, targetId)) {
    return { accepted: false };
  }

  const newParentId = targetId === root.id ? undefined : targetId;
  if (normalizeOldParentId(root, draggedId) === newParentId) {
    return { accepted: false };  // いまと同じ親。
  }
  return { accepted: true, newParentId };
}

// -------------------------------------------------------------------------
// Keyboard navigation
// -------------------------------------------------------------------------

/** 画面に出ている 1 行。キーボード移動はこの並びの上だけで起きる。 */
export interface VisibleRow {
  id: string;
  parentId: string | undefined;
  hasChildren: boolean;
  isCollapsed: boolean;
}

/**
 * 表示中のツリーを、画面に見えている順（上から下）の 1 次元配列にする。
 * 折りたたまれたノードの子は入らない — 見えない行へ矢印で飛べてはいけない。
 *
 * @param node 表示中のツリー（絞り込み済みのもの）
 * @param collapsed 折りたたまれた id（絞り込み中は空集合が渡る）
 * @returns 画面順の行
 */
export function flattenVisibleRows(
  node: SceneNode,
  collapsed: ReadonlySet<string>,
  parentId?: string,
): VisibleRow[] {
  const children = node.children ?? [];
  const hasChildren = children.length > 0;
  const isCollapsed = hasChildren && collapsed.has(node.id);
  const rows: VisibleRow[] = [{ id: node.id, parentId, hasChildren, isCollapsed }];
  if (!isCollapsed) {
    for (const child of children) {
      rows.push(...flattenVisibleRows(child, collapsed, node.id));
    }
  }
  return rows;
}

// -------------------------------------------------------------------------
// Filtering
// -------------------------------------------------------------------------

/** Whether one node's own text matches the (already lower-cased) needle. */
function nodeMatches(node: SceneNode, needle: string): boolean {
  const label = node.name ?? node.id;
  return (
    label.toLowerCase().includes(needle) ||
    node.id.toLowerCase().includes(needle) ||
    (node.kind ?? '').toLowerCase().includes(needle)
  );
}

/**
 * Narrow a scene tree to the nodes worth showing for `query`, or `undefined`
 * when nothing under `node` matches.
 *
 * Two rules, both about being able to act on the result:
 *  - A node whose own text matches is kept WITH its whole subtree. The user
 *    asked for that node; hiding what is inside it would be surprising.
 *  - A node that does not match is kept only when a descendant does, and then
 *    only with the matching branches. That keeps the path to a match visible —
 *    without the ancestors the row could not be reached in a tree view.
 *
 * Matching is a case-insensitive substring over name (falling back to id), id
 * and kind. Not a regular expression: a stray character in a pattern would
 * silently empty the panel instead of narrowing it.
 */
export function filterSceneTree(node: SceneNode, query: string): SceneNode | undefined {
  const needle = query.trim().toLowerCase();
  if (needle === '') {
    return node;
  }
  if (nodeMatches(node, needle)) {
    return node;
  }
  const keptChildren = (node.children ?? [])
    .map((child) => filterSceneTree(child, query))
    .filter((child): child is SceneNode => child !== undefined);
  if (keptChildren.length === 0) {
    return undefined;
  }
  return { ...node, children: keptChildren };
}

// -------------------------------------------------------------------------
// Recursive tree node
// -------------------------------------------------------------------------

/** ドラッグでの親付け替えに必要なものをまとめて渡す（行ごとに 6 個の prop を配らない）。 */
interface SceneTreeDrag {
  /** 編集できない接続では掴めないようにする。 */
  enabled: boolean;
  /** いま受け入れ表示にする行。 */
  dropTargetId: string | undefined;
  onDragStart: (id: string, event: React.DragEvent) => void;
  onDragEnd: () => void;
  onDragOver: (id: string, event: React.DragEvent) => void;
  onDragLeave: (id: string) => void;
  onDrop: (id: string, event: React.DragEvent) => void;
}

interface SceneTreeNodeProps {
  node: SceneNode;
  selectedId: string | undefined;
  onSelect: (id: string) => void;
  /** 折りたたまれているノードの id。絞り込み中は空集合が渡る。 */
  collapsed: ReadonlySet<string>;
  onToggleCollapsed: (id: string) => void;
  drag: SceneTreeDrag;
}

function SceneTreeNode({
  node,
  selectedId,
  onSelect,
  collapsed,
  onToggleCollapsed,
  drag,
}: SceneTreeNodeProps): React.JSX.Element {
  const isSelected = node.id === selectedId;
  const children = node.children ?? [];
  const label = node.name ?? node.id;
  const hasChildren = children.length > 0;
  const isCollapsed = hasChildren && collapsed.has(node.id);

  // Stop propagation so clicking a node row does not bubble to the body
  // deselect handler.
  const handleClick = (event: React.MouseEvent): void => {
    event.stopPropagation();
    onSelect(node.id);
  };

  // 折りたたみは選択と別の操作。行のボタンとは分け、伝播も止める。
  const handleToggle = (event: React.MouseEvent): void => {
    event.stopPropagation();
    onToggleCollapsed(node.id);
  };

  return (
    <li className="scene-node">
      <div className="scene-node__line">
        {hasChildren ? (
          <button
            type="button"
            className="scene-node__toggle"
            // 矢印キーの基準行を決めるための目印（焦点がトグルにあるときも行が分かる）。
            data-node-id={node.id}
            aria-expanded={!isCollapsed}
            aria-label={`${label} を${isCollapsed ? '展開' : '折りたたむ'}`}
            onClick={handleToggle}
          >
            {isCollapsed ? '\u25B8' : '\u25BE'}
          </button>
        ) : (
          /* 子が無い行も同じ量だけ字下げして、名前の左端を揃える。 */
          <span className="scene-node__toggle scene-node__toggle--empty" aria-hidden="true" />
        )}
        <button
          type="button"
          className={
            `scene-node__row${isSelected ? ' scene-node__row--selected' : ''}` +
            (drag.dropTargetId === node.id ? ' scene-node__row--drop' : '')
          }
          aria-selected={isSelected}
          // キー操作が「いまどの行に居るか」を読み、移った先へ焦点を移すための目印。
          data-node-id={node.id}
          draggable={drag.enabled}
          onClick={handleClick}
          onDragStart={(event) => drag.onDragStart(node.id, event)}
          onDragEnd={drag.onDragEnd}
          onDragOver={(event) => drag.onDragOver(node.id, event)}
          onDragLeave={() => drag.onDragLeave(node.id)}
          onDrop={(event) => drag.onDrop(node.id, event)}
        >
          <span className="scene-node__name">{label}</span>
          {node.kind !== undefined && <span className="scene-node__kind">{node.kind}</span>}
        </button>
      </div>
      {hasChildren && !isCollapsed && (
        <ul className="scene-tree__children" style={{ marginLeft: 12 }}>
          {children.map((child) => (
            <SceneTreeNode
              key={child.id}
              node={child}
              selectedId={selectedId}
              onSelect={onSelect}
              collapsed={collapsed}
              onToggleCollapsed={onToggleCollapsed}
              drag={drag}
            />
          ))}
        </ul>
      )}
    </li>
  );
}
