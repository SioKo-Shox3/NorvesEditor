// @vitest-environment jsdom
/**
 * GameViewPanel component tests — Phase 1 hook-mock refactor (revised).
 *
 * GameViewPanel obtains state via useBridgeState() and command callbacks via
 * useBridgeActions() (no props drilling, no inline invokeCommand).
 *
 * We therefore:
 *  1. Mock the BridgeContext hook useBridgeState() to supply controlled state
 *     without a real Provider.
 *  2. Mock useBridgeActions() to return spied action callbacks, so we can
 *     assert which action a button fires (e.g. dismissError).
 *  3. Mock dockview-react to avoid browser API requirements (ResizeObserver etc.)
 *
 * Covers:
 *   - Error banner renders when lastError is present.
 *   - Error banner absent when no lastError.
 *   - Dismiss button click fires actions.dismissError().
 *   - Regression (M2): notConnected kind with [object Object] message
 *     shows humanized label, does NOT render "[object Object]".
 *   - viewportState badge renders the value.
 *   - viewport thumbnail (Phase 7b).
 *   - thumbnail auto-refresh back-off (P7): exponential back-off on error,
 *     reset on ok, manual Refresh, unsupported stops the loop.
 *
 * P4: the engine/runtime/process control buttons (Launch / Stop Process /
 * Reconnect / Play / Pause / Stop / Focus Viewport) were moved to the main
 * toolbar (ToolbarActions, P3); their behaviour is covered by
 * ToolbarActions.test.tsx. This panel no longer renders them, so the former
 * Reconnect-button tests are removed and replaced by an assertion that the
 * panel does NOT render those controls (no duplicate controls — m1).
 */

import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { render, screen, fireEvent, cleanup, act } from '@testing-library/react';
import type { BridgeState } from '../../state/store.js';
import { INITIAL_STATE } from '../../state/store.js';
import type { BridgeActions, ThumbnailPullResult } from '../../hooks/useBridge.js';

// -------------------------------------------------------------------------
// Mock dockview-react (no browser-native APIs needed for unit tests)
// -------------------------------------------------------------------------

vi.mock('dockview-react', () => ({
  DockviewReact: () => null,
}));

// -------------------------------------------------------------------------
// Mock BridgeContext state hook
// -------------------------------------------------------------------------

let mockState: BridgeState = { ...INITIAL_STATE };

vi.mock('../../state/BridgeContext.js', () => ({
  useBridgeState: () => mockState,
}));

// -------------------------------------------------------------------------
// Mock useBridgeActions — spied callbacks so tests can assert which action
// a button fires. Action bodies are exercised by the hook's own tests.
// -------------------------------------------------------------------------

const mockActions = {
  connect:        vi.fn<BridgeActions['connect']>().mockResolvedValue(undefined),
  disconnect:     vi.fn<BridgeActions['disconnect']>().mockResolvedValue(undefined),
  reconnect:      vi.fn<BridgeActions['reconnect']>().mockResolvedValue(undefined),
  getStatus:      vi.fn<BridgeActions['getStatus']>().mockResolvedValue(undefined),
  getSceneTree:   vi.fn<BridgeActions['getSceneTree']>().mockResolvedValue(undefined),
  getObjectSnapshot: vi.fn<BridgeActions['getObjectSnapshot']>().mockResolvedValue(undefined),
  getSchemaSnapshot: vi.fn<BridgeActions['getSchemaSnapshot']>().mockResolvedValue(undefined),
  setObjectProperty: vi.fn<BridgeActions['setObjectProperty']>().mockResolvedValue({ accepted: true }),
  getViewportThumbnail: vi.fn<BridgeActions['getViewportThumbnail']>().mockResolvedValue('ok' as ThumbnailPullResult),
  play:           vi.fn<BridgeActions['play']>().mockResolvedValue(undefined),
  pause:          vi.fn<BridgeActions['pause']>().mockResolvedValue(undefined),
  stop:           vi.fn<BridgeActions['stop']>().mockResolvedValue(undefined),
  focusViewport:  vi.fn<BridgeActions['focusViewport']>().mockResolvedValue(undefined),
  launch:         vi.fn<BridgeActions['launch']>().mockResolvedValue(undefined),
  stopProcess:    vi.fn<BridgeActions['stopProcess']>().mockResolvedValue(undefined),
  dismissError:   vi.fn<BridgeActions['dismissError']>(),
  selectObject:   vi.fn<BridgeActions['selectObject']>(),
} satisfies BridgeActions;

vi.mock('../../hooks/useBridge.js', () => ({
  useBridgeActions: (): BridgeActions => mockActions,
}));

// -------------------------------------------------------------------------
// Import component AFTER mocks are set up
// -------------------------------------------------------------------------

import { GameViewPanel } from '../GameViewPanel.js';

// -------------------------------------------------------------------------
// Helpers
// -------------------------------------------------------------------------

/**
 * Build a minimal IDockviewPanelProps stub.
 * The panel does not use any dockview-specific props for rendering.
 */
function makeDockviewProps(): Parameters<typeof GameViewPanel>[0] {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  return {} as any;
}

afterEach(cleanup);
beforeEach(() => {
  for (const fn of Object.values(mockActions)) fn.mockClear();
  mockState = { ...INITIAL_STATE };
});

// -------------------------------------------------------------------------
// Error banner tests
// -------------------------------------------------------------------------

describe('GameViewPanel error banner', () => {
  it('renders the error message when lastError is set', () => {
    mockState = {
      ...INITIAL_STATE,
      lastError: { kind: 'process', message: 'engine executable not found: C:/x.exe' },
    };
    render(<GameViewPanel {...makeDockviewProps()} />);
    expect(screen.getByText(/engine executable not found: C:\/x\.exe/)).toBeTruthy();
  });

  it('renders the humanized kind label', () => {
    mockState = {
      ...INITIAL_STATE,
      lastError: { kind: 'process', message: 'engine executable not found: C:/x.exe' },
    };
    render(<GameViewPanel {...makeDockviewProps()} />);
    expect(screen.getByText('Process error')).toBeTruthy();
  });

  it('does not render error banner when lastError is undefined', () => {
    mockState = { ...INITIAL_STATE, lastError: undefined };
    render(<GameViewPanel {...makeDockviewProps()} />);
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('fires actions.dismissError when dismiss button is clicked', () => {
    mockState = {
      ...INITIAL_STATE,
      lastError: { kind: 'connect', message: 'refused' },
    };
    render(<GameViewPanel {...makeDockviewProps()} />);
    const btn = screen.getByRole('button', { name: 'Dismiss error' });
    fireEvent.click(btn);
    expect(mockActions.dismissError).toHaveBeenCalledOnce();
  });
});

// -------------------------------------------------------------------------
// M2 regression: [object Object] / missing message guard
// -------------------------------------------------------------------------

describe('GameViewPanel error banner — M2 regression', () => {
  it('shows humanized label and NOT [object Object] when message is "[object Object]"', () => {
    mockState = {
      ...INITIAL_STATE,
      lastError: { kind: 'notConnected', message: '[object Object]' },
    };
    render(<GameViewPanel {...makeDockviewProps()} />);
    expect(screen.getByText('Not connected')).toBeTruthy();
    expect(screen.queryByText(/\[object Object\]/)).toBeNull();
  });

  it('shows humanized label and NOT [object Object] when message is empty', () => {
    mockState = {
      ...INITIAL_STATE,
      lastError: { kind: 'alreadyConnected', message: '' },
    };
    render(<GameViewPanel {...makeDockviewProps()} />);
    expect(screen.getByText('Already connected')).toBeTruthy();
    expect(screen.queryByText(/\[object Object\]/)).toBeNull();
  });

  it('shows full "kind: message" when a real message is present', () => {
    mockState = {
      ...INITIAL_STATE,
      lastError: { kind: 'process', message: 'engine executable not found: C:/x.exe' },
    };
    render(<GameViewPanel {...makeDockviewProps()} />);
    expect(screen.getByText('Process error')).toBeTruthy();
    expect(screen.getByText(/engine executable not found/)).toBeTruthy();
  });
});

// -------------------------------------------------------------------------
// P4 (m1): engine/runtime/process controls were moved to the toolbar.
// GameViewPanel must NOT render them anymore (no duplicate controls).
// -------------------------------------------------------------------------

describe('GameViewPanel no duplicate engine/runtime controls (P4 m1)', () => {
  const movedButtonNames = [
    'Launch',
    'Stop',
    'Reconnect',
    'Play',
    'Pause',
    'Focus Viewport',
  ] as const;

  it.each(movedButtonNames)('does not render the "%s" control', (name) => {
    // Connected so that, were these controls present, they would be enabled
    // and findable — i.e. their absence is genuine, not just disabled.
    mockState = { ...INITIAL_STATE, connection: { status: 'connected' } };
    render(<GameViewPanel {...makeDockviewProps()} />);
    expect(screen.queryByRole('button', { name })).toBeNull();
  });

  it('still renders the Refresh Thumbnail control (panel-local, kept)', () => {
    mockState = { ...INITIAL_STATE, connection: { status: 'connected' } };
    render(<GameViewPanel {...makeDockviewProps()} />);
    expect(screen.getByRole('button', { name: 'Refresh Thumbnail' })).toBeTruthy();
  });
});

// -------------------------------------------------------------------------
// Viewport state badge
// -------------------------------------------------------------------------

describe('GameViewPanel viewport state badge', () => {
  it('renders the viewport state label when viewportState is provided', () => {
    mockState = { ...INITIAL_STATE, viewportState: 'hidden' };
    render(<GameViewPanel {...makeDockviewProps()} />);
    expect(screen.getByText('Hidden')).toBeTruthy();
  });

  it('renders "--" when viewportState is undefined', () => {
    mockState = { ...INITIAL_STATE, viewportState: undefined };
    const { container } = render(<GameViewPanel {...makeDockviewProps()} />);
    const viewportLabel = container.querySelector('.placeholder-box .label');
    expect(viewportLabel?.textContent).toBe('Viewport:');
    const placeholderBox = container.querySelector('.placeholder-box');
    expect(placeholderBox?.textContent).toContain('--');
  });

  it('renders "Focused" for focused state', () => {
    mockState = { ...INITIAL_STATE, viewportState: 'focused' };
    render(<GameViewPanel {...makeDockviewProps()} />);
    expect(screen.getByText('Focused')).toBeTruthy();
  });
});

// -------------------------------------------------------------------------
// Viewport thumbnail (Phase 7b)
// -------------------------------------------------------------------------

describe('GameViewPanel viewport thumbnail', () => {
  // A 1x1 transparent PNG (base64) — engine-agnostic generic image bytes.
  const PNG_1X1 =
    'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==';

  it('renders an <img> with a data: URL when a thumbnail is present and connected', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      viewportThumbnail: { imageBase64: PNG_1X1, mimeType: 'image/png', width: 1, height: 1 },
    };
    render(<GameViewPanel {...makeDockviewProps()} />);
    const img = screen.getByAltText('Engine viewport thumbnail') as HTMLImageElement;
    expect(img).toBeTruthy();
    expect(img.getAttribute('src')).toBe(`data:image/png;base64,${PNG_1X1}`);
    // The thumbnail title replaces the external-window notice.
    expect(screen.getByText('Engine Viewport (Thumbnail)')).toBeTruthy();
    expect(screen.queryByText('Engine Viewport (External Window)')).toBeNull();
  });

  it('falls back to the external-window notice when the engine reports unsupported', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      viewportThumbnailUnsupported: true,
    };
    render(<GameViewPanel {...makeDockviewProps()} />);
    expect(screen.getByText('Engine Viewport (External Window)')).toBeTruthy();
    expect(screen.queryByAltText('Engine viewport thumbnail')).toBeNull();
    // The refresh button is disabled when unsupported.
    const btn = screen.getByRole('button', { name: 'Refresh Thumbnail' }) as HTMLButtonElement;
    expect(btn.disabled).toBe(true);
  });

  it('falls back to the external-window notice when no thumbnail has been fetched', () => {
    mockState = { ...INITIAL_STATE, connection: { status: 'connected' } };
    render(<GameViewPanel {...makeDockviewProps()} />);
    expect(screen.getByText('Engine Viewport (External Window)')).toBeTruthy();
    expect(screen.queryByAltText('Engine viewport thumbnail')).toBeNull();
  });

  it('Refresh Thumbnail button fires getViewportThumbnail with the policy resolution cap', () => {
    mockState = { ...INITIAL_STATE, connection: { status: 'connected' } };
    render(<GameViewPanel {...makeDockviewProps()} />);
    mockActions.getViewportThumbnail.mockClear();
    const btn = screen.getByRole('button', { name: 'Refresh Thumbnail' });
    fireEvent.click(btn);
    expect(mockActions.getViewportThumbnail).toHaveBeenCalledWith(640, 360);
  });

  it('does not auto-fetch a thumbnail while disconnected', () => {
    mockState = { ...INITIAL_STATE, connection: { status: 'disconnected' } };
    render(<GameViewPanel {...makeDockviewProps()} />);
    expect(mockActions.getViewportThumbnail).not.toHaveBeenCalled();
    const btn = screen.getByRole('button', { name: 'Refresh Thumbnail' }) as HTMLButtonElement;
    expect(btn.disabled).toBe(true);
  });

  it('auto-fetches a thumbnail once on connect (mount)', () => {
    mockState = { ...INITIAL_STATE, connection: { status: 'connected' } };
    render(<GameViewPanel {...makeDockviewProps()} />);
    // The mount effect issues one immediate pull at the policy resolution.
    expect(mockActions.getViewportThumbnail).toHaveBeenCalledWith(640, 360);
  });

  it('reports a thumbnail fetch error through the error banner kind label', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'error' },
      lastError: { kind: 'VIEWPORT_GET_THUMBNAIL_FAILED', message: 'boom' },
    };
    render(<GameViewPanel {...makeDockviewProps()} />);
    expect(screen.getByText('Thumbnail fetch failed')).toBeTruthy();
  });
});

// -------------------------------------------------------------------------
// P7: thumbnail auto-refresh back-off
//
// useThumbnailAutoRefresh は GameViewPanel 内部のカスタムフック。
// ここでは GameViewPanel 経由で動作を検証する（useBridgeActions のモックで
// getViewportThumbnail の戻り値を制御し、vi.useFakeTimers で setTimeout を操る）。
//
// 検証項目:
//   1. 連続 'error' → 間隔が指数的に伸びる (1s → 2s → 4s → … → CAP=30s)
//   2. 'ok' で間隔が基準 (1000ms) にリセットされる
//   3. 手動 Refresh で失敗カウントがリセットされ即時 pull される
//   4. 'unsupported' でポーリングが停止する
//   5. 1fps 上限: 基準間隔 1000ms 未満で叩かない
// -------------------------------------------------------------------------

describe('GameViewPanel thumbnail auto-refresh back-off (P7)', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mockState = { ...INITIAL_STATE, connection: { status: 'connected' } };
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('calls pull immediately on mount (initial fetch)', async () => {
    mockActions.getViewportThumbnail.mockResolvedValue('ok');
    render(<GameViewPanel {...makeDockviewProps()} />);
    // flush the initial tick (Promise microtask)
    await act(async () => { await Promise.resolve(); });
    expect(mockActions.getViewportThumbnail).toHaveBeenCalledTimes(1);
  });

  it('applies exponential back-off on consecutive errors (1s → 2s → 4s)', async () => {
    // Every pull returns 'error'.
    mockActions.getViewportThumbnail.mockResolvedValue('error');
    render(<GameViewPanel {...makeDockviewProps()} />);

    // tick 0: immediate (mount)
    await act(async () => { await Promise.resolve(); });
    const callsAfterMount = mockActions.getViewportThumbnail.mock.calls.length;
    expect(callsAfterMount).toBe(1);

    // After 1st error, back-off = 1000ms * 2^(1-1) = 1000ms
    await act(async () => { vi.advanceTimersByTime(999); });
    expect(mockActions.getViewportThumbnail).toHaveBeenCalledTimes(1);
    await act(async () => { vi.advanceTimersByTime(1); });
    await act(async () => { await Promise.resolve(); });
    expect(mockActions.getViewportThumbnail).toHaveBeenCalledTimes(2);

    // After 2nd error, back-off = 1000ms * 2^(2-1) = 2000ms
    await act(async () => { vi.advanceTimersByTime(1999); });
    expect(mockActions.getViewportThumbnail).toHaveBeenCalledTimes(2);
    await act(async () => { vi.advanceTimersByTime(1); });
    await act(async () => { await Promise.resolve(); });
    expect(mockActions.getViewportThumbnail).toHaveBeenCalledTimes(3);

    // After 3rd error, back-off = 1000ms * 2^(3-1) = 4000ms
    await act(async () => { vi.advanceTimersByTime(3999); });
    expect(mockActions.getViewportThumbnail).toHaveBeenCalledTimes(3);
    await act(async () => { vi.advanceTimersByTime(1); });
    await act(async () => { await Promise.resolve(); });
    expect(mockActions.getViewportThumbnail).toHaveBeenCalledTimes(4);
  });

  it('caps back-off at 30 000ms even with many consecutive errors', async () => {
    mockActions.getViewportThumbnail.mockResolvedValue('error');
    render(<GameViewPanel {...makeDockviewProps()} />);
    // Drive enough errors to exceed the cap (2^5 = 32 → 32000ms > 30000ms)
    for (let i = 0; i < 6; i++) {
      await act(async () => { await Promise.resolve(); });
      // Advance past cap ceiling
      await act(async () => { vi.advanceTimersByTime(30_000); });
    }
    // After the 5th error and beyond, interval is capped at 30000ms.
    // Advancing only 29999ms after a pull must NOT trigger another pull.
    const countBefore = mockActions.getViewportThumbnail.mock.calls.length;
    await act(async () => { vi.advanceTimersByTime(29_999); });
    // Lower bound: no early fire within the cap window.
    expect(mockActions.getViewportThumbnail.mock.calls.length).toBe(countBefore);
    // Upper bound: exactly +1ms (total 30000ms) triggers the next pull.
    await act(async () => { vi.advanceTimersByTime(1); });
    await act(async () => { await Promise.resolve(); });
    expect(mockActions.getViewportThumbnail.mock.calls.length).toBeGreaterThan(countBefore);
  });

  it("resets failure counter to 0 on 'ok' and resumes base interval", async () => {
    // First two pulls return error, then ok.
    mockActions.getViewportThumbnail
      .mockResolvedValueOnce('error')
      .mockResolvedValueOnce('error')
      .mockResolvedValue('ok');

    render(<GameViewPanel {...makeDockviewProps()} />);

    // tick 0: error #1 → next at 1000ms
    await act(async () => { await Promise.resolve(); });
    expect(mockActions.getViewportThumbnail).toHaveBeenCalledTimes(1);

    // tick 1: error #2 → next at 2000ms
    await act(async () => { vi.advanceTimersByTime(1000); });
    await act(async () => { await Promise.resolve(); });
    expect(mockActions.getViewportThumbnail).toHaveBeenCalledTimes(2);

    // tick 2: ok → counter reset → next at 1000ms
    await act(async () => { vi.advanceTimersByTime(2000); });
    await act(async () => { await Promise.resolve(); });
    expect(mockActions.getViewportThumbnail).toHaveBeenCalledTimes(3);

    // Now interval must be 1000ms (base), not 4000ms (what 3rd error would give).
    await act(async () => { vi.advanceTimersByTime(999); });
    expect(mockActions.getViewportThumbnail).toHaveBeenCalledTimes(3);
    await act(async () => { vi.advanceTimersByTime(1); });
    await act(async () => { await Promise.resolve(); });
    expect(mockActions.getViewportThumbnail).toHaveBeenCalledTimes(4);
  });

  it("stops polling on 'unsupported' — no further pulls after the unsupported response", async () => {
    mockActions.getViewportThumbnail.mockResolvedValue('unsupported');
    render(<GameViewPanel {...makeDockviewProps()} />);

    // Initial pull returns unsupported.
    await act(async () => { await Promise.resolve(); });
    expect(mockActions.getViewportThumbnail).toHaveBeenCalledTimes(1);

    // Advance far into the future — no more pulls should occur.
    await act(async () => { vi.advanceTimersByTime(60_000); });
    await act(async () => { await Promise.resolve(); });
    expect(mockActions.getViewportThumbnail).toHaveBeenCalledTimes(1);
  });

  it('manual Refresh resets failure counter and triggers an immediate pull', async () => {
    // Two errors, then a manual refresh.
    mockActions.getViewportThumbnail
      .mockResolvedValueOnce('error')
      .mockResolvedValueOnce('error')
      .mockResolvedValue('ok');

    render(<GameViewPanel {...makeDockviewProps()} />);

    // tick 0: error #1 → next at 1000ms
    await act(async () => { await Promise.resolve(); });
    // tick 1: error #2 → next at 2000ms
    await act(async () => { vi.advanceTimersByTime(1000); });
    await act(async () => { await Promise.resolve(); });
    expect(mockActions.getViewportThumbnail).toHaveBeenCalledTimes(2);

    // Manual Refresh before the 2000ms back-off expires.
    // Should fire immediately (no waiting for the pending timeout).
    const btn = screen.getByRole('button', { name: 'Refresh Thumbnail' });
    fireEvent.click(btn);
    await act(async () => { await Promise.resolve(); });

    // Pull was triggered immediately by the manual refresh.
    expect(mockActions.getViewportThumbnail).toHaveBeenCalledTimes(3);

    // After the manual-refresh ok, the next auto-pull should come in 1000ms
    // (base interval), not 4000ms (what the 3rd consecutive error would give).
    await act(async () => { vi.advanceTimersByTime(999); });
    expect(mockActions.getViewportThumbnail).toHaveBeenCalledTimes(3);
    await act(async () => { vi.advanceTimersByTime(1); });
    await act(async () => { await Promise.resolve(); });
    expect(mockActions.getViewportThumbnail).toHaveBeenCalledTimes(4);
  });

  it('never pulls faster than 1000ms (1 fps floor maintained on ok)', async () => {
    mockActions.getViewportThumbnail.mockResolvedValue('ok');
    render(<GameViewPanel {...makeDockviewProps()} />);

    // tick 0: immediate pull
    await act(async () => { await Promise.resolve(); });
    expect(mockActions.getViewportThumbnail).toHaveBeenCalledTimes(1);

    // 999ms must NOT have triggered a second pull.
    await act(async () => { vi.advanceTimersByTime(999); });
    expect(mockActions.getViewportThumbnail).toHaveBeenCalledTimes(1);

    // At exactly 1000ms the second pull fires.
    await act(async () => { vi.advanceTimersByTime(1); });
    await act(async () => { await Promise.resolve(); });
    expect(mockActions.getViewportThumbnail).toHaveBeenCalledTimes(2);
  });

  it('does not poll while disconnected (loop never starts)', async () => {
    mockState = { ...INITIAL_STATE, connection: { status: 'disconnected' } };
    mockActions.getViewportThumbnail.mockResolvedValue('ok');
    render(<GameViewPanel {...makeDockviewProps()} />);
    await act(async () => { await Promise.resolve(); });
    await act(async () => { vi.advanceTimersByTime(5000); });
    await act(async () => { await Promise.resolve(); });
    expect(mockActions.getViewportThumbnail).not.toHaveBeenCalled();
  });
});
