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
import { useMcpSettings } from '../hooks/useMcpSettings.js';
import { requestLayoutReset } from '../shell/layoutReset.js';
import type { EnginePathSource } from '@norves/bridge-ui';

const ENGINE_PATH_SOURCE_LABELS: Record<EnginePathSource, string> = {
  env: '環境変数 NORVES_ENGINE_PATH',
  settings: '保存した設定',
  default: '既定値',
};

const MCP_STATE_LABELS = {
  disabled: '停止中',
  running: '待ち受け中',
  bindFailed: '待ち受けに失敗',
  storageFailed: '秘密の保存に失敗',
} as const;

// IDockviewPanelProps is accepted but not currently used for data.
// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export function SettingsPanel(_props: IDockviewPanelProps): React.JSX.Element {
  const state = useBridgeState();
  const actions = useBridgeActions();
  const engine = useEngineSettings();
  const mcp = useMcpSettings();
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

  function handleRegenerateMcpToken(): void {
    const confirmed = window.confirm(
      'MCPトークンを再生成します。現在のトークンによる接続、確認待ち、開いている編集まとまりは無効になります。接続先の設定を新しいトークンへ更新し、MCPクライアントを再接続してください。続けますか？',
    );
    if (confirmed) mcp.regenerateToken();
  }

  const workspace = state.workspace;
  const canOpenWorkspace = workspacePath.trim().length > 0;
  const mcpPort = mcp.settings?.port ?? 49770;
  const mcpEndpoint = `http://127.0.0.1:${mcpPort}/mcp`;
  const mcpClientCommand = `claude mcp add --transport http norves-editor ${mcpEndpoint} --header "Authorization: Bearer <Settingsで表示したトークン>"`;

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

        <div className="divider" />
        <section className="col settings-mcp" aria-labelledby="settings-mcp-title">
          <span id="settings-mcp-title" className="label">MCP 接続</span>
          <p className="settings-mcp__note">
            MCP対応クライアントからNorvesEditorへ接続します。接続先はこのPCの127.0.0.1だけで待ち受けます。
          </p>
          {mcp.settings === undefined ? (
            <span className="settings-mcp__value">
              {mcp.busy ? '設定を読み込み中…' : '設定を取得できません'}
            </span>
          ) : (
            <>
              <label className="settings-mcp__toggle">
                <input
                  type="checkbox"
                  checked={mcp.enabledDraft}
                  disabled={mcp.busy || mcp.tokenBusy}
                  onChange={(event) => mcp.setEnabledDraft(event.target.checked)}
                />
                MCPサーバーを有効にする
              </label>
              <label className="col settings-mcp__port" htmlFor="settings-mcp-port">
                <span className="label">待ち受けポート</span>
                <input
                  id="settings-mcp-port"
                  className="input"
                  type="number"
                  min={1}
                  max={65535}
                  step={1}
                  value={mcp.portDraft}
                  disabled={mcp.busy || mcp.tokenBusy}
                  onChange={(event) => mcp.setPortDraft(event.target.value)}
                />
              </label>
              <div className="row settings-mcp__actions">
                <button
                  className="btn btn--primary"
                  type="button"
                  disabled={mcp.busy || mcp.tokenBusy}
                  onClick={mcp.saveSettings}
                >
                  設定を適用
                </button>
                <span className="settings-mcp__value" aria-live="polite" data-testid="mcp-state">
                  接続状態: {MCP_STATE_LABELS[mcp.settings.state]}
                </span>
              </div>
              <div className="settings-mcp__item">
                <span className="label">接続先:</span>
                <code className="settings-mcp__value" data-testid="mcp-endpoint">
                  {mcp.settings.endpoint ?? mcpEndpoint}
                </code>
              </div>
              {mcp.settings.error !== undefined && (
                <div className="error-banner" role="alert">
                  <span className="error-banner__message">{mcp.settings.error}</span>
                </div>
              )}
              <div className="col settings-mcp__token-section">
                <span className="label">認証トークン</span>
                <p className="settings-mcp__note">
                  トークンは通常の設定取得では読み込みません。必要なときに表示し、クライアントの認証情報として設定してください。
                </p>
                {mcp.token !== undefined && (
                  <code className="settings-mcp__token" data-testid="mcp-token">
                    {mcp.token}
                  </code>
                )}
                <div className="row settings-mcp__actions">
                  {mcp.token !== undefined ? (
                    <button className="btn" type="button" onClick={mcp.hideToken}>
                      トークンを隠す
                    </button>
                  ) : (
                    <button
                      className="btn"
                      type="button"
                      disabled={mcp.busy || mcp.settings === undefined}
                      onClick={mcp.tokenBusy ? mcp.hideToken : mcp.showToken}
                    >
                      {mcp.tokenBusy ? '表示をキャンセル' : 'トークンを表示'}
                    </button>
                  )}
                  <button
                    className="btn"
                    type="button"
                    disabled={mcp.busy || mcp.tokenBusy}
                    onClick={handleRegenerateMcpToken}
                  >
                    トークンを再生成
                  </button>
                </div>
                <p className="settings-mcp__impact">
                  再生成すると以前のトークン、認証中の接続、確認待ち、開いている編集まとまりが無効になります。接続先のトークンを更新して、クライアントを再接続してください。
                </p>
              </div>
            </>
          )}
          {mcp.error !== undefined && (
            <div className="error-banner" role="alert">
              {mcp.error.kind !== undefined && (
                <span className="error-banner__kind">{mcp.error.kind}</span>
              )}
              <span className="error-banner__message">{mcp.error.message}</span>
              <button
                className="error-banner__dismiss"
                type="button"
                onClick={mcp.dismissError}
                aria-label="MCPエラーを閉じる"
              >
                ×
              </button>
            </div>
          )}
          <div className="col settings-mcp__instructions">
            <span className="label">接続手順</span>
            <ol className="settings-mcp__steps">
              <li>MCPサーバーを有効にして設定を適用します。状態が「待ち受け中」になると接続できます。</li>
              <li>「トークンを表示」を押し、下のコマンドの <code>&lt;Settingsで表示したトークン&gt;</code> を実際の値に置き換えてClaude Codeに登録します。</li>
              <li>接続名は <code>norves-editor</code> です。トークン再生成後は登録先の認証情報を新しい値へ更新し、クライアントを再接続します。</li>
            </ol>
            <code className="settings-mcp__command" data-testid="mcp-command">
              {mcpClientCommand}
            </code>
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
