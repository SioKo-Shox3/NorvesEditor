// @vitest-environment jsdom
/**
 * TitleBar component tests — P1 shell primitives.
 *
 * Placed under shell/__tests__/ to keep the shell subtree self-contained.
 *
 * Verifies the presentational contract:
 *   - title text is rendered.
 *   - actions slot renders when provided.
 *   - actions slot is absent when not provided.
 *   - An extra className is forwarded to the root element.
 *
 * No Bridge / Tauri / window API dependency.
 * data-tauri-drag-region is intentionally absent at P1; it is injected at P2.
 */

import { describe, it, expect, afterEach } from 'vitest';
import { render, screen, cleanup } from '@testing-library/react';
import { TitleBar } from '../TitleBar.js';

afterEach(cleanup);

// -------------------------------------------------------------------------
// Title rendering
// -------------------------------------------------------------------------

describe('TitleBar title', () => {
  it('renders the supplied title string', () => {
    render(<TitleBar title="NorvesEditor" />);
    expect(screen.getByText('NorvesEditor')).toBeTruthy();
  });

  it('renders an arbitrary title', () => {
    render(<TitleBar title="My Game Project" />);
    expect(screen.getByText('My Game Project')).toBeTruthy();
  });
});

// -------------------------------------------------------------------------
// Actions slot
// -------------------------------------------------------------------------

describe('TitleBar actions slot', () => {
  it('renders the actions slot when provided', () => {
    render(
      <TitleBar
        title="NorvesEditor"
        actions={<button type="button">X</button>}
      />,
    );
    expect(screen.getByRole('button', { name: 'X' })).toBeTruthy();
  });

  it('renders multiple action buttons when provided', () => {
    render(
      <TitleBar
        title="NorvesEditor"
        actions={
          <>
            <button type="button">Minimise</button>
            <button type="button">Maximise</button>
            <button type="button">Close</button>
          </>
        }
      />,
    );
    expect(screen.getByRole('button', { name: 'Minimise' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Maximise' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Close' })).toBeTruthy();
  });

  it('does not render the actions container when actions prop is absent', () => {
    const { container } = render(<TitleBar title="NorvesEditor" />);
    expect(container.querySelector('.titlebar__actions')).toBeNull();
  });
});

// -------------------------------------------------------------------------
// data-tauri-drag-region is NOT present at P1
// -------------------------------------------------------------------------

describe('TitleBar P1 scope guard', () => {
  it('does not have data-tauri-drag-region at P1 (added at P2)', () => {
    const { container } = render(<TitleBar title="NorvesEditor" />);
    const root = container.firstElementChild;
    expect(root?.getAttribute('data-tauri-drag-region')).toBeNull();
  });
});

// -------------------------------------------------------------------------
// Optional className forwarding
// -------------------------------------------------------------------------

describe('TitleBar className forwarding', () => {
  it('applies .titlebar base class', () => {
    const { container } = render(<TitleBar title="NorvesEditor" />);
    expect(container.firstElementChild?.classList.contains('titlebar')).toBe(true);
  });

  it('appends extra className to the root element', () => {
    const { container } = render(<TitleBar title="NorvesEditor" className="my-titlebar" />);
    const el = container.firstElementChild;
    expect(el?.classList.contains('titlebar')).toBe(true);
    expect(el?.classList.contains('my-titlebar')).toBe(true);
  });
});
