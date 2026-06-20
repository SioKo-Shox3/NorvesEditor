// @vitest-environment jsdom
/**
 * TitleBar component tests — P2 custom window controls.
 *
 * Placed under shell/__tests__/ to keep the shell subtree self-contained.
 *
 * Verifies the P2 contract:
 *   - title text is rendered.
 *   - each control button has its Japanese aria-label and fires the matching
 *     callback on click.
 *   - the maximise button label flips with the isMaximized prop.
 *   - the drag region (data-tauri-drag-region) is on the root bar and NOT on
 *     the control buttons (R8: a child drag region would swallow the click).
 *   - a double-click (mousedown detail:2) on the bar toggles maximise (the
 *     manual workaround for tauri#11945).
 *   - the optional actions slot still renders when provided.
 *
 * The component is presentational: window operations arrive as callback props,
 * so no Tauri runtime / mock is required here.
 */

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, cleanup, fireEvent } from '@testing-library/react';
import { TitleBar } from '../TitleBar.js';

afterEach(cleanup);

/** Build a props object with spy callbacks; overrides merge on top. */
function makeProps(overrides: Partial<Parameters<typeof TitleBar>[0]> = {}) {
  return {
    title: 'NorvesEditor',
    onMinimize: vi.fn(),
    onToggleMaximize: vi.fn(),
    onClose: vi.fn(),
    ...overrides,
  };
}

// -------------------------------------------------------------------------
// Title rendering
// -------------------------------------------------------------------------

describe('TitleBar title', () => {
  it('renders the supplied title string', () => {
    render(<TitleBar {...makeProps({ title: 'My Game Project' })} />);
    expect(screen.getByText('My Game Project')).toBeTruthy();
  });
});

// -------------------------------------------------------------------------
// Window control buttons — aria-labels + click callbacks
// -------------------------------------------------------------------------

describe('TitleBar window controls', () => {
  it('exposes minimise / maximise / close buttons with aria-labels', () => {
    render(<TitleBar {...makeProps()} />);
    expect(screen.getByRole('button', { name: '最小化' })).toBeTruthy();
    expect(screen.getByRole('button', { name: '最大化' })).toBeTruthy();
    expect(screen.getByRole('button', { name: '閉じる' })).toBeTruthy();
  });

  it('fires onMinimize when the minimise button is clicked', () => {
    const props = makeProps();
    render(<TitleBar {...props} />);
    fireEvent.click(screen.getByRole('button', { name: '最小化' }));
    expect(props.onMinimize).toHaveBeenCalledTimes(1);
    expect(props.onToggleMaximize).not.toHaveBeenCalled();
    expect(props.onClose).not.toHaveBeenCalled();
  });

  it('fires onToggleMaximize when the maximise button is clicked', () => {
    const props = makeProps();
    render(<TitleBar {...props} />);
    fireEvent.click(screen.getByRole('button', { name: '最大化' }));
    expect(props.onToggleMaximize).toHaveBeenCalledTimes(1);
    expect(props.onMinimize).not.toHaveBeenCalled();
    expect(props.onClose).not.toHaveBeenCalled();
  });

  it('fires onClose when the close button is clicked', () => {
    const props = makeProps();
    render(<TitleBar {...props} />);
    fireEvent.click(screen.getByRole('button', { name: '閉じる' }));
    expect(props.onClose).toHaveBeenCalledTimes(1);
    expect(props.onMinimize).not.toHaveBeenCalled();
    expect(props.onToggleMaximize).not.toHaveBeenCalled();
  });

  it('uses the "元に戻す" label for the maximise button when isMaximized', () => {
    render(<TitleBar {...makeProps({ isMaximized: true })} />);
    expect(screen.getByRole('button', { name: '元に戻す' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: '最大化' })).toBeNull();
  });
});

// -------------------------------------------------------------------------
// Drag region placement (R8)
// -------------------------------------------------------------------------

describe('TitleBar drag region', () => {
  it('puts data-tauri-drag-region on the root bar', () => {
    const { container } = render(<TitleBar {...makeProps()} />);
    const root = container.firstElementChild;
    expect(root?.hasAttribute('data-tauri-drag-region')).toBe(true);
  });

  it('puts data-tauri-drag-region on the title span', () => {
    render(<TitleBar {...makeProps({ title: 'NorvesEditor' })} />);
    const span = screen.getByText('NorvesEditor');
    expect(span.hasAttribute('data-tauri-drag-region')).toBe(true);
  });

  it('does not put data-tauri-drag-region on any control button', () => {
    render(<TitleBar {...makeProps()} />);
    for (const name of ['最小化', '最大化', '閉じる']) {
      const button = screen.getByRole('button', { name });
      expect(button.hasAttribute('data-tauri-drag-region')).toBe(false);
    }
  });
});

// -------------------------------------------------------------------------
// Double-click to maximise (manual workaround for tauri#11945)
// -------------------------------------------------------------------------

describe('TitleBar double-click maximise', () => {
  it('toggles maximise on a double-click (mousedown detail:2) of the bar', () => {
    const props = makeProps();
    const { container } = render(<TitleBar {...props} />);
    const root = container.firstElementChild as HTMLElement;
    fireEvent.mouseDown(root, { detail: 2 });
    expect(props.onToggleMaximize).toHaveBeenCalledTimes(1);
  });

  it('does not toggle maximise on a single mousedown (detail:1)', () => {
    const props = makeProps();
    const { container } = render(<TitleBar {...props} />);
    const root = container.firstElementChild as HTMLElement;
    fireEvent.mouseDown(root, { detail: 1 });
    expect(props.onToggleMaximize).not.toHaveBeenCalled();
  });
});

// -------------------------------------------------------------------------
// Optional actions slot
// -------------------------------------------------------------------------

describe('TitleBar actions slot', () => {
  it('renders the actions slot when provided', () => {
    render(<TitleBar {...makeProps({ actions: <span>extra</span> })} />);
    expect(screen.getByText('extra')).toBeTruthy();
  });

  it('does not render the actions container when actions prop is absent', () => {
    const { container } = render(<TitleBar {...makeProps()} />);
    expect(container.querySelector('.titlebar__actions')).toBeNull();
  });
});
