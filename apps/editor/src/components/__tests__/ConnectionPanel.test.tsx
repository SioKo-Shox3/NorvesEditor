// @vitest-environment jsdom
/**
 * ConnectionPanel tests — connection-setup controls.
 *
 * useBridgeActions and useBridgeState are vi.mock()-ed so tests run in jsdom
 * without a Tauri/BridgeProvider context. The focus here (P6) is the Reconnect
 * button's disabled condition, which must match the toolbar / GameView ("正"):
 * enabled only when status is 'connected' or 'error'; disabled while
 * 'connecting' or 'disconnected'.
 *
 * Role separation is also asserted: ConnectionPanel offers only connection
 * setup (Connect / Disconnect / Reconnect) — no process/runtime commands.
 */

import { describe, it, expect, vi, afterEach, type Mock } from 'vitest';
import { render, screen, cleanup } from '@testing-library/react';
import { ConnectionPanel } from '../ConnectionPanel.js';
import type { IDockviewPanelProps } from 'dockview-react';
import type { BridgeActions } from '../../hooks/useBridge.js';
import type { BridgeState, ConnectionStatus } from '../../state/store.js';

// -------------------------------------------------------------------------
// Module mocks
// -------------------------------------------------------------------------

vi.mock('../../hooks/useBridge.js', () => ({
  useBridgeActions: vi.fn(),
}));

vi.mock('../../state/BridgeContext.js', () => ({
  useBridgeState: vi.fn(),
}));

const { useBridgeActions } = await import('../../hooks/useBridge.js');
const { useBridgeState }   = await import('../../state/BridgeContext.js');

// -------------------------------------------------------------------------
// Helpers
// -------------------------------------------------------------------------

function makeActions(): BridgeActions {
  return {
    openWorkspace:      vi.fn().mockResolvedValue(undefined),
    getWorkspace:       vi.fn().mockResolvedValue(undefined),
    closeWorkspace:     vi.fn().mockResolvedValue(undefined),
    readAssetManifest:  vi.fn().mockResolvedValue(undefined),
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
    getObjectSnapshot: vi.fn().mockResolvedValue(undefined),
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
  };
}

function makeState(status: ConnectionStatus): BridgeState {
  return { connection: { status } } as BridgeState;
}

function setup(status: ConnectionStatus): void {
  (useBridgeActions as Mock).mockReturnValue(makeActions());
  (useBridgeState   as Mock).mockReturnValue(makeState(status));
  render(<ConnectionPanel {...({} as IDockviewPanelProps)} />);
}

function reconnectBtn(): HTMLButtonElement {
  return screen.getByRole('button', { name: 'Reconnect' }) as HTMLButtonElement;
}

afterEach(cleanup);

// -------------------------------------------------------------------------
// Reconnect disabled — unified with the toolbar / GameView ("正")
// -------------------------------------------------------------------------

describe('ConnectionPanel Reconnect disabled', () => {
  it('is disabled when connecting', () => {
    setup('connecting');
    expect(reconnectBtn().disabled).toBe(true);
  });

  it('is disabled when disconnected', () => {
    setup('disconnected');
    expect(reconnectBtn().disabled).toBe(true);
  });

  it('is enabled when connected', () => {
    setup('connected');
    expect(reconnectBtn().disabled).toBe(false);
  });

  it('is enabled when error', () => {
    setup('error');
    expect(reconnectBtn().disabled).toBe(false);
  });
});

// -------------------------------------------------------------------------
// Role separation — no process/runtime commands on the connection surface
// -------------------------------------------------------------------------

describe('ConnectionPanel role separation', () => {
  it('exposes only connection-setup buttons (no Launch/Play/etc.)', () => {
    setup('connected');
    const buttonNames = screen
      .getAllByRole('button')
      .map((b) => b.textContent?.trim());
    expect(buttonNames).toEqual(['Connect', 'Disconnect', 'Reconnect']);
  });
});
