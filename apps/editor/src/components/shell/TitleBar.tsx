/**
 * TitleBar — application title bar with custom window controls.
 *
 * Presentational and side-effect-free: window operations are injected as
 * callback props (onMinimize / onToggleMaximize / onClose). The component
 * never imports @tauri-apps/api itself, which keeps it engine-agnostic and
 * trivially mockable in tests. The App shell wires the real windowControls
 * service into these props.
 *
 * Drag-to-move: the bar carries `data-tauri-drag-region` (R8). Interactive
 * children (the window-control buttons) must NOT carry that attribute, or the
 * drag region swallows their clicks; they instead opt out via `app-region:
 * no-drag` in CSS.
 *
 * Double-click to maximise is handled manually in `onMouseDown` (e.detail===2)
 * rather than relying on the drag region's built-in double-click, which has a
 * known Tauri bug (tauri-apps/tauri#11945). This also means only
 * `allow-toggle-maximize` is needed — no separate maximise/unmaximize grant.
 *
 * Uses .titlebar CSS class (defined in styles.css) for layout.
 */

import type React from 'react';

// -------------------------------------------------------------------------
// Props
// -------------------------------------------------------------------------

export interface TitleBarProps {
  /** Title text rendered at the start of the bar. */
  title: string;
  /**
   * Optional slot for extra action content placed before the window controls.
   * Reserved for future use; window controls are rendered unconditionally.
   */
  actions?: React.ReactNode;
  /** Optional extra CSS class(es) to append to the root element. */
  className?: string;
  /** Minimise the window. */
  onMinimize: () => void;
  /** Toggle the window between maximised and restored. */
  onToggleMaximize: () => void;
  /** Close the window. */
  onClose: () => void;
  /**
   * Whether the window is currently maximised. Drives the maximise button's
   * glyph and aria-label ("最大化" vs "元に戻す"). Defaults to false.
   */
  isMaximized?: boolean;
}

// -------------------------------------------------------------------------
// Component
// -------------------------------------------------------------------------

export function TitleBar({
  title,
  actions,
  className,
  onMinimize,
  onToggleMaximize,
  onClose,
  isMaximized = false,
}: TitleBarProps): React.JSX.Element {
  const rootClass = ['titlebar', ...(className !== undefined ? [className] : [])].join(' ');

  // Manual double-click-to-maximise on the drag region. The drag region's
  // built-in double-click maximise is unreliable (tauri#11945), so detect the
  // second click here and toggle explicitly. e.detail === 2 is the dblclick.
  const handleBarMouseDown = (event: React.MouseEvent<HTMLDivElement>): void => {
    if (event.detail === 2) {
      onToggleMaximize();
    }
  };

  const maximizeLabel = isMaximized ? '元に戻す' : '最大化';

  return (
    <div
      className={rootClass}
      data-tauri-drag-region
      onMouseDown={handleBarMouseDown}
    >
      <span className="titlebar__title" data-tauri-drag-region>{title}</span>
      {actions !== undefined && <div className="titlebar__actions" data-tauri-drag-region>{actions}</div>}
      <div className="titlebar__controls">
        <button
          type="button"
          className="titlebar__control"
          aria-label="最小化"
          onClick={onMinimize}
        >
          {'–'}
        </button>
        <button
          type="button"
          className="titlebar__control"
          aria-label={maximizeLabel}
          onClick={onToggleMaximize}
        >
          {isMaximized ? '⧉' : '☐'}
        </button>
        <button
          type="button"
          className="titlebar__control titlebar__control--close"
          aria-label="閉じる"
          onClick={onClose}
        >
          {'✕'}
        </button>
      </div>
    </div>
  );
}
