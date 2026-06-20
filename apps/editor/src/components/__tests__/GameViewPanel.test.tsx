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
 * Covers (same semantics as before the refactor):
 *   - Error banner renders when lastError is present.
 *   - Error banner absent when no lastError.
 *   - Dismiss button click fires actions.dismissError().
 *   - Regression (M2): notConnected kind with [object Object] message
 *     shows humanized label, does NOT render "[object Object]".
 *   - Reconnect button enabled when connectionStatus='error'.
 *   - Reconnect button disabled when connectionStatus='connecting'.
 *   - viewportState badge renders the value.
 */

import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@testing-library/react';
import type { BridgeState } from '../../state/store.js';
import { INITIAL_STATE } from '../../state/store.js';
import type { BridgeActions } from '../../hooks/useBridge.js';

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
// Reconnect button disabled state
// -------------------------------------------------------------------------

describe('GameViewPanel Reconnect button', () => {
  it('is enabled when connectionStatus is "error"', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'error' },
    };
    render(<GameViewPanel {...makeDockviewProps()} />);
    const btn = screen.getByRole('button', { name: 'Reconnect' });
    expect((btn as HTMLButtonElement).disabled).toBe(false);
  });

  it('is disabled when connectionStatus is "connecting"', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connecting' },
    };
    render(<GameViewPanel {...makeDockviewProps()} />);
    const btn = screen.getByRole('button', { name: 'Reconnect' });
    expect((btn as HTMLButtonElement).disabled).toBe(true);
  });

  it('is disabled when connectionStatus is "disconnected"', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'disconnected' },
    };
    render(<GameViewPanel {...makeDockviewProps()} />);
    const btn = screen.getByRole('button', { name: 'Reconnect' });
    expect((btn as HTMLButtonElement).disabled).toBe(true);
  });

  it('is enabled when connectionStatus is "connected"', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
    };
    render(<GameViewPanel {...makeDockviewProps()} />);
    const btn = screen.getByRole('button', { name: 'Reconnect' });
    expect((btn as HTMLButtonElement).disabled).toBe(false);
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
