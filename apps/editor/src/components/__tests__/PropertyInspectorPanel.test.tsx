// @vitest-environment jsdom
/**
 * PropertyInspectorPanel tests — Phase 5 (object.setProperty write path) on top
 * of the Phase 4 read paths.
 *
 * Covers:
 *   - Four degradation states: disconnected / no-selection / METHOD_NOT_SUPPORTED /
 *     empty property bag.
 *   - schema fetch on the connected edge; object fetch on selection change.
 *   - Selection -> fetch race guard.
 *   - Editing: an edit control per value kind (string/number/boolean/null/array/
 *     object); commit on blur/Enter (scalars), toggle (boolean), Apply (JSON).
 *   - JSON editor local validation (invalid JSON is shown inline, never sent).
 *   - Edit-state locality: typing does NOT dispatch setObjectProperty per
 *     keystroke (commit-only).
 *   - accepted:false and backend-error feedback rendered inline.
 *
 * IDockviewPanelProps is stubbed; the panel only uses the bridge hooks.
 */

import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { render, screen, cleanup, fireEvent } from '@testing-library/react';
import type { BridgeState } from '../../state/store.js';
import { INITIAL_STATE } from '../../state/store.js';
import type { ObjectSnapshot, SetObjectPropertyResult } from '@norves/bridge-ui';

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
const setObjectProperty = vi.fn<
  (objectId: string, property: string, value: unknown) => Promise<SetObjectPropertyResult>
>();

vi.mock('../../hooks/useBridge.js', () => ({
  useBridgeActions: () => ({ getObjectSnapshot, getSchemaSnapshot, setObjectProperty }),
}));

import { PropertyInspectorPanel } from '../PropertyInspectorPanel.js';

afterEach(cleanup);
beforeEach(() => {
  mockState = { ...INITIAL_STATE };
  getObjectSnapshot.mockClear();
  getSchemaSnapshot.mockClear();
  setObjectProperty.mockReset();
  // Default: every write is accepted with the requested value echoed.
  setObjectProperty.mockImplementation((_id, _prop, value) =>
    Promise.resolve({ accepted: true, appliedValue: value as SetObjectPropertyResult['appliedValue'] }),
  );
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

function renderSelected(snapshot: ObjectSnapshot): void {
  mockState = {
    ...INITIAL_STATE,
    connection: { status: 'connected' },
    selectedObjectId: snapshot.objectId,
    objectSnapshot: snapshot,
  };
  render(<PropertyInspectorPanel {...makeDockviewProps()} />);
}

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
// Editor controls per value kind
// -------------------------------------------------------------------------

describe('PropertyInspectorPanel — editor controls', () => {
  it('renders the object header (name + kind + id)', () => {
    renderSelected(DEMO_SNAPSHOT);
    expect(screen.getByText('NodeA')).toBeTruthy();
    expect(screen.getByText('object')).toBeTruthy();
    expect(screen.getByText('n-1')).toBeTruthy();
  });

  it('renders all property names', () => {
    renderSelected(DEMO_SNAPSHOT);
    for (const name of ['label', 'fieldOfView', 'enabled', 'parent', 'position', 'metadata']) {
      expect(screen.getByText(name)).toBeTruthy();
    }
  });

  it('renders a text input for a string, seeded with the value', () => {
    renderSelected(DEMO_SNAPSHOT);
    const input = screen.getByDisplayValue('Example Name') as HTMLInputElement;
    expect(input.tagName).toBe('INPUT');
    expect(input.type).toBe('text');
  });

  it('renders a number input for a number', () => {
    renderSelected(DEMO_SNAPSHOT);
    const input = screen.getByDisplayValue('60') as HTMLInputElement;
    expect(input.type).toBe('number');
  });

  it('renders a checkbox for a boolean, checked when true', () => {
    renderSelected(DEMO_SNAPSHOT);
    const checkbox = screen.getByRole('checkbox') as HTMLInputElement;
    expect(checkbox.checked).toBe(true);
  });

  it('renders JSON textareas for null / array / object values', () => {
    renderSelected(DEMO_SNAPSHOT);
    const textareas = screen.getAllByRole('textbox').filter(
      (el): el is HTMLTextAreaElement => el.tagName === 'TEXTAREA',
    );
    // null + array + object = 3 JSON editors.
    expect(textareas.length).toBe(3);
    // The array editor is seeded with the pretty-printed JSON.
    const arrayEditor = textareas.find((t) => t.value.includes('1.5'));
    expect(arrayEditor).toBeTruthy();
  });
});

// -------------------------------------------------------------------------
// Commit behaviour (scalars: blur / Enter)
// -------------------------------------------------------------------------

describe('PropertyInspectorPanel — scalar commit', () => {
  it('commits a string edit on blur with the new value', () => {
    renderSelected(DEMO_SNAPSHOT);
    const input = screen.getByDisplayValue('Example Name') as HTMLInputElement;
    fireEvent.change(input, { target: { value: 'Renamed' } });
    // Typing alone does NOT dispatch (edit-state locality).
    expect(setObjectProperty).not.toHaveBeenCalled();
    fireEvent.blur(input);
    expect(setObjectProperty).toHaveBeenCalledWith('n-1', 'label', 'Renamed');
  });

  it('does not commit when the string is unchanged on blur', () => {
    renderSelected(DEMO_SNAPSHOT);
    const input = screen.getByDisplayValue('Example Name') as HTMLInputElement;
    fireEvent.blur(input);
    expect(setObjectProperty).not.toHaveBeenCalled();
  });

  it('commits a number edit as a number (not a string)', () => {
    renderSelected(DEMO_SNAPSHOT);
    const input = screen.getByDisplayValue('60') as HTMLInputElement;
    fireEvent.change(input, { target: { value: '75' } });
    fireEvent.blur(input);
    expect(setObjectProperty).toHaveBeenCalledWith('n-1', 'fieldOfView', 75);
  });

  it('shows an inline error and does not commit a non-numeric number entry', () => {
    renderSelected(DEMO_SNAPSHOT);
    const input = screen.getByDisplayValue('60') as HTMLInputElement;
    // type=number inputs reject letters in the DOM, but an empty entry is the
    // realistic invalid case; assert the empty-string path does not commit.
    fireEvent.change(input, { target: { value: '' } });
    fireEvent.blur(input);
    expect(setObjectProperty).not.toHaveBeenCalled();
    expect(screen.getByText(/数値として解釈できません/)).toBeTruthy();
  });

  it('commits a boolean immediately on toggle', () => {
    renderSelected(DEMO_SNAPSHOT);
    const checkbox = screen.getByRole('checkbox') as HTMLInputElement;
    fireEvent.click(checkbox);
    expect(setObjectProperty).toHaveBeenCalledWith('n-1', 'enabled', false);
  });
});

// -------------------------------------------------------------------------
// JSON editor (array / object / null): Apply + local validation
// -------------------------------------------------------------------------

describe('PropertyInspectorPanel — JSON editor', () => {
  function arrayTextarea(): HTMLTextAreaElement {
    const textareas = screen.getAllByRole('textbox').filter(
      (el): el is HTMLTextAreaElement => el.tagName === 'TEXTAREA',
    );
    const t = textareas.find((x) => x.value.includes('1.5'));
    if (!t) throw new Error('array JSON editor not found');
    return t;
  }

  it('shows an inline error for invalid JSON and does not commit', () => {
    renderSelected(DEMO_SNAPSHOT);
    const textarea = arrayTextarea();
    fireEvent.change(textarea, { target: { value: '[1, 2,' } });
    // Click the Apply button that belongs to this editor (its parent span).
    const apply = textarea.parentElement?.querySelector('button');
    expect(apply).toBeTruthy();
    fireEvent.click(apply as HTMLButtonElement);
    expect(setObjectProperty).not.toHaveBeenCalled();
    expect(screen.getByText(/不正な JSON/)).toBeTruthy();
  });

  it('commits the array via its own Apply button with the parsed value', () => {
    renderSelected(DEMO_SNAPSHOT);
    const textarea = arrayTextarea();
    fireEvent.change(textarea, { target: { value: '[9, 8, 7]' } });
    const apply = textarea.parentElement?.querySelector('button') as HTMLButtonElement;
    fireEvent.click(apply);
    expect(setObjectProperty).toHaveBeenCalledWith('n-1', 'position', [9, 8, 7]);
  });

  it('lets a null value be edited to a scalar via the JSON editor', () => {
    renderSelected(DEMO_SNAPSHOT);
    // The null editor is seeded with the text "null".
    const textareas = screen.getAllByRole('textbox').filter(
      (el): el is HTMLTextAreaElement => el.tagName === 'TEXTAREA',
    );
    const nullEditor = textareas.find((t) => t.value === 'null');
    expect(nullEditor).toBeTruthy();
    fireEvent.change(nullEditor as HTMLTextAreaElement, { target: { value: '"now-a-string"' } });
    const apply = (nullEditor as HTMLTextAreaElement).parentElement?.querySelector(
      'button',
    ) as HTMLButtonElement;
    fireEvent.click(apply);
    expect(setObjectProperty).toHaveBeenCalledWith('n-1', 'parent', 'now-a-string');
  });
});

// -------------------------------------------------------------------------
// Write feedback (accepted:false / backend error)
// -------------------------------------------------------------------------

describe('PropertyInspectorPanel — write feedback', () => {
  it('shows a rejected notice when the engine answers accepted:false', async () => {
    setObjectProperty.mockResolvedValue({ accepted: false });
    renderSelected(DEMO_SNAPSHOT);
    const input = screen.getByDisplayValue('Example Name') as HTMLInputElement;
    fireEvent.change(input, { target: { value: 'X' } });
    fireEvent.blur(input);
    expect(await screen.findByText(/エンジンが変更を拒否しました/)).toBeTruthy();
  });

  it('shows an error notice when the write throws', async () => {
    setObjectProperty.mockRejectedValue(new Error('boom'));
    renderSelected(DEMO_SNAPSHOT);
    const input = screen.getByDisplayValue('Example Name') as HTMLInputElement;
    fireEvent.change(input, { target: { value: 'X' } });
    fireEvent.blur(input);
    expect(await screen.findByText(/送信に失敗しました: boom/)).toBeTruthy();
  });
});

// -------------------------------------------------------------------------
// Fetch behaviour (unchanged from Phase 4)
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
    expect(screen.queryByDisplayValue('Example Name')).toBeNull();
    expect(screen.getByText(/プロパティを読み込み中/)).toBeTruthy();
  });

  it('renders the snapshot when its objectId matches the current selection', () => {
    renderSelected(DEMO_SNAPSHOT);
    expect(screen.getByDisplayValue('Example Name')).toBeTruthy();
  });
});
