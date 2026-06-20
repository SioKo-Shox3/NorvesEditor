// @vitest-environment jsdom
/**
 * SceneOutlinerPanel tests — Phase 3 (scene.getTree wired).
 *
 * Covers:
 *   - Three degradation states: disconnected / empty scene / METHOD_NOT_SUPPORTED.
 *   - Tree rendering (recursive, root + nested children).
 *   - Click-to-select, re-click to deselect, body-click deselect.
 *   - Selection highlight via selectedObjectId.
 *   - getSceneTree fetched on the connected edge + manual 更新 button.
 *
 * IDockviewPanelProps is stubbed; the panel only uses the bridge hooks.
 */

import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { render, screen, cleanup, fireEvent } from '@testing-library/react';
import type { BridgeState } from '../../state/store.js';
import { INITIAL_STATE } from '../../state/store.js';
import type { SceneNode } from '@norves/bridge-ui';

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

const selectObject = vi.fn();
const getSceneTree = vi.fn();

vi.mock('../../hooks/useBridge.js', () => ({
  useBridgeActions: () => ({ selectObject, getSceneTree }),
}));

import { SceneOutlinerPanel } from '../SceneOutlinerPanel.js';

afterEach(cleanup);
beforeEach(() => {
  mockState = { ...INITIAL_STATE };
  selectObject.mockClear();
  getSceneTree.mockClear();
});

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function makeDockviewProps(): any { return {}; }

const DEMO_TREE: SceneNode = {
  id: 'n-0',
  name: 'Root',
  kind: 'object',
  children: [
    { id: 'n-1', name: 'NodeA', kind: 'object' },
    {
      id: 'n-2',
      name: 'GroupNode',
      kind: 'object',
      children: [{ id: 'n-3', name: 'NodeB' }],
    },
  ],
};

// -------------------------------------------------------------------------
// Degradation states
// -------------------------------------------------------------------------

describe('SceneOutlinerPanel — disconnected state', () => {
  it('shows the disconnected empty state when not connected', () => {
    mockState = { ...INITIAL_STATE, connection: { status: 'disconnected' } };
    render(<SceneOutlinerPanel {...makeDockviewProps()} />);
    expect(screen.getByText(/エンジンに接続するとシーンが表示されます/)).toBeTruthy();
  });

  it('does not fetch the tree while disconnected', () => {
    mockState = { ...INITIAL_STATE, connection: { status: 'disconnected' } };
    render(<SceneOutlinerPanel {...makeDockviewProps()} />);
    expect(getSceneTree).not.toHaveBeenCalled();
  });
});

describe('SceneOutlinerPanel — empty scene', () => {
  it('shows "オブジェクトがありません" when root has no children', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      sceneTree: { id: 'n-0', name: 'Root' },
    };
    render(<SceneOutlinerPanel {...makeDockviewProps()} />);
    expect(screen.getByText(/オブジェクトがありません/)).toBeTruthy();
  });
});

describe('SceneOutlinerPanel — METHOD_NOT_SUPPORTED', () => {
  it('shows the unsupported notice when sceneUnsupported is set', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      sceneUnsupported: true,
    };
    render(<SceneOutlinerPanel {...makeDockviewProps()} />);
    expect(screen.getByText(/この engine はシーン照会に未対応です/)).toBeTruthy();
  });
});

// -------------------------------------------------------------------------
// Tree rendering + selection
// -------------------------------------------------------------------------

describe('SceneOutlinerPanel — tree rendering', () => {
  it('renders root and nested children recursively', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      sceneTree: DEMO_TREE,
    };
    render(<SceneOutlinerPanel {...makeDockviewProps()} />);
    expect(screen.getByText('Root')).toBeTruthy();
    expect(screen.getByText('NodeA')).toBeTruthy();
    expect(screen.getByText('GroupNode')).toBeTruthy();
    // NodeB is two levels deep — proves recursion.
    expect(screen.getByText('NodeB')).toBeTruthy();
  });

  it('falls back to the node id when name is absent', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      sceneTree: { id: 'n-0', children: [{ id: 'no-name-node' }] },
    };
    render(<SceneOutlinerPanel {...makeDockviewProps()} />);
    expect(screen.getByText('no-name-node')).toBeTruthy();
  });
});

describe('SceneOutlinerPanel — selection', () => {
  it('selects a node id on click', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      sceneTree: DEMO_TREE,
    };
    render(<SceneOutlinerPanel {...makeDockviewProps()} />);
    fireEvent.click(screen.getByText('NodeA'));
    expect(selectObject).toHaveBeenCalledWith('n-1');
  });

  it('deselects when re-clicking the already-selected node', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      sceneTree: DEMO_TREE,
      selectedObjectId: 'n-1',
    };
    render(<SceneOutlinerPanel {...makeDockviewProps()} />);
    fireEvent.click(screen.getByText('NodeA'));
    expect(selectObject).toHaveBeenCalledWith(undefined);
  });

  it('highlights the selected node', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      sceneTree: DEMO_TREE,
      selectedObjectId: 'n-2',
    };
    render(<SceneOutlinerPanel {...makeDockviewProps()} />);
    const selectedRow = screen.getByText('GroupNode').closest('button');
    expect(selectedRow?.className).toContain('scene-node__row--selected');
    const otherRow = screen.getByText('NodeA').closest('button');
    expect(otherRow?.className).not.toContain('scene-node__row--selected');
  });
});

// -------------------------------------------------------------------------
// Fetch behaviour
// -------------------------------------------------------------------------

describe('SceneOutlinerPanel — fetch', () => {
  it('fetches the tree once on the connected edge', () => {
    mockState = { ...INITIAL_STATE, connection: { status: 'connected' } };
    render(<SceneOutlinerPanel {...makeDockviewProps()} />);
    expect(getSceneTree).toHaveBeenCalledTimes(1);
  });

  it('re-fetches when the 更新 button is clicked', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      sceneTree: DEMO_TREE,
    };
    render(<SceneOutlinerPanel {...makeDockviewProps()} />);
    getSceneTree.mockClear();
    fireEvent.click(screen.getByText('更新'));
    expect(getSceneTree).toHaveBeenCalledTimes(1);
  });
});
