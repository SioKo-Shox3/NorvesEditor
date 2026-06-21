/**
 * layoutKey — single source of truth for the dockview layout localStorage key.
 *
 * P6 bumps the persisted-layout key from v2 to v3 because the default layout
 * structure changed again: Connection and Settings are removed from the main
 * window's dockview (they now open in their own Tauri windows), so the right
 * column is just Scene Outliner on top and the Property Inspector (when an
 * object is selected) below it, while Game View stays centre and Log lives in a
 * bottom EdgeGroup drawer. A bump means a saved v2 layout (which still encodes
 * Connection/Settings panels) cannot silently override the new default — the
 * same justification as the earlier v1 → v2 bump in P4.
 *
 * fix/default-layout bumps the key from v3 to v4 because the default panel
 * ratio changed: Game View now takes ~75 % of the width and the right column
 * (Scene Outliner) takes ~25 %, versus the previous 50:50 split. A saved v3
 * layout encodes the old 50:50 split ratio in the gridview snapshot and would
 * silently override the new default — the same justification as all prior bumps.
 *
 * Both AppLayout.tsx (persist/restore/reset) and layoutReset.ts (reset helper)
 * import LAYOUT_STORAGE_KEY so the active key is defined exactly once (no
 * duplicated hard-coded string that could drift between call sites).
 */

/** localStorage key for persisting the dockview layout JSON (v4). */
export const LAYOUT_STORAGE_KEY = 'norveseditor-layout-v4';

/**
 * Legacy keys removed once on startup so old garbage does not linger in
 * localStorage.
 *   v1 — legacy-ised in P4 (layout structure changed).
 *   v2 — legacy-ised in P6 (Connection/Settings removed from main-window
 *         dockview, making saved v2 layouts incompatible with the new default).
 *   v3 — legacy-ised in fix/default-layout (panel ratio changed to ~75:25,
 *         Game View dominant; saved v3 encodes the old 50:50 split ratio).
 * Current key is v4. These keys are never written to again.
 */
export const LEGACY_LAYOUT_STORAGE_KEYS = [
  'norveseditor-layout-v1',
  'norveseditor-layout-v2',
  'norveseditor-layout-v3',
] as const;
