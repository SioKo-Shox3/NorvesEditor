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
import { useEngineSettings } from '../hooks/useEngineSettings.js';
import { requestLayoutReset } from '../shell/layoutReset.js';
import type { EnginePathSource } from '@norves/bridge-ui';

const ENGINE_PATH_SOURCE_LABELS: Record<EnginePathSource, string> = {
  env: '環境変数 NORVES_ENGINE_PATH',
  settings: '保存した設定',
  default: '既定値',
};

// IDockviewPanelProps is accepted but not currently used for data.
// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export function SettingsPanel(_props: IDockviewPanelProps): React.JSX.Element {
  const state = useBridgeState();
  const actions = useBridgeActions();
  const engine = useEngineSettings();
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

        <div className="divider" />
        <section className="col settings-engine" aria-labelledby="settings-engine-title">
          <span id="settings-engine-title" className="label">エンジン</span>
          {engine.settings === undefined ? (
            <span className="settings-engine__value">
              {engine.busy ? '読み込み中…' : '設定を取得できません'}
            </span>
          ) : (
            <div className="col settings-engine__details">
              <div className="settings-engine__item">
                <span className="label">実行ファイル:</span>
                <span
                  className="settings-engine__value settings-engine__path"
                  data-testid="engine-effective-path"
                >
                  {engine.settings.effectivePath}
                </span>
              </div>
              <div className="settings-engine__item">
                <span className="label">出所:</span>
                <span className="settings-engine__value" data-testid="engine-path-source">
                  {ENGINE_PATH_SOURCE_LABELS[engine.settings.source]}
                </span>
              </div>
              {engine.settings.source === 'env' && (
                <p className="settings-engine__note" role="note">
                  環境変数 NORVES_ENGINE_PATH が設定されているため、保存した設定よりも環境変数が優先されています。
                  {engine.settings.savedPath !== null && (
                    <>
                      {' '}保存した設定:{' '}
                      <span className="settings-engine__path">{engine.settings.savedPath}</span>
                    </>
                  )}
                </p>
              )}
            </div>
          )}
          {engine.error !== undefined && (
            <div className="error-banner" role="alert">
              <span className="error-banner__kind">{engine.error.kind ?? 'error'}</span>
              <span className="error-banner__message">{engine.error.message}</span>
              <button
                className="error-banner__dismiss"
                type="button"
                onClick={engine.dismissError}
                aria-label="エラーを閉じる"
              >
                ×
              </button>
            </div>
          )}
          <div className="row">
            <button
              className="btn"
              type="button"
              disabled={engine.busy}
              onClick={engine.pick}
            >
              参照…
            </button>
            <button
              className="btn"
              type="button"
              disabled={engine.busy || engine.settings?.savedPath == null}
              onClick={engine.clear}
              title="保存したパスを消し、既定の実行ファイルを使う"
            >
              既定に戻す
            </button>
          </div>
          <div className="col settings-engine__args">
            <label className="label" htmlFor="settings-engine-args">
              起動引数(1 行に 1 つ)
            </label>
            <textarea
              id="settings-engine-args"
              className="settings-engine__args-input"
              rows={4}
              spellCheck={false}
              value={engine.argsDraft}
              disabled={engine.settings === undefined}
              onChange={(e) => engine.setArgsDraft(e.target.value)}
            />
            <p className="settings-engine__note">
              シェルを介さず、1 行をそのまま 1 つの引数として --bridge-port より前に渡します。空行は捨てます。
              --bridge-port はエディタが渡すので指定できません。
            </p>
            <div className="row settings-engine__args-actions">
              <button
                className="btn"
                type="button"
                disabled={engine.busy || engine.settings === undefined}
                onClick={engine.saveArgs}
              >
                引数を保存
              </button>
              {engine.argsSaved && (
                <span className="settings-engine__saved" role="status">
                  起動引数を保存しました
                </span>
              )}
            </div>
          </div>
        </section>
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
