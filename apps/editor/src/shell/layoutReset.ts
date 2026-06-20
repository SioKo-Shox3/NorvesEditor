/**
 * layoutReset — cross-window "reset the dockview layout" plumbing.
 *
 * The persisted layout lives ONLY in the main window (AppLayout / dockview runs
 * there; the Connection / Settings windows do not own a layout). So a layout
 * reset must always purge the MAIN window's localStorage entry and reload the
 * MAIN window — regardless of which window the user clicked the reset control in.
 *
 * Two trigger sites:
 *   - The main toolbar's "Reset Layout" button runs locally and can clear +
 *     reload directly (clearSavedLayoutAndReload).
 *   - The Settings window's reset button is in a *different* webview, so it
 *     cannot touch the main window's localStorage (each webview has its own
 *     storage origin in practice, and even when shared we do not want the
 *     Settings window to reload itself). It emits a frontend event instead
 *     (requestLayoutReset); the main window listens for it (subscribeLayoutReset)
 *     and performs the actual clear + reload. This avoids relying on shared
 *     localStorage between windows.
 *
 * This is purely a frontend window-to-window event (emit/listen over Tauri's
 * event system, covered by the already-granted core:event permission). It does
 * NOT touch the Rust bridge dispatcher, the protocol, or any backend relay.
 */

import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event';
import { LAYOUT_STORAGE_KEY } from '../components/shell/layoutKey.js';

/**
 * Frontend event name for a cross-window layout-reset request. The
 * `norves://` prefix namespaces it away from Tauri's own `tauri://` events and
 * the bridge's `bridge.*` / engine events; it is a UI-only event, not part of
 * the bridge protocol.
 */
export const LAYOUT_RESET_EVENT = 'norves://layout-reset';

/**
 * Clear the persisted layout and reload the current window so AppLayout's
 * onReady rebuilds the default layout. Call this in the MAIN window only (it is
 * the window that owns the dockview layout and its localStorage entry).
 */
export function clearSavedLayoutAndReload(): void {
  try {
    localStorage.removeItem(LAYOUT_STORAGE_KEY);
  } catch {
    // localStorage may be unavailable in some environments — ignore silently;
    // the reload still rebuilds from whatever (possibly nothing) is stored.
  }
  // Reload to trigger AppLayout's onReady with the default layout.
  window.location.reload();
}

/**
 * Request a layout reset from another window (e.g. the Settings window). Emits a
 * frontend event the main window listens for; the requesting window does NOT
 * touch its own localStorage or reload itself — the main window performs the
 * actual clear + reload via subscribeLayoutReset.
 */
export async function requestLayoutReset(): Promise<void> {
  await emit(LAYOUT_RESET_EVENT);
}

/**
 * Subscribe (in the MAIN window) to layout-reset requests emitted by other
 * windows. Returns the Tauri UnlistenFn so the caller can unsubscribe on
 * unmount. `handler` is invoked once per received request (payload is unused).
 */
export async function subscribeLayoutReset(handler: () => void): Promise<UnlistenFn> {
  return listen(LAYOUT_RESET_EVENT, () => {
    handler();
  });
}
