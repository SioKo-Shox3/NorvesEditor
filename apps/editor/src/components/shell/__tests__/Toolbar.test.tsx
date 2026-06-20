// @vitest-environment jsdom
/**
 * Toolbar component tests — P1 shell primitives.
 *
 * Placed under shell/__tests__/ to keep the shell subtree self-contained.
 *
 * Verifies the presentational contract:
 *   - role="toolbar" is present for accessibility.
 *   - children are rendered inside the toolbar.
 *   - Renders without children (empty toolbar).
 *   - An extra className is forwarded to the root element.
 *
 * No Bridge / Tauri / BridgeContext dependency.
 */

import { describe, it, expect, afterEach } from 'vitest';
import { render, screen, cleanup } from '@testing-library/react';
import { Toolbar } from '../Toolbar.js';

afterEach(cleanup);

// -------------------------------------------------------------------------
// Accessibility role
// -------------------------------------------------------------------------

describe('Toolbar accessibility', () => {
  it('has role="toolbar" on the root element', () => {
    render(<Toolbar />);
    expect(screen.getByRole('toolbar')).toBeTruthy();
  });
});

// -------------------------------------------------------------------------
// Children rendering
// -------------------------------------------------------------------------

describe('Toolbar children', () => {
  it('renders children inside the toolbar', () => {
    render(
      <Toolbar>
        <button type="button">Play</button>
        <button type="button">Pause</button>
      </Toolbar>,
    );
    expect(screen.getByRole('button', { name: 'Play' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Pause' })).toBeTruthy();
  });

  it('renders without children (empty toolbar)', () => {
    expect(() => render(<Toolbar />)).not.toThrow();
    expect(screen.getByRole('toolbar').childElementCount).toBe(0);
  });

  it('renders text children', () => {
    render(<Toolbar>toolbar label</Toolbar>);
    expect(screen.getByRole('toolbar').textContent).toBe('toolbar label');
  });
});

// -------------------------------------------------------------------------
// Optional className forwarding
// -------------------------------------------------------------------------

describe('Toolbar className forwarding', () => {
  it('applies .toolbar base class', () => {
    render(<Toolbar />);
    expect(screen.getByRole('toolbar').classList.contains('toolbar')).toBe(true);
  });

  it('appends extra className to the root element', () => {
    render(<Toolbar className="my-toolbar" />);
    const el = screen.getByRole('toolbar');
    expect(el.classList.contains('toolbar')).toBe(true);
    expect(el.classList.contains('my-toolbar')).toBe(true);
  });
});
