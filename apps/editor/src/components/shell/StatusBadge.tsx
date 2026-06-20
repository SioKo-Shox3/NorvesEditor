/**
 * StatusBadge — presentational connection-status indicator.
 *
 * Props-driven, stateless, async-free.
 * No Bridge / Tauri / window API dependency.
 * Engine-agnostic: accepts any ConnectionStatus value from the store.
 *
 * Renders the existing `.status-badge` CSS class hierarchy defined in styles.css.
 * Modifier class selection is driven by a lookup table so all ConnectionStatus
 * values are covered at compile time (satisfies Record<ConnectionStatus, string>).
 */

import type React from 'react';
import type { ConnectionStatus } from '../../state/store.js';

// -------------------------------------------------------------------------
// Modifier-class map — exhaustive over ConnectionStatus
// -------------------------------------------------------------------------

const STATUS_CSS: Record<ConnectionStatus, string> = {
  disconnected: 'status-badge--disconnected',
  connecting:   'status-badge--warning',
  connected:    'status-badge--connected',
  error:        'status-badge--error',
};

// -------------------------------------------------------------------------
// Props
// -------------------------------------------------------------------------

export interface StatusBadgeProps {
  /** Current connection status; drives dot colour and ARIA label. */
  status: ConnectionStatus;
  /** Human-readable label shown next to the dot. */
  label: string;
  /** Optional extra CSS class(es) to append to the root element. */
  className?: string;
}

// -------------------------------------------------------------------------
// Component
// -------------------------------------------------------------------------

export function StatusBadge({ status, label, className }: StatusBadgeProps): React.JSX.Element {
  const modifierClass = STATUS_CSS[status];
  const rootClass = [
    'status-badge',
    modifierClass,
    ...(className !== undefined ? [className] : []),
  ].join(' ');

  return (
    <span className={rootClass} role="status" aria-label={`${label} (${status})`}>
      <span className="status-badge__dot" aria-hidden="true" />
      {label}
    </span>
  );
}
