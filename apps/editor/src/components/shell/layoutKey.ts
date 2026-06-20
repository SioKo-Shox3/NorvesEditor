/**
 * layoutKey — single source of truth for the dockview layout localStorage key.
 *
 * P4 bumps the persisted-layout key from v1 to v2 because the default layout
 * was redesigned (Game View centre/maximised, Scene Outliner top-right, a
 * bottom-right tab group for Connection/Settings/Inspector, Log in a bottom
 * EdgeGroup drawer). A bump means an existing v1 layout (the old 6-panel
 * arrangement) cannot silently override the new default.
 *
 * Both AppLayout.tsx (persist/restore/reset) and SettingsPanel.tsx (reset
 * button) import this constant so the key is defined exactly once (no
 * duplicated hard-coded string that could drift between the two).
 */

/** localStorage key for persisting the dockview layout JSON (P4: v2). */
export const LAYOUT_STORAGE_KEY = 'norveseditor-layout-v2';

/**
 * Legacy v1 key. P4 removes any stale v1 entry once on startup so old garbage
 * does not linger in localStorage. Never written to again.
 */
export const LEGACY_LAYOUT_STORAGE_KEY_V1 = 'norveseditor-layout-v1';
