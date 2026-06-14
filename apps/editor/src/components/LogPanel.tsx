/**
 * LogPanel — scrollable log output area.
 *
 * P4: static shell with empty placeholder.
 * P6 will supply a live log entries array via `entries` prop.
 */

import type React from "react";

export type LogLevel = "info" | "warn" | "error" | "debug";

export interface LogEntry {
  /** Monotonic sequence id (for React key). */
  id: number;
  level: LogLevel;
  message: string;
  /** ISO-8601 timestamp string, trimmed to HH:MM:SS for display. */
  timestamp: string;
}

export interface LogPanelProps {
  /** P6: live log entries from bridge/engine */
  entries?: readonly LogEntry[];
}

function formatTime(iso: string): string {
  // "2026-06-14T12:34:56.789Z" -> "12:34:56"
  const t = iso.split("T")[1];
  return t ? t.slice(0, 8) : iso;
}

export function LogPanel({ entries = [] }: LogPanelProps): React.JSX.Element {
  return (
    <div className="panel">
      <div className="panel__header">
        <span>Log</span>
        {entries.length > 0 && (
          <span style={{ marginLeft: "auto", fontWeight: 400, textTransform: "none", letterSpacing: 0 }}>
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
                <span className="log-entry__time">{formatTime(e.timestamp)}</span>
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
