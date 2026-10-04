/**
 * useMcpSettings — Settings ウィンドウから MCP のバックエンド設定を操作する。
 *
 * Settings は別ウィンドウなので main 窓の store を共有せず、マウント時に
 * get_mcp_settings で現在値を取得する。トークンは表示を求められたときだけ取得し、
 * 隠す操作やアンマウントの後に届いた応答は画面へ反映しない。
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import {
  getMcpSettings,
  getMcpToken,
  regenerateMcpToken,
  setMcpSettings,
  setMcpWriteAccess,
  type McpSettingsPayload,
  type McpWriteMode,
} from '@norves/bridge-ui';
import { extractBackendError } from './useBridge.js';

export interface McpSettingsError {
  kind?: string;
  message: string;
}

export interface McpSettingsState {
  settings: McpSettingsPayload | undefined;
  error: McpSettingsError | undefined;
  busy: boolean;
  enabledDraft: boolean;
  portDraft: string;
  writeModeDraft: McpWriteMode;
  sceneRootDraft: string;
  token: string | undefined;
  tokenBusy: boolean;
  setEnabledDraft: (enabled: boolean) => void;
  setPortDraft: (port: string) => void;
  setWriteModeDraft: (mode: McpWriteMode) => void;
  setSceneRootDraft: (root: string) => void;
  saveWriteAccess: () => void;
  saveSettings: () => void;
  showToken: () => void;
  hideToken: () => void;
  regenerateToken: () => void;
  dismissError: () => void;
}

function isMcpSettingsPayload(value: unknown): value is McpSettingsPayload {
  if (value === null || typeof value !== 'object') return false;
  const v = value as Record<string, unknown>;
  return (
    typeof v['enabled'] === 'boolean' &&
    typeof v['port'] === 'number' &&
    Number.isInteger(v['port']) &&
    v['port'] >= 1 && v['port'] <= 65535 &&
    (v['state'] === 'disabled' || v['state'] === 'running' ||
      v['state'] === 'bindFailed' || v['state'] === 'storageFailed') &&
    (v['endpoint'] === undefined || typeof v['endpoint'] === 'string') &&
    (v['error'] === undefined || typeof v['error'] === 'string') &&
    (v['writeMode'] === undefined || v['writeMode'] === 'readOnly' ||
      v['writeMode'] === 'enabled' || v['writeMode'] === 'confirm') &&
    (v['sceneRootId'] === undefined || typeof v['sceneRootId'] === 'string')
  );
}

function isMcpTokenPayload(value: unknown): value is { token: string } {
  return (
    value !== null && typeof value === 'object' &&
    typeof (value as Record<string, unknown>)['token'] === 'string' &&
    (value as Record<string, string>)['token'].length > 0
  );
}

function portNumber(text: string): number | undefined {
  const trimmed = text.trim();
  if (!/^\d+$/.test(trimmed)) return undefined;
  const value = Number(trimmed);
  return Number.isInteger(value) && value >= 1 && value <= 65535 ? value : undefined;
}

type SettingsCommand = () => Promise<McpSettingsPayload>;

export function useMcpSettings(): McpSettingsState {
  const [settings, setSettings] = useState<McpSettingsPayload | undefined>(undefined);
  const [error, setError] = useState<McpSettingsError | undefined>(undefined);
  const [busy, setBusy] = useState(true);
  const [enabledDraft, setEnabledDraft] = useState(false);
  const [portDraft, setPortDraft] = useState('49770');
  const [writeModeDraft, setWriteModeDraft] = useState<McpWriteMode>('readOnly');
  const [sceneRootDraft, setSceneRootDraft] = useState('');
  const [token, setToken] = useState<string | undefined>(undefined);
  const [tokenBusy, setTokenBusy] = useState(false);
  const busyRef = useRef(true);
  const tokenBusyRef = useRef(false);
  const mountedRef = useRef(true);
  const requestRef = useRef(0);
  const tokenRequestRef = useRef(0);

  const applySettings = useCallback((next: McpSettingsPayload): void => {
    setSettings(next);
    setEnabledDraft(next.enabled);
    setPortDraft(String(next.port));
    setWriteModeDraft(next.writeMode ?? 'readOnly');
    setSceneRootDraft(next.sceneRootId ?? '');
  }, []);

  const run = useCallback(
    (command: SettingsCommand): void => {
      const request = ++requestRef.current;
      const isCurrent = (): boolean => mountedRef.current && request === requestRef.current;
      busyRef.current = true;
      setBusy(true);
      setError(undefined);
      command()
        .then((next: unknown) => {
          if (!isCurrent()) return;
          if (isMcpSettingsPayload(next)) {
            applySettings(next);
          } else {
            setError({ message: 'MCP設定の応答が不正です' });
          }
        })
        .catch((err: unknown) => {
          if (isCurrent()) setError(extractBackendError(err));
        })
        .finally(() => {
          if (request !== requestRef.current) return;
          busyRef.current = false;
          if (mountedRef.current) setBusy(false);
        });
    },
    [applySettings],
  );

  useEffect(() => {
    mountedRef.current = true;
    run(getMcpSettings);
    return () => {
      mountedRef.current = false;
      requestRef.current += 1;
      tokenRequestRef.current += 1;
      busyRef.current = false;
      tokenBusyRef.current = false;
    };
  }, [run]);

  const saveSettings = useCallback((): void => {
    if (busyRef.current || tokenBusyRef.current) return;
    const port = portNumber(portDraft);
    if (port === undefined) {
      setError({ message: 'ポート番号は1〜65535の整数で指定してください' });
      return;
    }
    run(() => setMcpSettings(enabledDraft, port));
  }, [enabledDraft, portDraft, run]);

  const hideToken = useCallback((): void => {
    tokenRequestRef.current += 1;
    tokenBusyRef.current = false;
    setTokenBusy(false);
    setToken(undefined);
  }, []);

  const saveWriteAccess = useCallback((): void => {
    if (busyRef.current || tokenBusyRef.current) return;
    const root = sceneRootDraft.trim();
    run(() => setMcpWriteAccess(writeModeDraft, root || undefined));
  }, [writeModeDraft, sceneRootDraft, run]);

  const showToken = useCallback((): void => {
    if (busyRef.current || tokenBusyRef.current) return;
    const request = ++tokenRequestRef.current;
    tokenBusyRef.current = true;
    setTokenBusy(true);
    setToken(undefined);
    setError(undefined);
    getMcpToken()
      .then((value: unknown) => {
        if (!mountedRef.current || request !== tokenRequestRef.current) return;
        if (isMcpTokenPayload(value)) {
          setToken(value.token);
        } else {
          setError({ message: 'MCPトークンの応答が不正です' });
        }
      })
      .catch((err: unknown) => {
        if (mountedRef.current && request === tokenRequestRef.current) {
          setError(extractBackendError(err));
        }
      })
      .finally(() => {
        if (request !== tokenRequestRef.current) return;
        tokenBusyRef.current = false;
        if (mountedRef.current) setTokenBusy(false);
      });
  }, []);

  const regenerateToken = useCallback((): void => {
    if (busyRef.current || tokenBusyRef.current) return;
    hideToken();
    run(regenerateMcpToken);
  }, [hideToken, run]);

  const dismissError = useCallback((): void => {
    setError(undefined);
  }, []);

  return {
    settings,
    error,
    busy,
    enabledDraft,
    portDraft,
    writeModeDraft,
    sceneRootDraft,
    token,
    tokenBusy,
    setEnabledDraft,
    setPortDraft,
    setWriteModeDraft,
    setSceneRootDraft,
    saveWriteAccess,
    saveSettings,
    showToken,
    hideToken,
    regenerateToken,
    dismissError,
  };
}
