/** main画面で確認待ちを購読し、パネルを閉じても期限と通知を維持する。 */
import { createContext, useCallback, useEffect, useRef, useState } from 'react';
import {
  getMcpSettings,
  subscribeEvent,
  BRIDGE_EVENTS,
  type McpWriteMode,
  type UnlistenFn,
} from '@norves/bridge-ui';
import {
  approveMcpConfirmation,
  rejectMcpConfirmation,
  getMcpConfirmations,
  type McpConfirmationRequest,
} from '../shell/mcpApprovals.js';

export interface McpApprovalsState {
  requests: McpConfirmationRequest[];
  mode: McpWriteMode | undefined;
  loading: boolean;
  busyIds: ReadonlySet<string>;
  notice: string;
  error: string | undefined;
  decide: (id: string, approved: boolean) => void;
}

export const McpApprovalsContext = createContext<McpApprovalsState | null>(null);

export function useMcpApprovals(): McpApprovalsState {
  const [requests, setRequests] = useState<McpConfirmationRequest[]>([]);
  const [mode, setMode] = useState<McpWriteMode>();
  const [loading, setLoading] = useState(true);
  const [busyIds, setBusyIds] = useState<ReadonlySet<string>>(new Set());
  const [notice, setNotice] = useState('');
  const [error, setError] = useState<string>();
  const current = useRef({ requests, mode });
  current.current = { requests, mode };
  const inFlight = useRef(new Set<string>());
  const lifecycle = useRef(0);
  // 初期取得と決定中に届く通知を、遅い応答で巻き戻さない。
  const revision = useRef(0);

  useEffect(() => {
    const epoch = ++lifecycle.current;
    let active = true;
    let unlisten: UnlistenFn | undefined;
    let settingsRequest = 0;
    setLoading(true);
    setMode(undefined);
    inFlight.current.clear();
    setBusyIds(new Set());

    const refreshMode = async (): Promise<void> => {
      const request = ++settingsRequest;
      current.current.mode = undefined;
      setMode(undefined);
      try {
        const settings = await getMcpSettings();
        if (!active || request !== settingsRequest) return;
        const next = settings.writeMode ?? 'readOnly';
        current.current.mode = next;
        setMode(next);
      } catch {
        if (active && request === settingsRequest) {
          setError('書き込み許可を取得できません。メイン画面を選び直して再取得してください。');
        }
      }
    };

    const apply = (next: McpConfirmationRequest[]): void => {
      const live = next.filter((request) => request.expiresAt > Date.now());
      const previous = current.current.requests;
      if (live.some((request) => !previous.some((old) => old.id === request.id))) {
        setNotice('MCPから書き込みの確認が届いています。');
      } else if (previous.some((old) => !live.some((request) => request.id === old.id))) {
        setNotice('確認待ちが終了しました（決定・期限切れ・取り下げ）。');
      }
      current.current.requests = live;
      setRequests(live);
      setLoading(false);
    };

    const initialize = async (): Promise<void> => {
      try {
        // 購読を先に確立し、初期取得の間に起きた取り下げを失わない。
        const dispose = await subscribeEvent<McpConfirmationRequest[]>(
          BRIDGE_EVENTS.mcpConfirmationsChanged,
          (next) => {
            if (!active) return;
            revision.current += 1;
            apply(next);
            void refreshMode();
          },
        );
        if (!active) {
          dispose();
          return;
        }
        unlisten = dispose;
        const version = revision.current;
        const [next] = await Promise.all([getMcpConfirmations(), refreshMode()]);
        if (active && revision.current === version) apply(next);
      } catch {
        if (active) {
          setError('確認待ちを取得できません。メイン画面を開き直してください。');
          setLoading(false);
        }
      }
    };
    void initialize();
    const onFocus = (): void => { void refreshMode(); };
    window.addEventListener('focus', onFocus);
    return () => {
      active = false;
      if (lifecycle.current === epoch) lifecycle.current += 1;
      unlisten?.();
      window.removeEventListener('focus', onFocus);
    };
  }, []);

  useEffect(() => {
    if (requests.length === 0) return;
    const deadline = Math.min(...requests.map((request) => request.expiresAt));
    const timer = window.setTimeout(() => {
      revision.current += 1;
      const live = current.current.requests.filter((request) => request.expiresAt > Date.now());
      current.current.requests = live;
      setRequests(live);
      setNotice('確認期限が切れました。期限切れの操作は承認されません。');
    }, Math.max(0, deadline - Date.now()));
    return () => window.clearTimeout(timer);
  }, [requests]);

  const decide = useCallback((id: string, approved: boolean): void => {
    const state = current.current;
    const request = state.requests.find((entry) => entry.id === id);
    if (!request || request.expiresAt <= Date.now() || inFlight.current.has(id)) return;
    if (approved && state.mode !== 'enabled' && state.mode !== 'confirm') return;
    const epoch = lifecycle.current;
    inFlight.current.add(id);
    setBusyIds(new Set(inFlight.current));
    setError(undefined);
    const command = approved ? approveMcpConfirmation : rejectMcpConfirmation;
    void command(id).then(() => {
      if (epoch !== lifecycle.current) return;
      revision.current += 1;
      const next = current.current.requests.filter((entry) => entry.id !== id);
      current.current.requests = next;
      setRequests(next);
      setNotice(approved
        ? '今回の要求を承認しました。状態が変わっている場合は再確認が届きます。'
        : '今回の要求を拒否しました。');
    }).catch(() => {
      if (epoch === lifecycle.current) {
        setError('確認への応答に失敗しました。期限切れや取り下げの可能性があります。');
      }
    }).finally(() => {
      if (epoch !== lifecycle.current) return;
      inFlight.current.delete(id);
      setBusyIds(new Set(inFlight.current));
    });
  }, []);

  return { requests, mode, loading, busyIds, notice, error, decide };
}
