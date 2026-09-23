/**
 * useEngineSettings — Settings ウィンドウのエンジン欄が使う状態と操作。
 *
 * Settings は別ウィンドウで開き、main 窓の store を共有しない。そのため値は
 * store を経由せず、マウント時に get_engine_settings でバックエンドから取り直し、
 * このフックのローカル状態に持つ。パスを変えるのは Rust 側のダイアログだけで、
 * 各コマンドは変更後(キャンセルなら現在)の設定を返すので、その返り値で表示を置き換える。
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import {
  clearEnginePath,
  getEngineSettings,
  pickEnginePath,
  type EngineSettingsPayload,
} from '@norves/bridge-ui';
import { extractBackendError } from './useBridge.js';

export interface EngineSettingsError {
  kind?: string;
  message: string;
}

export interface EngineSettingsState {
  /** 読み込み前、または読み込みに失敗したときは undefined。 */
  settings: EngineSettingsPayload | undefined;
  error: EngineSettingsError | undefined;
  /** コマンドの応答待ち。true の間は pick / clear を受け付けない。 */
  busy: boolean;
  pick: () => void;
  clear: () => void;
  dismissError: () => void;
}

function isEngineSettingsPayload(value: unknown): value is EngineSettingsPayload {
  if (value === null || typeof value !== 'object') return false;
  const v = value as Record<string, unknown>;
  return (
    typeof v['effectivePath'] === 'string' &&
    (v['source'] === 'env' || v['source'] === 'settings' || v['source'] === 'default') &&
    (v['savedPath'] === null || typeof v['savedPath'] === 'string')
  );
}

export function useEngineSettings(): EngineSettingsState {
  const [settings, setSettings] = useState<EngineSettingsPayload | undefined>(undefined);
  const [error, setError] = useState<EngineSettingsError | undefined>(undefined);
  const [busy, setBusy] = useState(true);
  // 再描画を待たずに二重実行を弾くための印。disabled の反映より先に来た2回目のクリックも止める。
  const busyRef = useRef(true);
  const mountedRef = useRef(true);

  const run = useCallback((command: () => Promise<EngineSettingsPayload>): void => {
    busyRef.current = true;
    setBusy(true);
    setError(undefined);
    command()
      .then((next: unknown) => {
        if (!mountedRef.current) return;
        // 形の合わない応答で欄ごと(ひいてはウィンドウごと)落とさず、エラーとして見せる。
        if (isEngineSettingsPayload(next)) {
          setSettings(next);
        } else {
          setError({ message: 'エンジンの設定の応答が不正です' });
        }
      })
      .catch((err: unknown) => {
        if (mountedRef.current) setError(extractBackendError(err));
      })
      .finally(() => {
        busyRef.current = false;
        if (mountedRef.current) setBusy(false);
      });
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    run(getEngineSettings);
    return () => {
      mountedRef.current = false;
    };
  }, [run]);

  const pick = useCallback((): void => {
    if (busyRef.current) return;
    run(pickEnginePath);
  }, [run]);

  const clear = useCallback((): void => {
    if (busyRef.current) return;
    run(clearEnginePath);
  }, [run]);

  const dismissError = useCallback((): void => {
    setError(undefined);
  }, []);

  return { settings, error, busy, pick, clear, dismissError };
}
