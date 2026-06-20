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
 * Both AppLayout.tsx (persist/restore/reset) and layoutReset.ts (reset helper)
 * import LAYOUT_STORAGE_KEY so the active key is defined exactly once (no
 * duplicated hard-coded string that could drift between call sites).
 */

/** localStorage key for persisting the dockview layout JSON (P6: v3). */
export const LAYOUT_STORAGE_KEY = 'norveseditor-layout-v3';

/**
 * Legacy keys removed once on startup so old garbage does not linger in
 * localStorage. v1 was legacy-ised in P4 (layout structure changed); v2 was
 * legacy-ised in P6 (Connection/Settings removed from main-window dockview,
 * making saved v2 layouts incompatible with the new default). Current key is
 * v3. Never written to again.
 */
export const LEGACY_LAYOUT_STORAGE_KEYS = [
  'norveseditor-layout-v1',
  'norveseditor-layout-v2',
] as const;
