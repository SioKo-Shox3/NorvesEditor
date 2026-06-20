// @vitest-environment jsdom
/**
 * PropertyInspectorPanel placeholder tests — Phase 1.
 *
 * Verifies the three empty-state branches:
 *   1. Disconnected   → "エンジンに接続するとプロパティが表示されます"
 *   2. No selection   → "選択なし"
 *   3. Object selected (placeholder) → shows the selectedObjectId
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

import { PropertyInspectorPanel } from '../PropertyInspectorPanel.js';

afterEach(cleanup);
beforeEach(() => {
  mockState = { ...INITIAL_STATE };
});

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function makeDockviewProps(): any { return {}; }

describe('PropertyInspectorPanel — disconnected state', () => {
  it('shows disconnected empty state when not connected', () => {
    mockState = { ...INITIAL_STATE, connection: { status: 'disconnected' } };
    render(<PropertyInspectorPanel {...makeDockviewProps()} />);
    expect(screen.getByText(/エンジンに接続するとプロパティが表示されます/)).toBeTruthy();
  });
});

describe('PropertyInspectorPanel — no selection state', () => {
  it('shows no-selection state when connected but nothing selected', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      selectedObjectId: undefined,
    };
    render(<PropertyInspectorPanel {...makeDockviewProps()} />);
    expect(screen.getByText(/選択なし/)).toBeTruthy();
  });
});

describe('PropertyInspectorPanel — selected object placeholder', () => {
  it('shows selectedObjectId when an object is selected', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      selectedObjectId: 'obj-camera-01',
    };
    render(<PropertyInspectorPanel {...makeDockviewProps()} />);
    expect(screen.getByText('obj-camera-01')).toBeTruthy();
  });
});
