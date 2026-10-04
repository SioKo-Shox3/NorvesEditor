// @vitest-environment jsdom
/**
 * ToolbarActions tests — main toolbar engine-control wiring.
 *
 * Strategy
 * --------
 * useBridgeActions and useBridgeState are vi.mock()-ed so tests run in jsdom
 * without a Tauri/BridgeProvider context.  Each test suite replaces the mock
 * return values to exercise a particular scenario.
 *
 * Scenarios covered
 * -----------------
 * (a) Button click → correct action function is called.
 * (b) disabled logic matches GameViewPanel exactly (the "正").
 *     - launchDisabled   : connected || connecting
 *     - stopProcessDisabled: !connected
 *     - reconnectDisabled  : connecting || disconnected || undefined
 *     - runtimeDisabled    : !connected
 * (c) View-toggle buttons are disabled when callback props are undefined.
 *     Connection/Settings/Log are wired (P5/P4); Reset Layout wired in P6.
 * (d) StatusBadge renders with the current connection status.
 */

import { describe, it, expect, vi, afterEach, type Mock } from 'vitest';
import { render, screen, cleanup, fireEvent } from '@testing-library/react';
import { ToolbarActions } from '../ToolbarActions.js';
import type { BridgeActions } from '../../../hooks/useBridge.js';
import type { BridgeState } from '../../../state/store.js';
import { bridgeReducer } from '../../../state/store.js';
import type { EditHistorySummary } from '@norves/bridge-ui';

// -------------------------------------------------------------------------
// Module mocks
// -------------------------------------------------------------------------

vi.mock('../../../hooks/useBridge.js', () => ({
  useBridgeActions: vi.fn(),
}));

vi.mock('../../../state/BridgeContext.js', () => ({
  useBridgeState: vi.fn(),
}));

// Lazy imports after mocking so we get the mocked versions.
const { useBridgeActions } = await import('../../../hooks/useBridge.js');
const { useBridgeState }   = await import('../../../state/BridgeContext.js');

// -------------------------------------------------------------------------
// Helper factories
// -------------------------------------------------------------------------

function makeActions(overrides: Partial<BridgeActions> = {}): BridgeActions {
  return {
    openWorkspace:      vi.fn().mockResolvedValue(undefined),
    getWorkspace:       vi.fn().mockResolvedValue(undefined),
    closeWorkspace:     vi.fn().mockResolvedValue(undefined),
    readAssetManifest:  vi.fn().mockResolvedValue(undefined),
    reloadAssetRuntime: vi.fn().mockResolvedValue(undefined),
    dismissAssetReloadError: vi.fn(),
    resolveAsset:       vi.fn().mockResolvedValue(undefined),
    selectAsset:        vi.fn(),
    clearAssetManifest: vi.fn(),
    dismissAssetError:  vi.fn(),
    connect:           vi.fn().mockResolvedValue(undefined),
    disconnect:        vi.fn().mockResolvedValue(undefined),
    reconnect:         vi.fn().mockResolvedValue(undefined),
    getStatus:         vi.fn().mockResolvedValue(undefined),
    getSceneTree:      vi.fn().mockResolvedValue(undefined),
    createObject:      vi.fn().mockResolvedValue({ accepted: true }),
    deleteObject:      vi.fn().mockResolvedValue({ accepted: true }),
    reparentObject:    vi.fn().mockResolvedValue({ accepted: true }),
    duplicateObject:   vi.fn().mockResolvedValue({ accepted: true }),
    getObjectSnapshot: vi.fn().mockResolvedValue(undefined),
    getComponentSnapshot: vi.fn().mockResolvedValue(undefined),
    addComponent: vi.fn().mockResolvedValue(false),
    removeComponent: vi.fn().mockResolvedValue(false),
    selectComponent: vi.fn(),
    getSchemaSnapshot: vi.fn().mockResolvedValue(undefined),
    setObjectProperty: vi.fn().mockResolvedValue({ accepted: true }),
    getViewportThumbnail: vi.fn().mockResolvedValue('ok' as const),
    play:              vi.fn().mockResolvedValue(undefined),
    pause:             vi.fn().mockResolvedValue(undefined),
    stop:              vi.fn().mockResolvedValue(undefined),
    focusViewport:     vi.fn().mockResolvedValue(undefined),
    launch:            vi.fn().mockResolvedValue(undefined),
    stopProcess:       vi.fn().mockResolvedValue(undefined),
    dismissError:      vi.fn(),
    selectObject:      vi.fn(),
    undo:              vi.fn().mockResolvedValue(undefined),
    redo:              vi.fn().mockResolvedValue(undefined),
    retryPendingEdit:   vi.fn().mockResolvedValue(undefined),
    discardPendingEdit: vi.fn().mockResolvedValue(undefined),
    refreshEditHistory: vi.fn().mockResolvedValue(undefined),
    dismissEditDiscardResult: vi.fn(),
    ...overrides,
  };
}

function makeState(
  status: BridgeState['connection']['status'],
  overrides: Partial<BridgeState> = {},
): BridgeState {
  return {
    connection: { status },
    ...overrides,
  } as BridgeState;
}

function makeHistory(overrides: Partial<EditHistorySummary> = {}): EditHistorySummary {
  return {
    generation: 3,
    historyRevision: 0,
    appliedRevision: 0,
    canUndo: false,
    canRedo: false,
    undoHeadId: null,
    undoRevision: 0,
    undoGroup: null,
    redoHeadId: null,
    redoRevision: 0,
    redoGroup: null,
    pending: false,
    ...overrides,
  };
}

function setup(
  status: BridgeState['connection']['status'],
  actionOverrides: Partial<BridgeActions> = {},
  props: Parameters<typeof ToolbarActions>[0] = {},
  stateOverrides: Partial<BridgeState> = {},
) {
  const actions = makeActions(actionOverrides);
  let state = makeState(status, stateOverrides);
  (useBridgeActions as Mock).mockReturnValue(actions);
  (useBridgeState   as Mock).mockReturnValue(state);
  const { rerender } = render(<ToolbarActions {...props} />);
  const setState = (next: BridgeState): void => {
    state = next;
    (useBridgeState as Mock).mockReturnValue(state);
    rerender(<ToolbarActions {...props} />);
  };
  return { actions, rerender, setState };
}

afterEach(cleanup);

// -------------------------------------------------------------------------
// (a) Click → action called
// -------------------------------------------------------------------------

describe('ToolbarActions click wiring', () => {
  it('calls actions.launch() when Launch is clicked', () => {
    const { actions } = setup('disconnected');
    fireEvent.click(screen.getByRole('button', { name: 'Launch engine' }));
    expect(actions.launch).toHaveBeenCalledOnce();
  });

  it('calls actions.stopProcess() when Stop Process is clicked', () => {
    const { actions } = setup('connected');
    fireEvent.click(screen.getByRole('button', { name: 'Stop process' }));
    expect(actions.stopProcess).toHaveBeenCalledOnce();
  });

  it('calls actions.reconnect() when Reconnect is clicked (status=connected)', () => {
    const { actions } = setup('connected');
    fireEvent.click(screen.getByRole('button', { name: 'Reconnect' }));
    expect(actions.reconnect).toHaveBeenCalledOnce();
  });

  it('calls actions.reconnect() when Reconnect is clicked (status=error)', () => {
    const { actions } = setup('error');
    fireEvent.click(screen.getByRole('button', { name: 'Reconnect' }));
    expect(actions.reconnect).toHaveBeenCalledOnce();
  });

  it('calls actions.play() when Play is clicked', () => {
    const { actions } = setup('connected');
    fireEvent.click(screen.getByRole('button', { name: 'Play' }));
    expect(actions.play).toHaveBeenCalledOnce();
  });

  it('calls actions.pause() when Pause is clicked', () => {
    const { actions } = setup('connected');
    fireEvent.click(screen.getByRole('button', { name: 'Pause' }));
    expect(actions.pause).toHaveBeenCalledOnce();
  });

  it('calls actions.stop() when Stop runtime is clicked', () => {
    const { actions } = setup('connected');
    fireEvent.click(screen.getByRole('button', { name: 'Stop runtime' }));
    expect(actions.stop).toHaveBeenCalledOnce();
  });

  it('calls actions.focusViewport() when Focus Viewport is clicked', () => {
    const { actions } = setup('connected');
    fireEvent.click(screen.getByRole('button', { name: 'Focus Viewport' }));
    expect(actions.focusViewport).toHaveBeenCalledOnce();
  });
});

// -------------------------------------------------------------------------
// (b) disabled logic — mirrors GameViewPanel (the "正")
// -------------------------------------------------------------------------

describe('ToolbarActions disabled: Launch', () => {
  it('is enabled when disconnected', () => {
    setup('disconnected');
    expect((screen.getByRole('button', { name: 'Launch engine' }) as HTMLButtonElement).disabled).toBe(false);
  });

  it('is disabled when connected', () => {
    setup('connected');
    expect((screen.getByRole('button', { name: 'Launch engine' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('is disabled when connecting', () => {
    setup('connecting');
    expect((screen.getByRole('button', { name: 'Launch engine' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('is enabled when error', () => {
    setup('error');
    expect((screen.getByRole('button', { name: 'Launch engine' }) as HTMLButtonElement).disabled).toBe(false);
  });
});

describe('ToolbarActions disabled: Stop Process', () => {
  it('is disabled when disconnected', () => {
    setup('disconnected');
    expect((screen.getByRole('button', { name: 'Stop process' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('is enabled when connected', () => {
    setup('connected');
    expect((screen.getByRole('button', { name: 'Stop process' }) as HTMLButtonElement).disabled).toBe(false);
  });

  it('is disabled when connecting', () => {
    setup('connecting');
    expect((screen.getByRole('button', { name: 'Stop process' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('is disabled when error', () => {
    setup('error');
    expect((screen.getByRole('button', { name: 'Stop process' }) as HTMLButtonElement).disabled).toBe(true);
  });
});

describe('ToolbarActions disabled: Reconnect', () => {
  // Reconnect enabled only when status is 'connected' or 'error'
  // (GameViewPanel: disabled when connecting || disconnected || undefined)

  it('is disabled when disconnected', () => {
    setup('disconnected');
    expect((screen.getByRole('button', { name: 'Reconnect' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('is disabled when connecting', () => {
    setup('connecting');
    expect((screen.getByRole('button', { name: 'Reconnect' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('is enabled when connected', () => {
    setup('connected');
    expect((screen.getByRole('button', { name: 'Reconnect' }) as HTMLButtonElement).disabled).toBe(false);
  });

  it('is enabled when error', () => {
    setup('error');
    expect((screen.getByRole('button', { name: 'Reconnect' }) as HTMLButtonElement).disabled).toBe(false);
  });
});

describe('ToolbarActions disabled: runtime buttons', () => {
  const runtimeBtns = ['Play', 'Pause', 'Stop runtime', 'Focus Viewport'];

  it.each(runtimeBtns)('%s is disabled when not connected', (label) => {
    setup('disconnected');
    expect((screen.getByRole('button', { name: label }) as HTMLButtonElement).disabled).toBe(true);
  });

  it.each(runtimeBtns)('%s is enabled when connected', (label) => {
    setup('connected');
    expect((screen.getByRole('button', { name: label }) as HTMLButtonElement).disabled).toBe(false);
  });

  it.each(['Play', 'Pause', 'Stop runtime'])('%s is disabled while an edit group is pending', (label) => {
    setup('connected', {}, {}, { editHistorySummary: makeHistory({ pending: true }) });
    expect((screen.getByRole('button', { name: label }) as HTMLButtonElement).disabled).toBe(true);
  });
});

// -------------------------------------------------------------------------
// (c) View-toggle buttons are disabled when callbacks are undefined (P3 stubs)
// -------------------------------------------------------------------------

describe('ToolbarActions view-toggle disabled-when-unwired', () => {
  const viewBtnLabels = [
    'Open Connection window',
    'Toggle Log',
    'Open Settings window',
    'Reset Layout',
  ] as const;

  it('all view-toggle buttons are disabled when no callbacks are provided', () => {
    setup('connected');
    for (const label of viewBtnLabels) {
      expect((screen.getByRole('button', { name: label }) as HTMLButtonElement).disabled).toBe(true);
    }
  });

  it('Connection button is enabled when onOpenConnection is provided', () => {
    const onOpen = vi.fn();
    setup('connected', {}, { onOpenConnection: onOpen });
    const btn = screen.getByRole('button', { name: 'Open Connection window' }) as HTMLButtonElement;
    expect(btn.disabled).toBe(false);
    fireEvent.click(btn);
    expect(onOpen).toHaveBeenCalledOnce();
  });

  it('Log button is enabled when onToggleLog is provided', () => {
    const onToggle = vi.fn();
    setup('connected', {}, { onToggleLog: onToggle });
    const btn = screen.getByRole('button', { name: 'Toggle Log' }) as HTMLButtonElement;
    expect(btn.disabled).toBe(false);
    fireEvent.click(btn);
    expect(onToggle).toHaveBeenCalledOnce();
  });

  it('Settings button is enabled when onOpenSettings is provided', () => {
    const onSettings = vi.fn();
    setup('connected', {}, { onOpenSettings: onSettings });
    const btn = screen.getByRole('button', { name: 'Open Settings window' }) as HTMLButtonElement;
    expect(btn.disabled).toBe(false);
    fireEvent.click(btn);
    expect(onSettings).toHaveBeenCalledOnce();
  });

  it('Reset Layout button is enabled when onResetLayout is provided', () => {
    const onReset = vi.fn();
    setup('connected', {}, { onResetLayout: onReset });
    const btn = screen.getByRole('button', { name: 'Reset Layout' }) as HTMLButtonElement;
    expect(btn.disabled).toBe(false);
    fireEvent.click(btn);
    expect(onReset).toHaveBeenCalledOnce();
  });
});

// -------------------------------------------------------------------------
// (e) Undo / Redo (Phase U1)
// -------------------------------------------------------------------------

describe('ToolbarActions Undo/Redo', () => {
  const undoBtn = (): HTMLButtonElement =>
    screen.getByRole('button', { name: 'Undo' }) as HTMLButtonElement;
  const redoBtn = (): HTMLButtonElement =>
    screen.getByRole('button', { name: 'Redo' }) as HTMLButtonElement;

  it('Undo is disabled when the history summary says it is empty', () => {
    setup('connected', {}, {}, { editHistorySummary: makeHistory() });
    expect(undoBtn().disabled).toBe(true);
  });

  it('Undo is enabled when the service summary permits it', () => {
    setup('connected', {}, {}, { editHistorySummary: makeHistory({ canUndo: true, undoHeadId: 12 }) });
    expect(undoBtn().disabled).toBe(false);
  });

  it('Undo is disabled when disconnected even if the summary permits it', () => {
    setup('disconnected', {}, {}, { editHistorySummary: makeHistory({ canUndo: true, undoHeadId: 12 }) });
    expect(undoBtn().disabled).toBe(true);
  });

  it('Undo is disabled when scene edit is unsupported', () => {
    setup(
      'connected',
      {},
      {},
      { editHistorySummary: makeHistory({ canUndo: true, undoHeadId: 12 }), sceneEditUnsupported: true },
    );
    expect(undoBtn().disabled).toBe(true);
  });

  it('Redo is disabled when the history summary says it is empty', () => {
    setup('connected', {}, {}, { editHistorySummary: makeHistory() });
    expect(redoBtn().disabled).toBe(true);
  });

  it('Redo is enabled when the service summary permits it', () => {
    setup('connected', {}, {}, { editHistorySummary: makeHistory({ canRedo: true, redoHeadId: 15 }) });
    expect(redoBtn().disabled).toBe(false);
  });

  it('Redo is disabled when scene edit is unsupported', () => {
    setup(
      'connected',
      {},
      {},
      { editHistorySummary: makeHistory({ canRedo: true, redoHeadId: 15 }), sceneEditUnsupported: true },
    );
    expect(redoBtn().disabled).toBe(true);
  });

  it('clicking Undo calls actions.undo()', () => {
    const { actions } = setup(
      'connected',
      {},
      {},
      { editHistorySummary: makeHistory({ canUndo: true, undoHeadId: 12 }) },
    );
    fireEvent.click(undoBtn());
    expect(actions.undo).toHaveBeenCalledOnce();
  });

  it('clicking Redo calls actions.redo()', () => {
    const { actions } = setup(
      'connected',
      {},
      {},
      { editHistorySummary: makeHistory({ canRedo: true, redoHeadId: 15 }) },
    );
    fireEvent.click(redoBtn());
    expect(actions.redo).toHaveBeenCalledOnce();
  });

  it('履歴要約イベントで取り消しボタンと先頭まとまり情報を更新する', () => {
    const { setState } = setup('connected');
    expect(undoBtn().disabled).toBe(true);

    const withUndo = bridgeReducer(
      makeState('connected'),
      {
        type: 'editHistorySummaryReceived',
        summary: makeHistory({
          canUndo: true,
          undoHeadId: 12,
          canRedo: true,
          redoHeadId: 13,
          undoGroup: {
            id: 'group-undo',
            name: '火花の色を調整',
            source: 'mcp',
            count: 12,
            createdAt: 10,
          },
          redoGroup: {
            id: 'group-redo',
            name: '照明強度を戻す',
            source: 'ui',
            count: 2,
            createdAt: 11,
          },
        }),
      },
    );
    setState(withUndo);
    expect(undoBtn().disabled).toBe(false);
    expect(screen.getByText('火花の色を調整')).toBeTruthy();
    expect(screen.getByText('（MCP・12件）')).toBeTruthy();
    expect(screen.getByText('照明強度を戻す')).toBeTruthy();
    expect(screen.getByText('（画面操作・2件）')).toBeTruthy();

    const pending = bridgeReducer(
      withUndo,
      { type: 'editHistorySummaryReceived', summary: makeHistory({ canUndo: true, undoHeadId: 12, pending: true }) },
    );
    setState(pending);
    expect(undoBtn().disabled).toBe(true);
  });

  it('保留中のまとまり情報と再試行・破棄を表示する', () => {
    setup('connected', {}, {}, {
      editHistorySummary: makeHistory({
        pending: true,
        pendingGroup: {
          id: 'edit-3-12',
          name: '3つの値を変更',
          direction: 'undo',
          source: 'ui',
          createdAt: 12,
          totalCount: 3,
          completedCount: 1,
          outcomeUnknown: false,
          retryAllowed: true,
        },
      }),
    });

    expect(screen.getByRole('alertdialog').textContent).toContain('3つの値を変更');
    expect(screen.getByText('出どころ: 画面操作')).toBeTruthy();
    expect(screen.getByText('1 / 3 件を処理済み')).toBeTruthy();
    expect(screen.getByRole('button', { name: '再試行' })).toBeTruthy();
    expect(screen.getByRole('button', { name: '破棄' })).toBeTruthy();
    expect(document.querySelector('.history-problem-shield')).not.toBeNull();
  });

  it('結果不明では再試行を表示しない', () => {
    setup('connected', {}, {}, {
      editHistorySummary: makeHistory({
        pending: true,
        pendingGroup: {
          id: 'edit-3-13',
          name: '不明な結果のまとまり',
          direction: 'redo',
          source: 'mcp',
          createdAt: 13,
          totalCount: 2,
          completedCount: 1,
          outcomeUnknown: true,
          retryAllowed: false,
        },
      }),
    });

    expect(screen.queryByRole('button', { name: '再試行' })).toBeNull();
    expect(screen.getByRole('button', { name: '状態を確認' })).toBeTruthy();
    expect(screen.getByRole('button', { name: '破棄' })).toBeTruthy();
    expect(screen.getByText(/結果は不明です/)).toBeTruthy();
  });

  it('MCPの単発undo拒否を0件処理済みで表示し、破棄後に操作を再開できる', () => {
    const { actions, setState } = setup('connected', {}, {}, {
      editHistorySummary: makeHistory({ pending: true, pendingGroup: {
        id: 'edit-3-14', name: '値を設定', direction: 'undo', source: 'mcp', createdAt: 14,
        totalCount: 1, completedCount: 0, outcomeUnknown: false, retryAllowed: true,
      } }),
    });
    expect(screen.getByText('0 / 1 件を処理済み')).toBeTruthy();
    expect(screen.getByText('出どころ: MCP')).toBeTruthy();
    expect(screen.getByRole('button', { name: '再試行' })).toBeTruthy();
    expect(undoBtn().disabled).toBe(true);
    fireEvent.click(screen.getByRole('button', { name: '破棄' }));
    expect(actions.discardPendingEdit).toHaveBeenCalledOnce();
    setState({ ...makeState('connected'), editHistorySummary: makeHistory({ canUndo: true, undoHeadId: 15 }) });
    expect(screen.queryByRole('alertdialog')).toBeNull();
    expect(undoBtn().disabled).toBe(false);
  });
});

// -------------------------------------------------------------------------
// (d) StatusBadge renders the connection status
// -------------------------------------------------------------------------

describe('ToolbarActions StatusBadge', () => {
  it('renders Connected status badge when status=connected', () => {
    setup('connected');
    expect(screen.getByRole('status', { name: /connected/i })).toBeTruthy();
  });

  it('renders Disconnected status badge when status=disconnected', () => {
    setup('disconnected');
    expect(screen.getByRole('status', { name: /disconnected/i })).toBeTruthy();
  });

  it('renders Connecting status badge when status=connecting', () => {
    setup('connecting');
    expect(screen.getByRole('status', { name: /connecting/i })).toBeTruthy();
  });

  it('renders Error status badge when status=error', () => {
    setup('error');
    expect(screen.getByRole('status', { name: /error/i })).toBeTruthy();
  });
});
