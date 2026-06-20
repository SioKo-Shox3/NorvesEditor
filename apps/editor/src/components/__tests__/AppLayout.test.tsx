// @vitest-environment jsdom
/**
 * AppLayout minimum rendering test — Phase 1.
 *
 * AppLayout renders a DockviewReact container. In tests we mock dockview-react
 * to avoid ResizeObserver / DOM layout APIs that jsdom lacks.
 *
 * We verify that AppLayout renders without throwing when wrapped in BridgeProvider.
 */

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup } from '@testing-library/react';
import React from 'react';
import { BridgeProvider } from '../../state/BridgeContext.js';

// -------------------------------------------------------------------------
// Mock dockview-react — prevents ResizeObserver and other browser API errors
// -------------------------------------------------------------------------

vi.mock('dockview-react', () => ({
  DockviewReact: () => React.createElement('div', { 'data-testid': 'dockview-root' }),
}));

import { AppLayout } from '../AppLayout.js';

afterEach(cleanup);

describe('AppLayout', () => {
  it('renders without throwing inside BridgeProvider', () => {
    expect(() => {
      render(
        React.createElement(BridgeProvider, null,
          React.createElement(AppLayout),
        ),
      );
    }).not.toThrow();
  });

  it('renders the dockview container element', () => {
    const { getByTestId } = render(
      React.createElement(BridgeProvider, null,
        React.createElement(AppLayout),
      ),
    );
    expect(getByTestId('dockview-root')).toBeTruthy();
  });
});
