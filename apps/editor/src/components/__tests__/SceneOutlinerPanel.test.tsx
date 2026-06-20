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

// -------------------------------------------------------------------------
// Live-refresh consume (sceneRefreshRequired set by scene.treeChanged)
// -------------------------------------------------------------------------

describe('SceneOutlinerPanel — live-refresh consume', () => {
  it('issues exactly one getSceneTree when sceneRefreshRequired flips on while connected', () => {
    // Mount connected with the flag unset so the connected-edge fetch fires once;
    // clear the mock to isolate the consume path, then flip the flag on.
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      sceneTree: DEMO_TREE,
      sceneRefreshRequired: false,
    };
    const { rerender } = render(<SceneOutlinerPanel {...makeDockviewProps()} />);
    getSceneTree.mockClear();

    mockState = { ...mockState, sceneRefreshRequired: true };
    rerender(<SceneOutlinerPanel {...makeDockviewProps()} />);
    expect(getSceneTree).toHaveBeenCalledTimes(1);
  });

  it('does not fetch from the consume effect when the flag is unset', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      sceneTree: DEMO_TREE,
      sceneRefreshRequired: false,
    };
    const { rerender } = render(<SceneOutlinerPanel {...makeDockviewProps()} />);
    // Connected-edge fetch fired once on mount; clear it and confirm a re-render
    // with the flag still unset triggers no further fetch.
    getSceneTree.mockClear();
    rerender(<SceneOutlinerPanel {...makeDockviewProps()} />);
    expect(getSceneTree).not.toHaveBeenCalled();
  });

  it('does not loop: a stuck flag across re-renders issues only one consume fetch', () => {
    // Simulate the window where the live flag is still true (reducer has not yet
    // cleared it) and the component re-renders. The ref guard must prevent a
    // second consume fetch.
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      sceneTree: DEMO_TREE,
      sceneRefreshRequired: true,
    };
    const { rerender } = render(<SceneOutlinerPanel {...makeDockviewProps()} />);
    // Drop the connected-edge fetch; only the consume path is under test now.
    getSceneTree.mockClear();
    rerender(<SceneOutlinerPanel {...makeDockviewProps()} />);
    rerender(<SceneOutlinerPanel {...makeDockviewProps()} />);
    expect(getSceneTree).not.toHaveBeenCalled();
  });

  it('re-arms after the flag clears: a later set fires another consume fetch', () => {
    // First render with the flag set consumes one fetch.
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      sceneTree: DEMO_TREE,
      sceneRefreshRequired: true,
    };
    const { rerender } = render(<SceneOutlinerPanel {...makeDockviewProps()} />);
    getSceneTree.mockClear();

    // Reducer clears the flag (sceneTreeLoaded). The ref re-arms.
    mockState = { ...mockState, sceneRefreshRequired: false };
    rerender(<SceneOutlinerPanel {...makeDockviewProps()} />);
    expect(getSceneTree).not.toHaveBeenCalled();

    // A new live event sets the flag again -> exactly one more consume fetch.
    mockState = { ...mockState, sceneRefreshRequired: true };
    rerender(<SceneOutlinerPanel {...makeDockviewProps()} />);
    expect(getSceneTree).toHaveBeenCalledTimes(1);
  });

  it('does not consume while disconnected even if the flag is set', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'disconnected' },
      sceneRefreshRequired: true,
    };
    render(<SceneOutlinerPanel {...makeDockviewProps()} />);
    expect(getSceneTree).not.toHaveBeenCalled();
  });
});
