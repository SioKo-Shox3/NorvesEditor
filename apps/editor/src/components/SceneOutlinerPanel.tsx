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

import { useEffect, useRef } from 'react';
import type React from 'react';
import type { IDockviewPanelProps } from 'dockview-react';
import type { SceneNode } from '@norves/bridge-ui';
import { useBridgeState } from '../state/BridgeContext.js';
import { useBridgeActions } from '../hooks/useBridge.js';

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

  const hasTree = sceneTree !== undefined;
  const editDisabled = !isConnected || sceneEditUnsupported;
  const selectionRequiredDisabled = editDisabled || selectedObjectId === undefined;
  // "Empty scene" = a root with no children (root itself is still selectable).
  const isEmptyScene = hasTree && (sceneTree.children?.length ?? 0) === 0;

  return (
    <div className="panel">
      <div className="panel__header">
        <span>Scene Outliner</span>
        <div style={{ marginLeft: 'auto', display: 'flex', gap: 6 }}>
          <button
            className="btn"
            type="button"
            onClick={handleCreate}
            disabled={editDisabled}
            title="Create a scene object"
            style={{ padding: '2px 8px', fontSize: 11 }}
          >
            追加
          </button>
          <button
            className="btn"
            type="button"
            onClick={handleDelete}
            disabled={selectionRequiredDisabled}
            title="Delete the selected scene object"
            style={{ padding: '2px 8px', fontSize: 11 }}
          >
            削除
          </button>
          <button
            className="btn"
            type="button"
            onClick={handleReparentToRoot}
            disabled={selectionRequiredDisabled}
            title="Move the selected scene object to root"
            style={{ padding: '2px 8px', fontSize: 11 }}
          >
            rootへ移動
          </button>
          <button
            className="btn"
            type="button"
            onClick={handleDuplicate}
            disabled={selectionRequiredDisabled}
            title="Duplicate the selected scene object"
            style={{ padding: '2px 8px', fontSize: 11 }}
          >
            複製
          </button>
          {isConnected && (
            <button
              className="btn"
              type="button"
              onClick={handleRefresh}
              title="Re-fetch the scene tree (scene.getTree)"
              style={{ padding: '2px 8px', fontSize: 11 }}
            >
              更新
            </button>
          )}
        </div>
      </div>

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
        ) : (
          <ul className="scene-tree">
            <SceneTreeNode
              node={sceneTree}
              selectedId={selectedObjectId}
              onSelect={handleSelect}
            />
          </ul>
        )}
      </div>
    </div>
  );
}

// -------------------------------------------------------------------------
// Recursive tree node
// -------------------------------------------------------------------------

interface SceneTreeNodeProps {
  node: SceneNode;
  selectedId: string | undefined;
  onSelect: (id: string) => void;
}

function SceneTreeNode({ node, selectedId, onSelect }: SceneTreeNodeProps): React.JSX.Element {
  const isSelected = node.id === selectedId;
  const children = node.children ?? [];
  const label = node.name ?? node.id;

  // Stop propagation so clicking a node row does not bubble to the body
  // deselect handler.
  const handleClick = (event: React.MouseEvent): void => {
    event.stopPropagation();
    onSelect(node.id);
  };

  return (
    <li className="scene-node">
      <button
        type="button"
        className={`scene-node__row${isSelected ? ' scene-node__row--selected' : ''}`}
        aria-selected={isSelected}
        onClick={handleClick}
      >
        <span className="scene-node__name">{label}</span>
        {node.kind !== undefined && <span className="scene-node__kind">{node.kind}</span>}
      </button>
      {children.length > 0 && (
        <ul className="scene-tree__children" style={{ marginLeft: 12 }}>
          {children.map((child) => (
            <SceneTreeNode
              key={child.id}
              node={child}
              selectedId={selectedId}
              onSelect={onSelect}
            />
          ))}
        </ul>
      )}
    </li>
  );
}
