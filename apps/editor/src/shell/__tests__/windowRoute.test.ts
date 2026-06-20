/**
 * windowRoute tests — query-parameter route resolution.
 *
 * Pure function; no DOM needed. Verifies the main/connection/settings split and
 * the fallback to 'main' for absent / unknown values.
 */

import { describe, it, expect } from 'vitest';
import { resolveWindowRoute } from '../windowRoute.js';

describe('resolveWindowRoute', () => {
  it("returns 'main' when the window param is absent", () => {
    expect(resolveWindowRoute('')).toBe('main');
    expect(resolveWindowRoute('?foo=bar')).toBe('main');
  });

  it("returns 'main' for an explicit window=main", () => {
    expect(resolveWindowRoute('?window=main')).toBe('main');
  });

  it("returns 'connection' for window=connection", () => {
    expect(resolveWindowRoute('?window=connection')).toBe('connection');
  });

  it("returns 'settings' for window=settings", () => {
    expect(resolveWindowRoute('?window=settings')).toBe('settings');
  });

  it("falls back to 'main' for an unknown window value", () => {
    expect(resolveWindowRoute('?window=bogus')).toBe('main');
  });
});
