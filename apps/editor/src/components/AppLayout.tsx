/**
 * AppLayout — 4-panel static shell for NorvesEditor.
 *
 * Layout (CSS grid, desktop editor):
 *
 *   +---------------------------+------------------+
 *   |                           |                  |
 *   |        Game View          |   Connection     |
 *   |    (primary control)      |                  |
 *   |                           +------------------+
 *   |                           |                  |
 *   +---------------------------+   Settings       |
 *   |                           |                  |
 *   |           Log             |                  |
 *   |      (full width)         |                  |
 *   +---------------------------+------------------+
 *
 * Game View takes the dominant left column (~60 %).
 * Right sidebar has Connection (top) and Settings (bottom).
 * Log occupies the bottom strip across the full width.
 *
 * P6 will pass live state/handlers as props down to each panel.
 */

import type React from "react";
import { GameViewPanel } from "./GameViewPanel";
import { LogPanel }      from "./LogPanel";
import { ConnectionPanel } from "./ConnectionPanel";
import { SettingsPanel } from "./SettingsPanel";

const layoutStyle: React.CSSProperties = {
  display: "grid",
  gridTemplateColumns: "1fr 280px",
  gridTemplateRows:    "1fr 220px",
  gap:    "4px",
  padding: "4px",
  height: "100%",
  width:  "100%",
  overflow: "hidden",
};

const rightColumnStyle: React.CSSProperties = {
  display: "grid",
  gridTemplateRows: "1fr 1fr",
  gap: "4px",
  minHeight: 0,
};

export function AppLayout(): React.JSX.Element {
  return (
    <div style={layoutStyle}>
      {/* Top-left: Game View (spans 1 column, 1 row) */}
      <div style={{ gridColumn: "1", gridRow: "1", minHeight: 0, display: "flex", flexDirection: "column" }}>
        <GameViewPanel />
      </div>

      {/* Right sidebar: Connection + Settings stacked */}
      <div style={{ gridColumn: "2", gridRow: "1 / span 2", minHeight: 0, ...rightColumnStyle }}>
        <ConnectionPanel />
        <SettingsPanel />
      </div>

      {/* Bottom: Log (full left width) */}
      <div style={{ gridColumn: "1", gridRow: "2", minHeight: 0, display: "flex", flexDirection: "column" }}>
        <LogPanel />
      </div>
    </div>
  );
}
