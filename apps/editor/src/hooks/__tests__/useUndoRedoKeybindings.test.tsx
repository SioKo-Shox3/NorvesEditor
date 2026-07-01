// @vitest-environment jsdom
/**
 * useUndoRedoKeybindings tests (Phase U1).
 *
 * useBridgeActions is vi.mock()-ed so we can assert which action a keystroke
 * dispatches without a real Tauri/BridgeProvider context. The hook is exercised
 * through a tiny host component that mounts it; keydown events are dispatched on
 * window (bubble phase).
 *
 * Covers:
 *   - Ctrl+Z  → undo
 *   - Ctrl+Y  → redo
 *   - Ctrl+Shift+Z → redo
 *   - Cmd (metaKey) variants also work (macOS)
 *   - ignored when the focused element is an INPUT / TEXTAREA / contentEditable
 *   - ignored when event.defaultPrevented is already set
 *   - preventDefault is called only when we actually dispatch
 *   - listener is removed on unmount (no leak)
 */

import { describe, it, expect, vi, afterEach, beforeEach, type Mock } from 'vitest';
import { render, cleanup } from '@testing-library/react';
import type React from 'react';

// -------------------------------------------------------------------------
// Module mock — only useBridgeActions is used by the hook.
// -------------------------------------------------------------------------

vi.mock('../useBridge.js', () => ({
  useBridgeActions: vi.fn(),
}));

const { useBridgeActions } = await import('../useBridge.js');
const { useUndoRedoKeybindings } = await import('../useUndoRedoKeybindings.js');

// -------------------------------------------------------------------------
// Helpers
// -------------------------------------------------------------------------

function makeActions(): { undo: Mock; redo: Mock } {
  return { undo: vi.fn().mockResolvedValue(undefined), redo: vi.fn().mockResolvedValue(undefined) };
}

function Host(): React.JSX.Element {
  useUndoRedoKeybindings();
  return <div>host</div>;
}

function setup(): { undo: Mock; redo: Mock; unmount: () => void } {
  const actions = makeActions();
  // The hook reads .undo / .redo off the returned object; a partial is fine here.
  (useBridgeActions as Mock).mockReturnValue(actions);
  const { unmount } = render(<Host />);
  return { ...actions, unmount };
}

/** Dispatch a keydown on `target` (default window) and return the event. */
function press(
  init: KeyboardEventInit,
  target: EventTarget = window,
): KeyboardEvent {
  const event = new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init });
  target.dispatchEvent(event);
  return event;
}

afterEach(cleanup);
beforeEach(() => { vi.clearAllMocks(); });

// -------------------------------------------------------------------------
// Dispatch mapping
// -------------------------------------------------------------------------

describe('useUndoRedoKeybindings dispatch mapping', () => {
  it('Ctrl+Z triggers undo', () => {
    const { undo, redo } = setup();
    const ev = press({ key: 'z', ctrlKey: true });
    expect(undo).toHaveBeenCalledOnce();
    expect(redo).not.toHaveBeenCalled();
    expect(ev.defaultPrevented).toBe(true);
  });

  it('Ctrl+Y triggers redo', () => {
    const { undo, redo } = setup();
    const ev = press({ key: 'y', ctrlKey: true });
    expect(redo).toHaveBeenCalledOnce();
    expect(undo).not.toHaveBeenCalled();
    expect(ev.defaultPrevented).toBe(true);
  });

  it('Ctrl+Shift+Z triggers redo (not undo)', () => {
    const { undo, redo } = setup();
    const ev = press({ key: 'z', ctrlKey: true, shiftKey: true });
    expect(redo).toHaveBeenCalledOnce();
    expect(undo).not.toHaveBeenCalled();
    expect(ev.defaultPrevented).toBe(true);
  });

  it('Cmd+Z (metaKey) triggers undo on macOS', () => {
    const { undo } = setup();
    press({ key: 'z', metaKey: true });
    expect(undo).toHaveBeenCalledOnce();
  });

  it('Cmd+Shift+Z (metaKey) triggers redo on macOS', () => {
    const { redo } = setup();
    press({ key: 'z', metaKey: true, shiftKey: true });
    expect(redo).toHaveBeenCalledOnce();
  });

  it('a bare Z (no modifier) does nothing', () => {
    const { undo, redo } = setup();
    const ev = press({ key: 'z' });
    expect(undo).not.toHaveBeenCalled();
    expect(redo).not.toHaveBeenCalled();
    expect(ev.defaultPrevented).toBe(false);
  });
});

// -------------------------------------------------------------------------
// Ignore rules
// -------------------------------------------------------------------------

describe('useUndoRedoKeybindings ignores text-editing surfaces', () => {
  it('ignores Ctrl+Z when an INPUT is focused (native undo wins)', () => {
    const { undo } = setup();
    const input = document.createElement('input');
    document.body.appendChild(input);
    input.focus();
    const ev = press({ key: 'z', ctrlKey: true }, input);
    expect(undo).not.toHaveBeenCalled();
    expect(ev.defaultPrevented).toBe(false);
    input.remove();
  });

  it('ignores Ctrl+Z when a TEXTAREA is the target', () => {
    const { undo } = setup();
    const textarea = document.createElement('textarea');
    document.body.appendChild(textarea);
    const ev = press({ key: 'z', ctrlKey: true }, textarea);
    expect(undo).not.toHaveBeenCalled();
    expect(ev.defaultPrevented).toBe(false);
    textarea.remove();
  });

  it('ignores Ctrl+Z when the target is contentEditable', () => {
    const { undo } = setup();
    const div = document.createElement('div');
    div.contentEditable = 'true';
    // jsdom does not derive isContentEditable from the attribute (it depends on
    // layout it doesn't implement), so force the getter to reflect a real
    // browser's behavior for the element under test.
    Object.defineProperty(div, 'isContentEditable', { value: true, configurable: true });
    document.body.appendChild(div);
    const ev = press({ key: 'z', ctrlKey: true }, div);
    expect(undo).not.toHaveBeenCalled();
    expect(ev.defaultPrevented).toBe(false);
    div.remove();
  });

  it('ignores an event whose defaultPrevented is already set', () => {
    const { undo } = setup();
    const event = new KeyboardEvent('keydown', {
      key: 'z',
      ctrlKey: true,
      bubbles: true,
      cancelable: true,
    });
    event.preventDefault(); // simulate an earlier handler consuming it
    window.dispatchEvent(event);
    expect(undo).not.toHaveBeenCalled();
  });
});

// -------------------------------------------------------------------------
// Cleanup
// -------------------------------------------------------------------------

describe('useUndoRedoKeybindings cleanup', () => {
  it('removes the listener on unmount (no dispatch after unmount)', () => {
    const { undo, unmount } = setup();
    unmount();
    press({ key: 'z', ctrlKey: true });
    expect(undo).not.toHaveBeenCalled();
  });
});
