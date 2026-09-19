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
const createObject = vi.fn();
const deleteObject = vi.fn();
const reparentObject = vi.fn();
const duplicateObject = vi.fn();

vi.mock('../../hooks/useBridge.js', () => ({
  useBridgeActions: () => ({
    selectObject,
    getSceneTree,
    createObject,
    deleteObject,
    reparentObject,
    duplicateObject,
  }),
}));

import {
  SceneOutlinerPanel,
  filterSceneTree,
  __resetOutlinerMemory,
} from '../SceneOutlinerPanel.js';

afterEach(cleanup);
beforeEach(() => {
  mockState = { ...INITIAL_STATE };
  selectObject.mockClear();
  getSceneTree.mockClear();
  createObject.mockClear();
  deleteObject.mockClear();
  reparentObject.mockClear();
  duplicateObject.mockClear();
  // 絞り込みと折りたたみはパネルをまたいで残るので、テストごとに戻す。
  __resetOutlinerMemory();
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

// -------------------------------------------------------------------------
// Scene edit toolbar
// -------------------------------------------------------------------------

describe('SceneOutlinerPanel — scene edit toolbar', () => {
  it('creates under the selected object when available', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      sceneTree: DEMO_TREE,
      selectedObjectId: 'n-2',
    };
    render(<SceneOutlinerPanel {...makeDockviewProps()} />);
    getSceneTree.mockClear();

    fireEvent.click(screen.getByText('追加'));

    expect(createObject).toHaveBeenCalledWith('n-2', undefined);
    expect(selectObject).not.toHaveBeenCalledWith(undefined);
  });

  it('creates at root when nothing is selected', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      sceneTree: DEMO_TREE,
    };
    render(<SceneOutlinerPanel {...makeDockviewProps()} />);

    fireEvent.click(screen.getByText('追加'));

    expect(createObject).toHaveBeenCalledWith(undefined, undefined);
  });

  it('deletes the selected object', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      sceneTree: DEMO_TREE,
      selectedObjectId: 'n-1',
    };
    render(<SceneOutlinerPanel {...makeDockviewProps()} />);

    fireEvent.click(screen.getByText('削除'));

    expect(deleteObject).toHaveBeenCalledWith('n-1');
  });

  it('moves the selected object to root by omitting newParentId', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      sceneTree: DEMO_TREE,
      selectedObjectId: 'n-1',
    };
    render(<SceneOutlinerPanel {...makeDockviewProps()} />);

    fireEvent.click(screen.getByText('rootへ移動'));

    expect(reparentObject).toHaveBeenCalledWith('n-1', undefined);
  });

  it('duplicates the selected object by omitting newParentId', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      sceneTree: DEMO_TREE,
      selectedObjectId: 'n-1',
    };
    render(<SceneOutlinerPanel {...makeDockviewProps()} />);

    fireEvent.click(screen.getByText('複製'));

    expect(duplicateObject).toHaveBeenCalledWith('n-1', undefined);
  });

  it('disables edit controls when scene edit is unsupported', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      sceneTree: DEMO_TREE,
      selectedObjectId: 'n-1',
      sceneEditUnsupported: true,
    };
    render(<SceneOutlinerPanel {...makeDockviewProps()} />);

    expect((screen.getByText('追加').closest('button') as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByText('削除').closest('button') as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByText('rootへ移動').closest('button') as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByText('複製').closest('button') as HTMLButtonElement).disabled).toBe(true);
  });

  it('disables delete, root move, and duplicate when nothing is selected', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      sceneTree: DEMO_TREE,
    };
    render(<SceneOutlinerPanel {...makeDockviewProps()} />);

    expect((screen.getByText('追加').closest('button') as HTMLButtonElement).disabled).toBe(false);
    expect((screen.getByText('削除').closest('button') as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByText('rootへ移動').closest('button') as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByText('複製').closest('button') as HTMLButtonElement).disabled).toBe(true);
  });
});

// -------------------------------------------------------------------------
// Filtering
// -------------------------------------------------------------------------

describe('filterSceneTree', () => {
  it('returns the tree unchanged for an empty or blank query', () => {
    expect(filterSceneTree(DEMO_TREE, '')).toBe(DEMO_TREE);
    expect(filterSceneTree(DEMO_TREE, '   ')).toBe(DEMO_TREE);
  });

  it('keeps a matching node with its whole subtree', () => {
    // GroupNode matches; NodeB does not, but it lives inside the match.
    const result = filterSceneTree(DEMO_TREE, 'groupnode');
    expect(result?.id).toBe('n-0');
    expect(result?.children).toHaveLength(1);
    expect(result?.children?.[0]?.id).toBe('n-2');
    expect(result?.children?.[0]?.children?.[0]?.id).toBe('n-3');
  });

  it('keeps the ancestors of a deep match and drops the other branches', () => {
    const result = filterSceneTree(DEMO_TREE, 'NodeB');
    // Root is kept as the path to the match, NodeA is gone.
    expect(result?.children?.map((c) => c.id)).toEqual(['n-2']);
    expect(result?.children?.[0]?.children?.map((c) => c.id)).toEqual(['n-3']);
  });

  it('matches id and kind, not just the name', () => {
    expect(filterSceneTree(DEMO_TREE, 'n-1')?.children?.map((c) => c.id)).toEqual(['n-1']);
    // Every demo node but NodeB declares kind "object".
    expect(filterSceneTree(DEMO_TREE, 'object')?.id).toBe('n-0');
  });

  it('is case-insensitive and treats the query as a literal, not a pattern', () => {
    expect(filterSceneTree(DEMO_TREE, 'NODEA')?.children?.map((c) => c.id)).toEqual(['n-1']);
    // A regex metacharacter matches nothing rather than matching everything.
    expect(filterSceneTree(DEMO_TREE, '.*')).toBeUndefined();
  });

  it('returns undefined when nothing matches', () => {
    expect(filterSceneTree(DEMO_TREE, 'zzz')).toBeUndefined();
  });

  it('does not mutate the input tree', () => {
    const before = JSON.stringify(DEMO_TREE);
    filterSceneTree(DEMO_TREE, 'NodeB');
    expect(JSON.stringify(DEMO_TREE)).toBe(before);
  });
});

describe('SceneOutlinerPanel — filter input', () => {
  function renderConnected(selectedObjectId?: string): void {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      sceneTree: DEMO_TREE,
      selectedObjectId,
    };
    render(<SceneOutlinerPanel {...makeDockviewProps()} />);
  }

  it('narrows the tree as the user types and restores it when cleared', () => {
    renderConnected();
    expect(screen.getByText('NodeA')).toBeTruthy();

    const input = screen.getByLabelText('シーンを絞り込む');
    fireEvent.change(input, { target: { value: 'NodeB' } });
    expect(screen.queryByText('NodeA')).toBeNull();
    expect(screen.getByText('NodeB')).toBeTruthy();
    // The path to the match stays reachable.
    expect(screen.getByText('GroupNode')).toBeTruthy();

    fireEvent.change(input, { target: { value: '' } });
    expect(screen.getByText('NodeA')).toBeTruthy();
  });

  it('shows a no-match state instead of an empty tree', () => {
    renderConnected();
    fireEvent.change(screen.getByLabelText('シーンを絞り込む'), { target: { value: 'zzz' } });
    expect(screen.getByText(/一致するオブジェクトがありません/)).toBeTruthy();
    // Not the "empty scene" copy: the scene is fine, the query is not.
    // (Exact match — the no-match copy contains that string as a substring.)
    expect(screen.queryByText('オブジェクトがありません。')).toBeNull();
  });

  it('never changes the selection or re-fetches while filtering', () => {
    renderConnected('n-1');
    // Mounting fetches the tree once (the connected edge); filtering must not
    // add to that, so count from here.
    selectObject.mockClear();
    getSceneTree.mockClear();

    fireEvent.change(screen.getByLabelText('シーンを絞り込む'), { target: { value: 'zzz' } });
    expect(selectObject).not.toHaveBeenCalled();
    expect(getSceneTree).not.toHaveBeenCalled();
  });

  it('offers no filter input while disconnected or unsupported', () => {
    mockState = { ...INITIAL_STATE, connection: { status: 'disconnected' } };
    render(<SceneOutlinerPanel {...makeDockviewProps()} />);
    expect(screen.queryByLabelText('シーンを絞り込む')).toBeNull();
    cleanup();

    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      sceneUnsupported: true,
    };
    render(<SceneOutlinerPanel {...makeDockviewProps()} />);
    expect(screen.queryByLabelText('シーンを絞り込む')).toBeNull();
  });
});

// -------------------------------------------------------------------------
// 折りたたみと、パネルをまたぐ表示状態の記憶
// -------------------------------------------------------------------------

describe('SceneOutlinerPanel — collapsing and remembered view state', () => {
  function renderTree(): void {
    mockState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      sceneTree: DEMO_TREE,
    };
    render(<SceneOutlinerPanel {...makeDockviewProps()} />);
  }

  it('collapses a node without changing the selection', () => {
    renderTree();
    expect(screen.getByText('NodeB')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'GroupNode を折りたたむ' }));
    expect(screen.queryByText('NodeB')).toBeNull();
    // 親自身は残り、選択は動かない。
    expect(screen.getByText('GroupNode')).toBeTruthy();
    expect(selectObject).not.toHaveBeenCalled();
  });

  it('expands again from the same control', () => {
    renderTree();
    fireEvent.click(screen.getByRole('button', { name: 'GroupNode を折りたたむ' }));
    const expand = screen.getByRole('button', { name: 'GroupNode を展開' });
    expect(expand.getAttribute('aria-expanded')).toBe('false');
    fireEvent.click(expand);
    expect(screen.getByText('NodeB')).toBeTruthy();
    expect(
      screen.getByRole('button', { name: 'GroupNode を折りたたむ' }).getAttribute('aria-expanded'),
    ).toBe('true');
  });

  it('gives leaf nodes no toggle', () => {
    renderTree();
    expect(screen.queryByRole('button', { name: /NodeA を/ })).toBeNull();
  });

  it('ignores collapsing while a filter is active, so matches are never hidden', () => {
    renderTree();
    fireEvent.click(screen.getByRole('button', { name: 'GroupNode を折りたたむ' }));
    expect(screen.queryByText('NodeB')).toBeNull();

    fireEvent.change(screen.getByLabelText('シーンを絞り込む'), { target: { value: 'NodeB' } });
    expect(screen.getByText('NodeB')).toBeTruthy();

    // 絞り込みを消すと、畳んだままの状態に戻る。
    fireEvent.change(screen.getByLabelText('シーンを絞り込む'), { target: { value: '' } });
    expect(screen.queryByText('NodeB')).toBeNull();
  });

  it('remembers the filter and the collapsed nodes across unmount', () => {
    renderTree();
    fireEvent.change(screen.getByLabelText('シーンを絞り込む'), { target: { value: 'Node' } });
    fireEvent.click(screen.getByRole('button', { name: 'GroupNode を折りたたむ' }));
    cleanup();

    // dockview はタブを離れるとパネルをアンマウントする。開き直したら元の見え方に戻る。
    renderTree();
    expect((screen.getByLabelText('シーンを絞り込む') as HTMLInputElement).value).toBe('Node');
    fireEvent.change(screen.getByLabelText('シーンを絞り込む'), { target: { value: '' } });
    expect(screen.queryByText('NodeB')).toBeNull();
    expect(screen.getByRole('button', { name: 'GroupNode を展開' })).toBeTruthy();
  });
});
