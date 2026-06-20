// @vitest-environment jsdom
/**
 * StatusBadge component tests — P1 shell primitives.
 *
 * Placed under shell/__tests__/ to keep the shell subtree self-contained:
 * each subsequent phase adds its own components and tests here without
 * scattering them across the top-level __tests__/ directory.
 *
 * These tests verify only the presentational contract:
 *   - The correct label text is rendered.
 *   - The status-badge modifier class is applied for each ConnectionStatus.
 *   - The role="status" element is present for accessibility.
 *   - The aria-label combines label and status.
 *   - An extra className is forwarded to the root element.
 *
 * No Bridge / Tauri / BridgeContext dependency: StatusBadge is fully
 * props-driven and engine-agnostic.
 */

import { describe, it, expect, afterEach } from 'vitest';
import { render, screen, cleanup } from '@testing-library/react';
import { StatusBadge } from '../StatusBadge.js';
import type { ConnectionStatus } from '../../../state/store.js';

afterEach(cleanup);

// -------------------------------------------------------------------------
// Label rendering
// -------------------------------------------------------------------------

describe('StatusBadge label', () => {
  it('renders the supplied label text', () => {
    render(<StatusBadge status="connected" label="Connected" />);
    expect(screen.getByText('Connected')).toBeTruthy();
  });

  it('renders an arbitrary label', () => {
    render(<StatusBadge status="error" label="Connection error" />);
    expect(screen.getByText('Connection error')).toBeTruthy();
  });
});

// -------------------------------------------------------------------------
// role="status" accessibility
// -------------------------------------------------------------------------

describe('StatusBadge accessibility', () => {
  it('has role="status" on the root element', () => {
    render(<StatusBadge status="disconnected" label="Disconnected" />);
    expect(screen.getByRole('status')).toBeTruthy();
  });

  it('sets aria-label combining label and status', () => {
    render(<StatusBadge status="connecting" label="Connecting..." />);
    const el = screen.getByRole('status');
    expect(el.getAttribute('aria-label')).toBe('Connecting... (connecting)');
  });
});

// -------------------------------------------------------------------------
// Modifier CSS classes — one test per ConnectionStatus value
// -------------------------------------------------------------------------

describe('StatusBadge modifier classes', () => {
  const cases: Array<{ status: ConnectionStatus; expectedClass: string }> = [
    { status: 'disconnected', expectedClass: 'status-badge--disconnected' },
    { status: 'connecting',   expectedClass: 'status-badge--warning'      },
    { status: 'connected',    expectedClass: 'status-badge--connected'    },
    { status: 'error',        expectedClass: 'status-badge--error'        },
  ];

  for (const { status, expectedClass } of cases) {
    it(`applies "${expectedClass}" for status="${status}"`, () => {
      render(<StatusBadge status={status} label={status} />);
      const el = screen.getByRole('status');
      expect(el.classList.contains('status-badge')).toBe(true);
      expect(el.classList.contains(expectedClass)).toBe(true);
    });
  }
});

// -------------------------------------------------------------------------
// Optional className forwarding
// -------------------------------------------------------------------------

describe('StatusBadge className forwarding', () => {
  it('appends extra className to the root element', () => {
    render(<StatusBadge status="connected" label="OK" className="my-badge" />);
    const el = screen.getByRole('status');
    expect(el.classList.contains('my-badge')).toBe(true);
  });
});
