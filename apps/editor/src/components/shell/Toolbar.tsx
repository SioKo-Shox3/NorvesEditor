/**
 * Toolbar — presentational horizontal strip container.
 *
 * Props-driven, stateless, async-free.
 * No Bridge / Tauri / window API dependency.
 * Engine-agnostic: generic children container with no action wiring.
 * Action wiring is deferred to P3.
 *
 * Uses --header-h token for height (already defined in styles.css :root).
 * user-select: none is applied via .toolbar CSS class.
 */

import type React from 'react';

// -------------------------------------------------------------------------
// Props
// -------------------------------------------------------------------------

export interface ToolbarProps {
  /** Toolbar content — typically buttons or button groups. */
  children?: React.ReactNode;
  /** Optional extra CSS class(es) to append to the root element. */
  className?: string;
}

// -------------------------------------------------------------------------
// Component
// -------------------------------------------------------------------------

export function Toolbar({ children, className }: ToolbarProps): React.JSX.Element {
  const rootClass = ['toolbar', ...(className !== undefined ? [className] : [])].join(' ');

  return (
    <div className={rootClass} role="toolbar">
      {children}
    </div>
  );
}
