/**
 * LogPanel — scrollable log output area.
 *
 * Phase 1 refactor: props drilling removed. Log entries are now
 * obtained directly via useBridgeState().
 * Rendering logic is unchanged from the original implementation.
 */

import type React from 'react';
import type { IDockviewPanelProps } from 'dockview-react';
import { useBridgeState } from '../state/BridgeContext.js';
import type { LogEntry } from '../state/store.js';

function formatTime(iso: string): string {
  // "2026-06-14T12:34:56.789Z" -> "12:34:56"
  const t = iso.split('T')[1];
  return t !== undefined ? t.slice(0, 8) : iso;
}

// IDockviewPanelProps is accepted but not currently used for data.
// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export function LogPanel(_props: IDockviewPanelProps): React.JSX.Element {
  const state = useBridgeState();
  const entries: readonly LogEntry[] = state.logs;

  return (
    <div className="panel">
      <div className="panel__header">
        <span>Log</span>
        {entries.length > 0 && (
          <span style={{ marginLeft: 'auto', fontWeight: 400, textTransform: 'none', letterSpacing: 0 }}>
            {entries.length} entries
          </span>
        )}
      </div>

      <div className="panel__body">
        {entries.length === 0 ? (
          <ul className="log-list">
            <li className="log-list__empty">No logs yet.</li>
          </ul>
        ) : (
          <ul className="log-list">
            {entries.map((e) => (
              <li key={e.id} className={`log-entry log-entry--${e.level}`}>
                {e.timestamp !== undefined && (
                  <span className="log-entry__time">{formatTime(e.timestamp)}</span>
                )}
                <span className="log-entry__level">{e.level.toUpperCase()}</span>
                <span className="log-entry__msg">{e.message}</span>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
