/** 操作記録の取得・購読と、共通履歴の先頭だけを取り消す処理を管理する。 */
import { useCallback, useEffect, useRef, useState } from 'react';
import {
  BRIDGE_EVENTS,
  editGetHistory,
  editUndo,
  getMcpOperations,
  subscribeEvent,
  type McpOperation,
  type McpOperationsPayload,
  type UnlistenFn,
} from '@norves/bridge-ui';
import { useBridgeDispatch, useBridgeState } from '../state/BridgeContext.js';
import type { BridgeState } from '../state/store.js';

function unavailableReason(
  record: McpOperation,
  snapshot: McpOperationsPayload | undefined,
  state: BridgeState,
  busy: boolean,
  historyUncertain: boolean,
): string | undefined {
  if (!snapshot || record.sessionId !== snapshot.sessionId) return '別の起動の記録です。';
  if (!record.displayGroupId) return '取り消せる履歴のまとまりがありません。';
  if (!record.actorFinished) return '操作の結果を確認中です。';
  if (busy) return '取り消し処理中です。';
  if (historyUncertain) return '履歴を取得できません。パネルを開き直してください。';
  if (state.connection.status !== 'connected') return 'エンジンに接続してください。';
  if (state.sceneEditUnsupported) return 'この接続ではシーン編集を利用できません。';
  const history = state.editHistorySummary;
  if (!history) return '履歴を確認中です。';
  if (history.pending) return '履歴の処理が保留中です。画面上部で状態確認・再試行・破棄を行ってください。';
  if (history.undoGroup?.id !== record.displayGroupId) {
    return '現在の取り消し対象ではありません。後の編集から順に取り消してください。履歴が破棄済みの場合も取り消せません。';
  }
  if (!history.canUndo || history.undoHeadId === null) return '現在このまとまりは取り消せません。';
  return undefined;
}

export function useMcpOperations() {
  const bridge = useBridgeState();
  const dispatch = useBridgeDispatch();
  const bridgeRef = useRef(bridge);
  bridgeRef.current = bridge;
  const [snapshot, setSnapshot] = useState<McpOperationsPayload>();
  const snapshotRef = useRef<McpOperationsPayload | undefined>(undefined);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string>();
  const [undoError, setUndoError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const [historyUncertain, setHistoryUncertain] = useState(false);
  const uncertainRef = useRef(false);
  const lifecycle = useRef(0);

  useEffect(() => {
    const epoch = ++lifecycle.current;
    let active = true;
    let unlisten: UnlistenFn | undefined;
    let eventVersion = 0;
    const retiredSessions = new Set<string>();
    snapshotRef.current = undefined;
    setSnapshot(undefined);
    setLoading(true);
    setError(undefined);
    busyRef.current = false;
    setBusy(false);

    const apply = (next: McpOperationsPayload): void => {
      if (!active || retiredSessions.has(next.sessionId)) return;
      const previous = snapshotRef.current;
      if (previous?.sessionId === next.sessionId && next.revision < previous.revision) return;
      if (previous && previous.sessionId !== next.sessionId) retiredSessions.add(previous.sessionId);
      snapshotRef.current = next;
      setSnapshot(next);
      setLoading(false);
      setError(undefined);
    };

    void (async () => {
      try {
        const dispose = await subscribeEvent<McpOperationsPayload>(
          BRIDGE_EVENTS.mcpOperationsChanged,
          (next) => {
            if (!active) return;
            eventVersion += 1;
            apply(next);
          },
        );
        if (!active) { dispose(); return; }
        unlisten = dispose;
        const version = eventVersion;
        try {
          const initial = await getMcpOperations();
          // 通知後の初期応答は、同一起動の新しい改訂だけを採用する。
          if (version === eventVersion || initial.sessionId === snapshotRef.current?.sessionId) apply(initial);
        } catch {
          if (active && version === eventVersion) {
            setError('操作記録を取得できません。パネルを閉じて開き直してください。');
            setLoading(false);
          }
        }
      } catch {
        if (active) {
          setError('操作記録の通知を受信できません。パネルを閉じて開き直してください。');
          setLoading(false);
        }
      }
    })();
    return () => {
      active = false;
      if (lifecycle.current === epoch) lifecycle.current += 1;
      unlisten?.();
    };
  }, []);

  const undo = useCallback(async (requestId: string): Promise<void> => {
    const current = snapshotRef.current;
    const record = current?.records.find((entry) => entry.requestId === requestId);
    const state = bridgeRef.current;
    if (!record || unavailableReason(record, current, state, busyRef.current, uncertainRef.current)) return;
    const history = state.editHistorySummary;
    if (!history) return;
    const epoch = lifecycle.current;
    busyRef.current = true;
    setBusy(true);
    setUndoError(undefined);
    try {
      // クリック時点の先頭と改訂を指定し、列待ち中に別の編集が入ったら拒否させる。
      await editUndo(history.undoHeadId, history.undoRevision);
    } catch {
      if (epoch === lifecycle.current) {
        setUndoError('取り消しに失敗しました。履歴や保留状態を確認してください。自動再送は行いません。');
      }
    } finally {
      // 成否にかかわらず、部分失敗・拒否を含む共通履歴を取り直す。
      try {
        const summary = await editGetHistory();
        if (epoch === lifecycle.current) dispatch({ type: 'editHistorySummaryReceived', summary });
      } catch {
        if (epoch === lifecycle.current) {
          uncertainRef.current = true;
          setHistoryUncertain(true);
          setUndoError('取り消し後の履歴を取得できません。パネルを開き直して状態を確認してください。自動再送は行いません。');
        }
      }
      if (epoch === lifecycle.current) {
        busyRef.current = false;
        setBusy(false);
      }
    }
  }, [dispatch]);

  return {
    snapshot, loading, error, undoError, undo,
    undoUnavailableReason: (record: McpOperation) => unavailableReason(record, snapshot, bridge, busy, historyUncertain),
  };
}
