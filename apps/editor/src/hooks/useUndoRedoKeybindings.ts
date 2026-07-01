/**
 * useUndoRedoKeybindings — global keyboard shortcuts for scene-edit undo/redo
 * (Phase U1).
 *
 * Registers a single `window` keydown listener (bubble phase, cleaned up on
 * unmount) that maps:
 *   - Ctrl/Cmd+Z (no Shift)                 → undo
 *   - Ctrl/Cmd+Y  OR  Ctrl/Cmd+Shift+Z      → redo
 *
 * The listener is inert (returns without preventing default) when:
 *   - the event was already handled (event.defaultPrevented), or
 *   - the focus is inside a text-editing surface (INPUT / TEXTAREA /
 *     contentEditable) — so the PropertyInspector's text/number/JSON editors
 *     keep their NATIVE undo/redo.
 *
 * preventDefault is called ONLY when we actually dispatch undo/redo, so unrelated
 * Ctrl+Z presses (or presses in text fields) are never swallowed.
 *
 * Mount this exactly ONCE, in the main window only (BridgeRoot in App.tsx). It
 * does NOT use the capture phase.
 */

import { useEffect } from 'react';
import { useBridgeActions } from './useBridge.js';

/**
 * True when the event target is a text-editing surface where the browser's own
 * undo/redo must win (an INPUT, a TEXTAREA, or any contentEditable element).
 */
function isTextEditingTarget(target: EventTarget | null): boolean {
  if (target === null || !(target instanceof HTMLElement)) {
    return false;
  }
  const tag = target.tagName;
  if (tag === 'INPUT' || tag === 'TEXTAREA') {
    return true;
  }
  return target.isContentEditable;
}

export function useUndoRedoKeybindings(): void {
  const actions = useBridgeActions();

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      // Respect a handler that already consumed this event, and never intercept
      // typing / native undo in text-editing surfaces.
      if (event.defaultPrevented || isTextEditingTarget(event.target)) {
        return;
      }
      // Ctrl (Win/Linux) or Cmd (macOS) must be held; a bare Z/Y does nothing.
      const mod = event.ctrlKey || event.metaKey;
      if (!mod) {
        return;
      }
      const key = event.key.toLowerCase();
      if (key === 'z' && !event.shiftKey) {
        // Ctrl/Cmd+Z → undo
        event.preventDefault();
        void actions.undo();
      } else if (key === 'y' || (key === 'z' && event.shiftKey)) {
        // Ctrl/Cmd+Y or Ctrl/Cmd+Shift+Z → redo
        event.preventDefault();
        void actions.redo();
      }
    };

    // Bubble phase (NOT capture) so a text field's own handler can preventDefault
    // first and opt out via the defaultPrevented check above.
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.removeEventListener('keydown', onKeyDown);
    };
  }, [actions]);
}
