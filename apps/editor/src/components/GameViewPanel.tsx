/**
 * GameViewPanel — primary control panel for the engine process.
 *
 * P4: all buttons are rendered disabled / inert (static shell).
 * P6 will supply real handlers and live state via props.
 *
 * NOTE: Alpha has NO embedded viewport. The engine renders in its own
 * external window. This panel drives the process and runtime only.
 */

import type React from "react";

/** Engine process lifecycle state label (placeholder for P6 state). */
export type EngineState = "stopped" | "starting" | "running" | "error";

/** Runtime / simulation state label (placeholder for P6 state). */
export type RuntimeState = "idle" | "playing" | "paused";

export interface GameViewPanelProps {
  /** P6: current engine process state */
  engineState?: EngineState;
  /** P6: current runtime simulation state */
  runtimeState?: RuntimeState;
  /** P6: process lifecycle handlers */
  onLaunch?: () => void;
  onStopProcess?: () => void;
  onReconnect?: () => void;
  /** P6: runtime simulation handlers */
  onPlay?: () => void;
  onPause?: () => void;
  onStopRuntime?: () => void;
  onFocusViewport?: () => void;
  /** P4 default: everything disabled until P6 wires state + handlers */
  disabled?: boolean;
}

export function GameViewPanel({
  engineState = "stopped",
  runtimeState = "idle",
  onLaunch,
  onStopProcess,
  onReconnect,
  onPlay,
  onPause,
  onStopRuntime,
  onFocusViewport,
  disabled = true,
}: GameViewPanelProps): React.JSX.Element {
  const engineLabel: Record<EngineState, string> = {
    stopped:  "Stopped",
    starting: "Starting...",
    running:  "Running",
    error:    "Error",
  };

  const runtimeLabel: Record<RuntimeState, string> = {
    idle:    "Idle",
    playing: "Playing",
    paused:  "Paused",
  };

  return (
    <div className="panel">
      <div className="panel__header">
        <span>Game View</span>
      </div>

      <div className="panel__body col">
        {/* Viewport notice */}
        <div className="placeholder-box">
          <span className="placeholder-box__title">Engine Viewport (External Window)</span>
          <span>
            In alpha, the engine renders in its own window.
            Use &quot;Focus Viewport&quot; to bring it to the foreground.
          </span>
        </div>

        {/* Status readout */}
        <div className="divider" />
        <div className="row" style={{ gap: 16 }}>
          <span className="label">Engine:</span>
          <StatusChip state={engineState} label={engineLabel[engineState]} />
          <span className="label" style={{ marginLeft: 8 }}>Runtime:</span>
          <StatusChip state={runtimeState} label={runtimeLabel[runtimeState]} />
        </div>

        {/* Process controls */}
        <div className="divider" />
        <div className="label">Process</div>
        <div className="row">
          <button
            className="btn btn--primary"
            disabled={disabled}
            onClick={onLaunch}
            type="button"
          >
            Launch
          </button>
          <button
            className="btn btn--danger"
            disabled={disabled}
            onClick={onStopProcess}
            type="button"
          >
            Stop
          </button>
          <button
            className="btn"
            disabled={disabled}
            onClick={onReconnect}
            type="button"
          >
            Reconnect
          </button>
        </div>

        {/* Runtime controls */}
        <div className="divider" />
        <div className="label">Runtime</div>
        <div className="row">
          <button
            className="btn"
            disabled={disabled}
            onClick={onPlay}
            type="button"
          >
            Play
          </button>
          <button
            className="btn"
            disabled={disabled}
            onClick={onPause}
            type="button"
          >
            Pause
          </button>
          <button
            className="btn"
            disabled={disabled}
            onClick={onStopRuntime}
            type="button"
          >
            Stop
          </button>
          <button
            className="btn"
            disabled={disabled}
            onClick={onFocusViewport}
            type="button"
          >
            Focus Viewport
          </button>
        </div>
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Internal sub-component                                               */
/* ------------------------------------------------------------------ */

type ChipState = EngineState | RuntimeState;

function chipClass(state: ChipState): string {
  if (state === "running" || state === "playing") return "status-badge--connected";
  if (state === "error")                           return "status-badge--error";
  if (state === "starting" || state === "paused")  return "status-badge--warning";
  return "status-badge--disconnected";
}

interface StatusChipProps {
  state: ChipState;
  label: string;
}

function StatusChip({ state, label }: StatusChipProps): React.JSX.Element {
  return (
    <span className={`status-badge ${chipClass(state)}`}>
      <span className="status-badge__dot" />
      {label}
    </span>
  );
}
