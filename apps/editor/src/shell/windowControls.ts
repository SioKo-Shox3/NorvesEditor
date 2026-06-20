/**
 * windowControls — thin service wrapping the current Tauri window's controls.
 *
 * This isolates the Tauri window side effects away from React components
 * (typescript.md: "副作用は hook/service へ"). Components receive these
 * functions as callbacks and never import @tauri-apps/api/window directly,
 * which keeps them presentational and trivially mockable in tests.
 *
 * Each call resolves getCurrentWindow() fresh and holds no state of its own,
 * so the service stays a stateless adapter over the platform window API.
 *
 * Only the window operations exercised by the custom title bar are exposed:
 *   - minimizeWindow        -> Window.minimize()        (core:window:allow-minimize)
 *   - toggleMaximizeWindow  -> Window.toggleMaximize()  (core:window:allow-toggle-maximize)
 *   - closeWindow           -> Window.close()           (core:window:allow-close)
 *   - isWindowMaximized     -> Window.isMaximized()     (core:window:allow-is-maximized)
 *
 * Window dragging is driven declaratively by `data-tauri-drag-region` in the
 * markup (core:window:allow-start-dragging), so it needs no wrapper here.
 */

import { getCurrentWindow } from '@tauri-apps/api/window';

/** Minimise the current window. */
export async function minimizeWindow(): Promise<void> {
  await getCurrentWindow().minimize();
}

/** Toggle the current window between maximised and restored. */
export async function toggleMaximizeWindow(): Promise<void> {
  await getCurrentWindow().toggleMaximize();
}

/** Close the current window. */
export async function closeWindow(): Promise<void> {
  await getCurrentWindow().close();
}

/** Resolve whether the current window is currently maximised. */
export async function isWindowMaximized(): Promise<boolean> {
  return getCurrentWindow().isMaximized();
}
