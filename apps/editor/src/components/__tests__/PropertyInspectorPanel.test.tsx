// @vitest-environment jsdom
/**
 * PropertyInspectorPanel tests — Phase 4 (object.getSnapshot + schema.getSnapshot).
 *
 * Covers:
 *   - Four degradation states: disconnected / no-selection / METHOD_NOT_SUPPORTED /
 *     empty property bag.
 *   - Property rendering for every value kind (string/number/boolean/null/
 *     array/object), proving the type-driven renderer.
 *   - schema fetch on the connected edge; object fetch on selection change.
 *   - Selection -> fetch race guard: a stored snapshot for a DIFFERENT object
 *     than the current selection is not rendered.
 *
 * IDockviewPanelProps is stubbed; the panel only uses the bridge hooks.
 */

import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { render, screen, cleanup } from '@testing-library/react';
import type { BridgeState } from '../../state/store.js';
import { INITIAL_STATE } from '../../state/store.js';
import type { ObjectSnapshot } from '@norves/bridge-ui';

// -------------------------------------------------------------------------
// Mock dockview-react
// -------------------------------------------------------------------------

vi.mock('dockview-react', () => ({
  DockviewReact: () => null,
}));

// -------------------------------------------------------------------------
// Mock BridgeContext + bridge actions
// -------------------------------------------------------------------------

let mockState: BridgeState = { ...INITIAL_STATE };

vi.mock('../../state/BridgeContext.js', () => ({
  useBridgeState:    () => mockState,
  useBridgeDispatch: () => vi.fn(),
}));

const getObjectSnapshot = vi.fn();
const getSchemaSnapshot = vi.fn();

vi.mock('../../hooks/useBridge.js', () => ({
  useBridgeActions: () => ({ getObjectSnapshot, getSchemaSnapshot }),
}));

import { PropertyInspectorPanel } from '../PropertyInspectorPanel.js';

afterEach(cleanup);
beforeEach(() => {
  mockState = { ...INITIAL_STATE };
  getObjectSnapshot.mockClear();
  getSchemaSnapshot.mockClear();
});

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function makeDockviewProps(): any { return {}; }

const DEMO_SNAPSHOT: ObjectSnapshot = {
  objectId: 'n-1',
  name: 'NodeA',
  kind: 'object',
  properties: [
    { name: 'label', value: 'Example Name', valueType: 'string' },
    { name: 'fieldOfView', value: 60, valueType: 'number' },
    { name: 'enabled', value: true, valueType: 'boolean' },
    { name: 'parent', value: null },
    { name: 'position', value: [0, 1.5, -10], valueType: 'vector3' },
    { name: 'metadata', value: { locked: false, tag: 'primary' } },
  ],
};

// -------------------------------------------------------------------------
// Degradation states
// -------------------------------------------------------------------------

describe('PropertyInspectorPanel — disconnected state', () => {
  it('shows the disconnected empty state when not connected', () => {
    mockState = { ...INITIAL_STATE, connection: { status: 'disconnected' } };
    render(<PropertyInspectorPanel {...makeDockviewProps()} />);
    expect(screen.getByText(/エンジンに接続するとプロパティが表示されます/)).toBeTruthy();
  });

  it('does not fetch while disconnected', () => {
    mockState = { ...INITIAL_STATE, connection: { status: 'disconnected' } };
    render(<PropertyInspectorPanel {...makeDockviewProps()} />);
    expect(getSchemaSnapshot).not.toHaveBeenCalled();
    expect(getObjectSnapshot).not.toHaveBeenCalled();
  });
});

describe('PropertyInspectorPanel — no selection state', () => {
  it('shows "選択なし" when connected but nothing selected', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      selectedObjectId: undefined,
    };
    render(<PropertyInspectorPanel {...makeDockviewProps()} />);
    expect(screen.getByText(/選択なし/)).toBeTruthy();
  });
});

describe('PropertyInspectorPanel — METHOD_NOT_SUPPORTED', () => {
  it('shows the unsupported notice when objectUnsupported is set', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      selectedObjectId: 'n-1',
      objectUnsupported: true,
    };
    render(<PropertyInspectorPanel {...makeDockviewProps()} />);
    expect(screen.getByText(/この engine はオブジェクト照会に未対応です/)).toBeTruthy();
  });
});

describe('PropertyInspectorPanel — empty property bag', () => {
  it('shows "プロパティがありません" when the snapshot has no properties', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      selectedObjectId: 'n-9',
      objectSnapshot: { objectId: 'n-9', name: 'Empty', properties: [] },
    };
    render(<PropertyInspectorPanel {...makeDockviewProps()} />);
    expect(screen.getByText(/プロパティがありません/)).toBeTruthy();
  });
});

// -------------------------------------------------------------------------
// Property rendering (type-driven)
// -------------------------------------------------------------------------

describe('PropertyInspectorPanel — property rendering', () => {
  beforeEach(() => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      selectedObjectId: 'n-1',
      objectSnapshot: DEMO_SNAPSHOT,
    };
  });

  it('renders the object header (name + kind + id)', () => {
    render(<PropertyInspectorPanel {...makeDockviewProps()} />);
    expect(screen.getByText('NodeA')).toBeTruthy();
    expect(screen.getByText('object')).toBeTruthy();
    expect(screen.getByText('n-1')).toBeTruthy();
  });

  it('renders all property names', () => {
    render(<PropertyInspectorPanel {...makeDockviewProps()} />);
    for (const name of ['label', 'fieldOfView', 'enabled', 'parent', 'position', 'metadata']) {
      expect(screen.getByText(name)).toBeTruthy();
    }
  });

  it('renders scalar values branched by kind', () => {
    render(<PropertyInspectorPanel {...makeDockviewProps()} />);
    expect(screen.getByText('Example Name')).toBeTruthy(); // string
    expect(screen.getByText('60')).toBeTruthy();           // number
    expect(screen.getByText('true')).toBeTruthy();         // boolean
    expect(screen.getByText('null')).toBeTruthy();         // null
  });

  it('renders array / object values as collapsible JSON previews', () => {
    render(<PropertyInspectorPanel {...makeDockviewProps()} />);
    // Array summary.
    expect(screen.getByText(/Array\(3\)/)).toBeTruthy();
    // Object summary.
    expect(screen.getByText(/Object\{2\}/)).toBeTruthy();
  });
});

// -------------------------------------------------------------------------
// Fetch behaviour
// -------------------------------------------------------------------------

describe('PropertyInspectorPanel — fetch', () => {
  it('fetches the schema once on the connected edge', () => {
    mockState = { ...INITIAL_STATE, connection: { status: 'connected' } };
    render(<PropertyInspectorPanel {...makeDockviewProps()} />);
    expect(getSchemaSnapshot).toHaveBeenCalledTimes(1);
  });

  it('fetches the object snapshot for the selected id', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      selectedObjectId: 'n-1',
    };
    render(<PropertyInspectorPanel {...makeDockviewProps()} />);
    expect(getObjectSnapshot).toHaveBeenCalledWith('n-1');
  });

  it('does not fetch an object snapshot when nothing is selected', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      selectedObjectId: undefined,
    };
    render(<PropertyInspectorPanel {...makeDockviewProps()} />);
    expect(getObjectSnapshot).not.toHaveBeenCalled();
  });
});

// -------------------------------------------------------------------------
// Selection -> fetch race guard
// -------------------------------------------------------------------------

describe('PropertyInspectorPanel — race guard', () => {
  it('does not render a snapshot whose objectId differs from the current selection', () => {
    // A late response for n-1 lingers in the store while n-2 is now selected.
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      selectedObjectId: 'n-2',
      objectSnapshot: DEMO_SNAPSHOT, // objectId 'n-1'
    };
    render(<PropertyInspectorPanel {...makeDockviewProps()} />);
    // The stale n-1 property is NOT shown; the loading state for n-2 is.
    expect(screen.queryByText('Example Name')).toBeNull();
    expect(screen.getByText(/プロパティを読み込み中/)).toBeTruthy();
  });

  it('renders the snapshot when its objectId matches the current selection', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      selectedObjectId: 'n-1',
      objectSnapshot: DEMO_SNAPSHOT,
    };
    render(<PropertyInspectorPanel {...makeDockviewProps()} />);
    expect(screen.getByText('Example Name')).toBeTruthy();
  });
});
