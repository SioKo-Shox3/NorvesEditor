/**
 * AppTitleBar — container that wires the windowControls service into the
 * presentational TitleBar.
 *
 * This is the single place that connects the platform window side effects to
 * the title bar. TitleBar itself stays presentational (callback props only);
 * this container owns the side-effecting calls and the maximised-state that
 * drives the maximise button glyph/label.
 *
 * Maximised state is kept in sync by:
 *   - querying isWindowMaximized() once on mount,
 *   - re-querying after a toggle, and
 *   - subscribing to the window resize event (via getCurrentWindow().onResized,
 *     which uses the already-granted core:event permission, not a window
 *     permission) so OS-driven maximise/restore is reflected too.
 *
 * The window-control callbacks are fire-and-forget: rejections are swallowed
 * rather than surfaced, since a failed minimise/close is non-fatal and there is
 * no UI affordance for it in alpha.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import type React from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { TitleBar } from '../components/shell/TitleBar.js';
import {
  closeWindow,
  isWindowMaximized,
  minimizeWindow,
  toggleMaximizeWindow,
} from './windowControls.js';

export interface AppTitleBarProps {
  /** Title text shown in the bar. */
  title: string;
}

/** Swallow a rejected window operation; failures are non-fatal in alpha. */
function ignoreRejection(promise: Promise<unknown>): void {
  promise.catch(() => {
    // Intentionally ignored: a failed window control is non-fatal.
  });
}

export function AppTitleBar({ title }: AppTitleBarProps): React.JSX.Element {
  const [maximized, setMaximized] = useState(false);
  // Tracks whether the component is still mounted so async callbacks never
  // call setMaximized after unmount (consistent with the onResized guard).
  const mountedRef = useRef(true);

  // Refresh the cached maximised state from the platform window.
  const refreshMaximized = useCallback((): void => {
    isWindowMaximized()
      .then((value) => {
        if (mountedRef.current) {
          setMaximized(value);
        }
      })
      .catch(() => {
        // Ignore: keep the last known state on query failure.
      });
  }, []);

  // Initial query + stay in sync with OS-driven resize/maximise.
  useEffect(() => {
    refreshMaximized();

    let unlisten: (() => void) | undefined;
    let cancelled = false;
    getCurrentWindow()
      .onResized(() => {
        refreshMaximized();
      })
      .then((fn) => {
        if (cancelled) {
          fn();
        } else {
          unlisten = fn;
        }
      })
      .catch(() => {
        // Ignore: resize sync is best-effort.
      });

    return () => {
      mountedRef.current = false;
      cancelled = true;
      unlisten?.();
    };
  }, [refreshMaximized]);

  const handleMinimize = useCallback((): void => {
    ignoreRejection(minimizeWindow());
  }, []);

  const handleToggleMaximize = useCallback((): void => {
    toggleMaximizeWindow()
      .then(refreshMaximized)
      .catch(() => {
        // Ignore: non-fatal.
      });
  }, [refreshMaximized]);

  const handleClose = useCallback((): void => {
    ignoreRejection(closeWindow());
  }, []);

  return (
    <TitleBar
      title={title}
      onMinimize={handleMinimize}
      onToggleMaximize={handleToggleMaximize}
      onClose={handleClose}
      isMaximized={maximized}
    />
  );
}
