/**
 * windowRoute — resolves which window route the current webview should render.
 *
 * Routing is query-parameter based (no react-router; zero new runtime deps).
 * The `?window=` parameter selects the route:
 *   - absent / 'main' → the main editor shell.
 *   - 'connection'    → the Connection secondary window.
 *   - 'settings'      → the Settings secondary window.
 *
 * Any unknown value falls back to 'main' so a malformed URL never yields a blank
 * window. The label used by windowManager.openSecondaryWindow IS the route
 * value, so the secondary routes line up 1:1 with SecondaryWindowTarget.
 */

import type { SecondaryWindowTarget } from './windowManager.js';

/** The resolved route for a webview: the main shell or a secondary panel. */
export type WindowRoute = 'main' | SecondaryWindowTarget;

/** The query-string parameter that selects the window route. */
export const WINDOW_ROUTE_PARAM = 'window';

/**
 * Resolve the {@link WindowRoute} from a location search string
 * (e.g. `?window=connection`). Pass `window.location.search` in the app;
 * tests pass a literal so they need not stub the global location.
 */
export function resolveWindowRoute(search: string): WindowRoute {
  const value = new URLSearchParams(search).get(WINDOW_ROUTE_PARAM);
  if (value === 'connection' || value === 'settings') {
    return value;
  }
  return 'main';
}
