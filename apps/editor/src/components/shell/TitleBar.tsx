/**
 * TitleBar — presentational application title bar.
 *
 * Props-driven, stateless, async-free.
 * No Bridge / Tauri / window API dependency.
 * data-tauri-drag-region and window API calls are deferred to P2.
 * Engine-agnostic: displays a title string and optional action slot.
 *
 * Uses .titlebar CSS class (defined in styles.css) for layout.
 */

import type React from 'react';

// -------------------------------------------------------------------------
// Props
// -------------------------------------------------------------------------

export interface TitleBarProps {
  /** Title text rendered in the centre / start of the bar. */
  title: string;
  /**
   * Optional slot for action buttons (e.g. window controls).
   * Injected at P2; at P1 this remains optional and unused.
   */
  actions?: React.ReactNode;
  /** Optional extra CSS class(es) to append to the root element. */
  className?: string;
}

// -------------------------------------------------------------------------
// Component
// -------------------------------------------------------------------------

export function TitleBar({ title, actions, className }: TitleBarProps): React.JSX.Element {
  const rootClass = ['titlebar', ...(className !== undefined ? [className] : [])].join(' ');

  return (
    <div className={rootClass}>
      <span className="titlebar__title">{title}</span>
      {actions !== undefined && (
        <div className="titlebar__actions">{actions}</div>
      )}
    </div>
  );
}
