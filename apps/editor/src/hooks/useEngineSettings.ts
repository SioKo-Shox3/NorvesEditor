/**
 * useEngineSettings — Settings ウィンドウのエンジン欄が使う状態と操作。
 *
 * Settings は別ウィンドウで開き、main 窓の store を共有しない。そのため値は
 * store を経由せず、マウント時に get_engine_settings でバックエンドから取り直し、
 * このフックのローカル状態に持つ。パスを変えるのは Rust 側のダイアログだけで、
 * 各コマンドは変更後(キャンセルなら現在)の設定を返すので、その返り値で表示を置き換える。
 *
 * 起動引数は入力欄の下書き(1 行 1 引数)をこのフックが持ち、保存でバックエンドへ送る。
 * 下書きを保存済みの値で置き換えるのは、初回の取得と保存の成功のときだけ
 * (パスの操作で書きかけの引数を消さない)。保存の応答待ちの間に下書きを編集したときは、
 * 応答で置き換えず新しい下書きを残す(下書きの編集の世代で判定する)。
 *
 * 応答は要求ごとの世代番号で照合し、最後に出した要求の応答だけを反映する。StrictMode の
 * 二重マウントなどで古い要求の応答が後から届いても、表示の上書きや処理中の解除はしない。
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import {
  clearEnginePath,
  getEngineSettings,
  pickEnginePath,
  setEngineArgs,
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
  /** コマンドの応答待ち。true の間は pick / clear / saveArgs を受け付けない。 */
  busy: boolean;
  /** 起動引数の入力欄の内容(1 行 1 引数)。 */
  argsDraft: string;
  /** 直前の起動引数の保存が成功したら true。次の操作を始めると false に戻る。 */
  argsSaved: boolean;
  pick: () => void;
  clear: () => void;
  setArgsDraft: (text: string) => void;
  saveArgs: () => void;
  dismissError: () => void;
}

function isEngineSettingsPayload(value: unknown): value is EngineSettingsPayload {
  if (value === null || typeof value !== 'object') return false;
  const v = value as Record<string, unknown>;
  const savedArgs = v['savedArgs'];
  return (
    typeof v['effectivePath'] === 'string' &&
    (v['source'] === 'env' || v['source'] === 'settings' || v['source'] === 'default') &&
    (v['savedPath'] === null || typeof v['savedPath'] === 'string') &&
    Array.isArray(savedArgs) &&
    savedArgs.every((arg) => typeof arg === 'string')
  );
}

/** 入力欄の内容を 1 行 1 引数に分ける。空行を捨てるのはバックエンド。 */
export function splitArgsDraft(text: string): string[] {
  return text.split(/\r\n|\n|\r/);
}

function joinArgs(args: readonly string[]): string {
  return args.join('\n');
}

type Command = () => Promise<EngineSettingsPayload>;

export function useEngineSettings(): EngineSettingsState {
  const [settings, setSettings] = useState<EngineSettingsPayload | undefined>(undefined);
  const [error, setError] = useState<EngineSettingsError | undefined>(undefined);
  const [busy, setBusy] = useState(true);
  const [argsDraft, setArgsDraft] = useState('');
  const [argsSaved, setArgsSaved] = useState(false);
  // 再描画を待たずに二重実行を弾くための印。disabled の反映より先に来た2回目のクリックも止める。
  const busyRef = useRef(true);
  const mountedRef = useRef(true);
  // 最後に出した要求の世代。これと一致しない応答は古いので捨てる。
  const requestRef = useRef(0);
  // 下書きを編集するたびに進める世代。保存の応答が来たとき、送った後に編集されたかを見る。
  const draftEditRef = useRef(0);

  const run = useCallback(
    (command: Command, onApplied?: (next: EngineSettingsPayload) => void): void => {
      const request = ++requestRef.current;
      const isCurrent = (): boolean => mountedRef.current && request === requestRef.current;
      busyRef.current = true;
      setBusy(true);
      setError(undefined);
      setArgsSaved(false);
      command()
        .then((next: unknown) => {
          if (!isCurrent()) return;
          // 形の合わない応答で欄ごと(ひいてはウィンドウごと)落とさず、エラーとして見せる。
          if (isEngineSettingsPayload(next)) {
            setSettings(next);
            onApplied?.(next);
          } else {
            setError({ message: 'エンジンの設定の応答が不正です' });
          }
        })
        .catch((err: unknown) => {
          if (isCurrent()) setError(extractBackendError(err));
        })
        .finally(() => {
          // 後から出した要求がまだ処理中なら、その要求の完了まで処理中のままにする。
          if (request !== requestRef.current) return;
          busyRef.current = false;
          if (mountedRef.current) setBusy(false);
        });
    },
    [],
  );

  useEffect(() => {
    mountedRef.current = true;
    run(getEngineSettings, (next) => setArgsDraft(joinArgs(next.savedArgs)));
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

  const saveArgs = useCallback((): void => {
    if (busyRef.current) return;
    const args = splitArgsDraft(argsDraft);
    const sentEdit = draftEditRef.current;
    run(
      () => setEngineArgs(args),
      (next) => {
        // 応答待ちの間に編集されていたら、新しい下書きを残し「保存しました」も出さない。
        if (draftEditRef.current !== sentEdit) return;
        // 空行を捨てた後の、実際に保存された並びを見せる。
        setArgsDraft(joinArgs(next.savedArgs));
        setArgsSaved(true);
      },
    );
  }, [run, argsDraft]);

  const editArgsDraft = useCallback((text: string): void => {
    draftEditRef.current += 1;
    setArgsDraft(text);
    setArgsSaved(false);
  }, []);

  const dismissError = useCallback((): void => {
    setError(undefined);
  }, []);

  return {
    settings,
    error,
    busy,
    argsDraft,
    argsSaved,
    pick,
    clear,
    setArgsDraft: editArgsDraft,
    saveArgs,
    dismissError,
  };
}
