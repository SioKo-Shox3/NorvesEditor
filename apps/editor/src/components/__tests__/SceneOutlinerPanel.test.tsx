// @vitest-environment jsdom
/**
 * SceneOutlinerPanel placeholder tests — Phase 1.
 *
 * Verifies the three empty-state branches:
 *   1. Disconnected  → "エンジンに接続するとシーンが表示されます"
 *   2. Connected     → "オブジェクトがありません" (no scene data yet)
 *
 * IDockviewPanelProps is stubbed out; the panel only uses useBridgeState().
 */

import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { render, screen, cleanup } from '@testing-library/react';
import type { BridgeState } from '../../state/store.js';
import { INITIAL_STATE } from '../../state/store.js';

// -------------------------------------------------------------------------
// Mock dockview-react
// -------------------------------------------------------------------------

vi.mock('dockview-react', () => ({
  DockviewReact: () => null,
}));

// -------------------------------------------------------------------------
// Mock BridgeContext hooks
// -------------------------------------------------------------------------

let mockState: BridgeState = { ...INITIAL_STATE };

vi.mock('../../state/BridgeContext.js', () => ({
  useBridgeState:    () => mockState,
  useBridgeDispatch: () => vi.fn(),
}));

import { SceneOutlinerPanel } from '../SceneOutlinerPanel.js';

afterEach(cleanup);
beforeEach(() => {
  mockState = { ...INITIAL_STATE };
});

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function makeDockviewProps(): any { return {}; }

describe('SceneOutlinerPanel — disconnected state', () => {
  it('shows the disconnected empty state when not connected', () => {
    mockState = { ...INITIAL_STATE, connection: { status: 'disconnected' } };
    render(<SceneOutlinerPanel {...makeDockviewProps()} />);
    expect(screen.getByText(/エンジンに接続するとシーンが表示されます/)).toBeTruthy();
  });
});

describe('SceneOutlinerPanel — connected empty state', () => {
  it('shows the empty scene state when connected', () => {
    mockState = { ...INITIAL_STATE, connection: { status: 'connected' } };
    render(<SceneOutlinerPanel {...makeDockviewProps()} />);
    expect(screen.getByText(/オブジェクトがありません/)).toBeTruthy();
  });
});
