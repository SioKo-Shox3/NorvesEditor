// @vitest-environment jsdom
/**
 * GameViewPanel component tests — Workstream K finalization.
 *
 * GameViewPanel is purely prop-driven (no store/hook imports inside it)
 * so no Provider wrapper is needed. We render it directly.
 *
 * Covers:
 *   - Error banner renders when lastError is provided.
 *   - Error banner absent when lastError is undefined.
 *   - Dismiss button click fires onDismissError.
 *   - Regression (M2): notConnected kind with [object Object] message
 *     shows humanized label, does NOT render "[object Object]".
 *   - Reconnect button enabled when connectionStatus='error'.
 *   - Reconnect button disabled when connectionStatus='connecting'.
 *   - viewportState badge renders the value.
 */

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@testing-library/react';
import { GameViewPanel } from '../GameViewPanel.js';

afterEach(cleanup);

// -------------------------------------------------------------------------
// Error banner tests
// -------------------------------------------------------------------------

describe('GameViewPanel error banner', () => {
  it('renders the error message when lastError is set', () => {
    render(
      <GameViewPanel
        connected={false}
        lastError={{ kind: 'process', message: 'engine executable not found: C:/x.exe' }}
      />,
    );
    expect(screen.getByText(/engine executable not found: C:\/x\.exe/)).toBeTruthy();
  });

  it('renders the humanized kind label', () => {
    render(
      <GameViewPanel
        connected={false}
        lastError={{ kind: 'process', message: 'engine executable not found: C:/x.exe' }}
      />,
    );
    expect(screen.getByText('Process error')).toBeTruthy();
  });

  it('does not render error banner when lastError is undefined', () => {
    render(
      <GameViewPanel
        connected={false}
        lastError={undefined}
      />,
    );
    // Should have no element with role="alert"
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('calls onDismissError when dismiss button is clicked', () => {
    const spy = vi.fn();
    render(
      <GameViewPanel
        connected={false}
        lastError={{ kind: 'connect', message: 'refused' }}
        onDismissError={spy}
      />,
    );
    const btn = screen.getByRole('button', { name: 'Dismiss error' });
    fireEvent.click(btn);
    expect(spy).toHaveBeenCalledOnce();
  });
});

// -------------------------------------------------------------------------
// M2 regression: [object Object] / missing message guard
// -------------------------------------------------------------------------

describe('GameViewPanel error banner — M2 regression', () => {
  it('shows humanized label and NOT [object Object] when message is "[object Object]"', () => {
    render(
      <GameViewPanel
        connected={false}
        lastError={{ kind: 'notConnected', message: '[object Object]' }}
      />,
    );
    // Humanized label must appear
    expect(screen.getByText('Not connected')).toBeTruthy();
    // The literal "[object Object]" must NOT appear anywhere in the DOM
    expect(screen.queryByText(/\[object Object\]/)).toBeNull();
  });

  it('shows humanized label and NOT [object Object] when message is empty', () => {
    render(
      <GameViewPanel
        connected={false}
        lastError={{ kind: 'alreadyConnected', message: '' }}
      />,
    );
    expect(screen.getByText('Already connected')).toBeTruthy();
    expect(screen.queryByText(/\[object Object\]/)).toBeNull();
  });

  it('shows full "kind: message" when a real message is present', () => {
    render(
      <GameViewPanel
        connected={false}
        lastError={{ kind: 'process', message: 'engine executable not found: C:/x.exe' }}
      />,
    );
    // Both the label and the message text must be present
    expect(screen.getByText('Process error')).toBeTruthy();
    expect(screen.getByText(/engine executable not found/)).toBeTruthy();
  });
});

// -------------------------------------------------------------------------
// Reconnect button disabled state
// -------------------------------------------------------------------------

describe('GameViewPanel Reconnect button', () => {
  it('is enabled when connectionStatus is "error"', () => {
    render(
      <GameViewPanel
        connected={false}
        connectionStatus="error"
      />,
    );
    const btn = screen.getByRole('button', { name: 'Reconnect' });
    expect((btn as HTMLButtonElement).disabled).toBe(false);
  });

  it('is disabled when connectionStatus is "connecting"', () => {
    render(
      <GameViewPanel
        connected={false}
        connectionStatus="connecting"
      />,
    );
    const btn = screen.getByRole('button', { name: 'Reconnect' });
    expect((btn as HTMLButtonElement).disabled).toBe(true);
  });

  it('is disabled when connectionStatus is "disconnected"', () => {
    render(
      <GameViewPanel
        connected={false}
        connectionStatus="disconnected"
      />,
    );
    const btn = screen.getByRole('button', { name: 'Reconnect' });
    expect((btn as HTMLButtonElement).disabled).toBe(true);
  });

  it('is disabled when connectionStatus is undefined (safe fallback)', () => {
    render(
      <GameViewPanel
        connected={false}
        connectionStatus={undefined}
      />,
    );
    const btn = screen.getByRole('button', { name: 'Reconnect' });
    expect((btn as HTMLButtonElement).disabled).toBe(true);
  });

  it('is enabled when connectionStatus is "connected"', () => {
    render(
      <GameViewPanel
        connected={true}
        connectionStatus="connected"
      />,
    );
    const btn = screen.getByRole('button', { name: 'Reconnect' });
    expect((btn as HTMLButtonElement).disabled).toBe(false);
  });
});

// -------------------------------------------------------------------------
// Viewport state badge
// -------------------------------------------------------------------------

describe('GameViewPanel viewport state badge', () => {
  it('renders the viewport state label when viewportState is provided', () => {
    render(
      <GameViewPanel
        connected={false}
        viewportState="hidden"
      />,
    );
    expect(screen.getByText('Hidden')).toBeTruthy();
  });

  it('renders "--" when viewportState is undefined', () => {
    const { container } = render(
      <GameViewPanel
        connected={false}
        viewportState={undefined}
      />,
    );
    // The viewport badge row contains "Viewport:" label followed by "--"
    const viewportLabel = container.querySelector('.placeholder-box .label');
    expect(viewportLabel?.textContent).toBe('Viewport:');
    // At least one "--" must appear within the placeholder-box (viewport badge)
    const placeholderBox = container.querySelector('.placeholder-box');
    expect(placeholderBox?.textContent).toContain('--');
  });

  it('renders "Focused" for focused state', () => {
    render(
      <GameViewPanel
        connected={false}
        viewportState="focused"
      />,
    );
    expect(screen.getByText('Focused')).toBeTruthy();
  });
});
