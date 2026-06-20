/**
 * SceneOutlinerPanel — scene hierarchy browser (Phase 1 placeholder).
 *
 * This is a structural placeholder that establishes:
 *  - The IDockviewPanelProps interface contract.
 *  - The three empty-state branches: disconnected / empty scene / no selection.
 *  - Reference to useBridgeState() for future Phase 3 data wiring.
 *
 * Actual scene data (sceneTree) will be wired in Phase 3 (scene.getTree).
 * Until then, the panel displays a context-appropriate empty state.
 *
 * Engine-agnostic: no mock-specific names or assumptions.
 */

import type React from 'react';
import type { IDockviewPanelProps } from 'dockview-react';
import { useBridgeState } from '../state/BridgeContext.js';

// IDockviewPanelProps is accepted but not currently used for data.
// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export function SceneOutlinerPanel(_props: IDockviewPanelProps): React.JSX.Element {
  const state = useBridgeState();
  const isConnected = state.connection.status === 'connected';

  return (
    <div className="panel">
      <div className="panel__header">
        <span>Scene Outliner</span>
      </div>

      <div className="panel__body col">
        {!isConnected ? (
          /* State 1: Disconnected — engine not attached */
          <div className="placeholder-box" style={{ flex: 1 }}>
            <span className="placeholder-box__title">Scene Outliner</span>
            <span>エンジンに接続するとシーンが表示されます。</span>
            <span style={{ fontSize: 11 }}>Connect to an engine to view the scene hierarchy.</span>
          </div>
        ) : (
          /* State 2 / 3: Connected but no scene data yet (Phase 3 will populate this) */
          <div className="placeholder-box" style={{ flex: 1 }}>
            <span className="placeholder-box__title">空のシーン</span>
            <span>オブジェクトがありません。</span>
            <span style={{ fontSize: 11 }}>
              Scene data will appear here once scene.getTree is wired (Phase 3).
            </span>
          </div>
        )}
      </div>
    </div>
  );
}
