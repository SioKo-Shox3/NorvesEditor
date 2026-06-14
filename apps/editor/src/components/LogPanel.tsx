/**
 * LogPanel — scrollable log output area.
 *
 * P6: renders live log entries from the bridge state store.
 * LogEntry type comes from the state store (which uses LogLevel from @norves/bridge-types).
 */

import type React from 'react';
import type { LogEntry } from '../state/store.js';

export interface LogPanelProps {
  /** Live log entries from bridge state store. */
  entries?: readonly LogEntry[];
}

function formatTime(iso: string): string {
  // "2026-06-14T12:34:56.789Z" -> "12:34:56"
  const t = iso.split('T')[1];
  return t !== undefined ? t.slice(0, 8) : iso;
}

export function LogPanel({ entries = [] }: LogPanelProps): React.JSX.Element {
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
