/**
 * PropertyInspectorPanel — property inspector for selected scene objects (Phase 1 placeholder).
 *
 * This is a structural placeholder that establishes:
 *  - The IDockviewPanelProps interface contract.
 *  - The three empty-state branches: disconnected / no selection / empty properties.
 *  - Reference to useBridgeState() for selectedObjectId and future Phase 4 data wiring.
 *
 * Actual object snapshot (properties) will be wired in Phase 4 (object.getSnapshot).
 * Until then, the panel displays a context-appropriate empty state.
 *
 * Engine-agnostic: no mock-specific names or field assumptions.
 */

import type React from 'react';
import type { IDockviewPanelProps } from 'dockview-react';
import { useBridgeState } from '../state/BridgeContext.js';

// IDockviewPanelProps is accepted but not currently used for data.
// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export function PropertyInspectorPanel(_props: IDockviewPanelProps): React.JSX.Element {
  const state = useBridgeState();
  const isConnected      = state.connection.status === 'connected';
  const selectedObjectId = state.selectedObjectId;

  return (
    <div className="panel">
      <div className="panel__header">
        <span>Property Inspector</span>
      </div>

      <div className="panel__body col">
        {!isConnected ? (
          /* State 1: Disconnected */
          <div className="placeholder-box" style={{ flex: 1 }}>
            <span className="placeholder-box__title">Property Inspector</span>
            <span>エンジンに接続するとプロパティが表示されます。</span>
            <span style={{ fontSize: 11 }}>Connect to an engine to inspect properties.</span>
          </div>
        ) : selectedObjectId === undefined ? (
          /* State 2: Connected but nothing selected */
          <div className="placeholder-box" style={{ flex: 1 }}>
            <span className="placeholder-box__title">選択なし</span>
            <span>Scene Outliner でオブジェクトを選択してください。</span>
            <span style={{ fontSize: 11 }}>Select an object in the Scene Outliner to inspect its properties.</span>
          </div>
        ) : (
          /* State 3: Object selected but no snapshot data yet (Phase 4 will populate this) */
          <div className="placeholder-box" style={{ flex: 1 }}>
            <span className="placeholder-box__title">
              {selectedObjectId}
            </span>
            <span>プロパティデータを読み込み中...</span>
            <span style={{ fontSize: 11 }}>
              Property data will appear here once object.getSnapshot is wired (Phase 4).
            </span>
          </div>
        )}
      </div>
    </div>
  );
}
