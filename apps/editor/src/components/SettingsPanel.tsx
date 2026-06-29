/**
 * SettingsPanel — editor settings and layout controls.
 *
 * P6: Settings is rendered ONLY in its own Tauri window (SecondaryWindowRoot);
 * it is no longer a panel in the main window's dockview. The layout it resets
 * lives in the MAIN window, so the reset button here cannot clear localStorage
 * or reload locally — that would touch the wrong window. Instead it emits a
 * frontend layout-reset request (requestLayoutReset); the main window listens
 * for it and performs the actual clear + reload (see shell/layoutReset.ts).
 * This avoids relying on shared localStorage between windows.
 *
 * IDockviewPanelProps is accepted but containerApi is not used here; the reset
 * works via the cross-window event, not the dockview API.
 */

import type React from 'react';
import { useEffect, useState } from 'react';
import type { IDockviewPanelProps } from 'dockview-react';
import { useBridgeState } from '../state/BridgeContext.js';
import { useBridgeActions } from '../hooks/useBridge.js';
import { requestLayoutReset } from '../shell/layoutReset.js';

// IDockviewPanelProps is accepted but not currently used for data.
// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export function SettingsPanel(_props: IDockviewPanelProps): React.JSX.Element {
  const state = useBridgeState();
  const actions = useBridgeActions();
  const [workspacePath, setWorkspacePath] = useState(state.workspace?.rootPath ?? '');

  // The Settings window mounts its own BridgeProvider, so the store starts empty
  // even when the backend already holds an open workspace. Rehydrate from the
  // backend on mount so Current workspace / Close reflect the real state.
  const { getWorkspace } = actions;
  useEffect(() => {
    void getWorkspace();
  }, [getWorkspace]);

  // Keep the path input in sync with the resolved workspace (e.g. after the
  // mount rehydrate above), without clobbering it back to empty on close.
  useEffect(() => {
    if (state.workspace) {
      setWorkspacePath(state.workspace.rootPath);
    }
  }, [state.workspace]);

  function handleResetLayout(): void {
    // Fire-and-forget: emit a reset request to the main window. We do NOT touch
    // this window's localStorage or reload it — the main window owns the layout
    // and performs the actual reset on receiving the event. A failed emit is
    // non-fatal (the main toolbar's Reset Layout button is an alternative path).
    void requestLayoutReset().catch((err: unknown) => {
      console.error('[SettingsPanel] Failed to request layout reset:', err);
    });
  }

  function handleWorkspacePathChange(e: React.ChangeEvent<HTMLInputElement>): void {
    setWorkspacePath(e.target.value);
  }

  function handleOpenWorkspace(): void {
    void actions.openWorkspace(workspacePath);
  }

  function handleCloseWorkspace(): void {
    void actions.closeWorkspace();
  }

  const workspace = state.workspace;
  const canOpenWorkspace = workspacePath.trim().length > 0;

  return (
    <div className="panel">
      <div className="panel__header">
        <span>Settings</span>
      </div>

      <div className="panel__body col">
        <div className="col" style={{ gap: 4 }}>
          <label className="label" htmlFor="workspace-root">
            Workspace root
          </label>
          <input
            id="workspace-root"
            className="input"
            type="text"
            value={workspacePath}
            onChange={handleWorkspacePathChange}
            placeholder="C:/Projects/Game"
            spellCheck={false}
          />
          <div className="row">
            <button
              className="btn btn--primary"
              type="button"
              disabled={!canOpenWorkspace}
              onClick={handleOpenWorkspace}
            >
              Open Workspace
            </button>
            <button
              className="btn"
              type="button"
              disabled={workspace === undefined}
              onClick={handleCloseWorkspace}
            >
              Close Workspace
            </button>
          </div>
        </div>

        <div className="divider" />
        <div className="col" style={{ gap: 4 }}>
          <span className="label">Current workspace</span>
          {workspace === undefined ? (
            <span style={{ fontSize: 12 }}>None</span>
          ) : (
            <div className="col" style={{ gap: 2 }}>
              <div className="row">
                <span className="label">Name:</span>
                <span style={{ fontSize: 12 }}>{workspace.name}</span>
              </div>
              <div className="row">
                <span className="label">Root:</span>
                <span style={{ fontSize: 12, wordBreak: 'break-all' }}>
                  {workspace.rootPath}
                </span>
              </div>
              <div className="row">
                <span className="label">Assets:</span>
                <span style={{ fontSize: 12, wordBreak: 'break-all' }}>
                  {workspace.assetsRoot}
                </span>
              </div>
            </div>
          )}
        </div>

        {/* Layout reset — relays the request to the main window (P6). */}
        <div className="divider" />
        <div className="col" style={{ gap: 4 }}>
          <span className="label">Layout</span>
          <button
            className="btn"
            type="button"
            onClick={handleResetLayout}
            title="Delete the main window's saved layout and restore defaults"
          >
            レイアウトをリセット
          </button>
        </div>
      </div>
    </div>
  );
}
