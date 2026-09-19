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
          <ul className="scene-tree">
            <SceneTreeNode
              node={visibleTree}
              selectedId={selectedObjectId}
              onSelect={handleSelect}
              collapsed={filtering ? EMPTY_COLLAPSED : collapsed}
              onToggleCollapsed={toggleCollapsed}
            />
          </ul>
        )}
      </div>
    </div>
  );
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

interface SceneTreeNodeProps {
  node: SceneNode;
  selectedId: string | undefined;
  onSelect: (id: string) => void;
  /** 折りたたまれているノードの id。絞り込み中は空集合が渡る。 */
  collapsed: ReadonlySet<string>;
  onToggleCollapsed: (id: string) => void;
}

function SceneTreeNode({
  node,
  selectedId,
  onSelect,
  collapsed,
  onToggleCollapsed,
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
          className={`scene-node__row${isSelected ? ' scene-node__row--selected' : ''}`}
          aria-selected={isSelected}
          onClick={handleClick}
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
            />
          ))}
        </ul>
      )}
    </li>
  );
}
