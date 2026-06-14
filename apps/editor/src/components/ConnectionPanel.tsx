/**
 * ConnectionPanel — bridge connection controls.
 *
 * P4: static / inert shell. The endpoint input has local uncontrolled state
 * but performs NO backend call. Buttons are rendered disabled.
 * P6 will lift the endpoint value, wire Connect/Disconnect handlers, and
 * supply live connection status.
 */

import type React from "react";
import { useState } from "react";

export type ConnectionStatus = "disconnected" | "connecting" | "connected" | "error";

const STATUS_LABELS: Record<ConnectionStatus, string> = {
  disconnected: "Disconnected",
  connecting:   "Connecting...",
  connected:    "Connected",
  error:        "Connection error",
};

const STATUS_CSS: Record<ConnectionStatus, string> = {
  disconnected: "status-badge--disconnected",
  connecting:   "status-badge--warning",
  connected:    "status-badge--connected",
  error:        "status-badge--error",
};

export interface ConnectionPanelProps {
  /** P6: live connection status from bridge */
  status?: ConnectionStatus;
  /** P6: initial or externally controlled endpoint value */
  defaultEndpoint?: string;
  /** P6: called when user clicks Connect */
  onConnect?: (endpoint: string) => void;
  /** P6: called when user clicks Disconnect */
  onDisconnect?: () => void;
  /** P4 default: controls disabled until P6 wires real handlers */
  disabled?: boolean;
}

export function ConnectionPanel({
  status = "disconnected",
  defaultEndpoint = "ws://127.0.0.1:9001",
  onConnect,
  onDisconnect,
  disabled = true,
}: ConnectionPanelProps): React.JSX.Element {
  // Local uncontrolled state for the input (P6 may lift this or replace entirely).
  const [endpoint, setEndpoint] = useState<string>(defaultEndpoint);

  const isConnected = status === "connected";

  function handleConnect(): void {
    onConnect?.(endpoint);
  }

  return (
    <div className="panel">
      <div className="panel__header">
        <span>Connection</span>
      </div>

      <div className="panel__body col">
        {/* Status indicator */}
        <div className="row">
          <span className="label">Status:</span>
          <span className={`status-badge ${STATUS_CSS[status]}`}>
            <span className="status-badge__dot" />
            {STATUS_LABELS[status]}
          </span>
        </div>

        <div className="divider" />

        {/* Endpoint input */}
        <div className="col" style={{ gap: 4 }}>
          <label className="label" htmlFor="conn-endpoint">
            Bridge endpoint
          </label>
          <input
            id="conn-endpoint"
            className="input"
            type="text"
            value={endpoint}
            onChange={(e) => setEndpoint(e.target.value)}
            placeholder="ws://127.0.0.1:9001"
            disabled={disabled || isConnected}
            spellCheck={false}
          />
        </div>

        {/* Connect / Disconnect */}
        <div className="row">
          <button
            className="btn btn--primary"
            type="button"
            disabled={disabled || isConnected}
            onClick={handleConnect}
          >
            Connect
          </button>
          <button
            className="btn btn--danger"
            type="button"
            disabled={disabled || !isConnected}
            onClick={onDisconnect}
          >
            Disconnect
          </button>
        </div>
      </div>
    </div>
  );
}
