/**
 * windowManager — service that opens the secondary editor windows (Connection,
 * Settings) as separate Tauri WebviewWindows.
 *
 * This isolates the multi-window side effects away from React components
 * (typescript.md: "副作用は hook/service へ"). The main toolbar wires its
 * Open-Connection / Open-Settings buttons to openSecondaryWindow(); components
 * never import @tauri-apps/api/webviewWindow directly, which keeps them
 * presentational and trivially mockable in tests.
 *
 * Each secondary window is a brand new webview = an independent React tree that
 * boots App() with `?window=<target>` and renders SecondaryWindowRoot. State
 * stays in sync because the Rust backend broadcasts bridge events to every
 * window; each window subscribes once via useBridgeSubscriptions(). The backend
 * is NOT changed by this feature.
 *
 * The window label IS the routing target ('connection' | 'settings'); reusing
 * it as the label lets getByLabel() deduplicate so a second click focuses the
 * existing window instead of spawning a duplicate.
 *
 * Creation IPC requires core:webview:allow-create-webview-window on the main
 * window's capability (capabilities/default.json). Secondary windows are NOT
 * granted that permission, so they cannot spawn further windows.
 */

import { WebviewWindow } from '@tauri-apps/api/webviewWindow';

/** The routing targets that can be opened in their own window. */
export type SecondaryWindowTarget = 'connection' | 'settings';

/** Per-target window metadata (label === target, used for getByLabel dedup). */
interface SecondaryWindowSpec {
  /** Window/webview label — also the `?window=` route value. */
  readonly label: SecondaryWindowTarget;
  /** OS window title (also rendered by the custom AppTitleBar inside). */
  readonly title: string;
  /** Initial logical width in pixels. */
  readonly width: number;
  /** Initial logical height in pixels. */
  readonly height: number;
}

const WINDOW_SPECS: Record<SecondaryWindowTarget, SecondaryWindowSpec> = {
  connection: { label: 'connection', title: 'Connection', width: 420, height: 460 },
  settings:   { label: 'settings',   title: 'Settings',   width: 480, height: 420 },
};

/**
 * Open (or focus) the secondary window for `target`.
 *
 * - If a window with that label already exists, focus it (never re-create).
 * - Otherwise create a decorations:false WebviewWindow pointed at
 *   `index.html?window=<target>`. The route is app-relative, so Tauri appends
 *   it to the dev-server URL in development and to the bundled app URL in
 *   production.
 *
 * Window-creation failures are logged but never thrown: a failed open is
 * non-fatal (the panel is still reachable inside the main window in P5).
 */
export async function openSecondaryWindow(target: SecondaryWindowTarget): Promise<void> {
  const spec = WINDOW_SPECS[target];

  // Dedup: focus an already-open window rather than spawning a duplicate.
  const existing = await WebviewWindow.getByLabel(spec.label);
  if (existing !== null) {
    try {
      await existing.setFocus();
    } catch (err: unknown) {
      console.error(`[windowManager] Failed to focus '${spec.label}' window:`, err);
    }
    return;
  }

  // Create a fresh window. The label doubles as the `?window=` route value.
  const webview = new WebviewWindow(spec.label, {
    url: `index.html?window=${spec.label}`,
    title: spec.title,
    width: spec.width,
    height: spec.height,
    decorations: false,
    resizable: true,
  });

  // Surface creation success/failure without crashing the caller. The handlers
  // are registered once. In Tauri 2, window-creation failures (e.g. permission
  // denied) are delivered as a 'tauri://error' event — not as a rejection of
  // once(). The try/catch here guards against listener-registration failures,
  // which are effectively impossible in practice.
  try {
    await webview.once('tauri://created', () => {
      // Window created — nothing else to do; the new webview boots App().
    });
    await webview.once('tauri://error', (event) => {
      console.error(`[windowManager] Failed to create '${spec.label}' window:`, event.payload);
    });
  } catch (err: unknown) {
    console.error(`[windowManager] Failed to open '${spec.label}' window:`, err);
  }
}
