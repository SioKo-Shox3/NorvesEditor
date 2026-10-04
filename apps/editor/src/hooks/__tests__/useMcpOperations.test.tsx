// @vitest-environment jsdom
import { StrictMode } from 'react';
import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { BRIDGE_EVENTS, type EditHistorySummary, type McpOperation, type McpOperationsPayload, type UnlistenFn } from '@norves/bridge-ui';
import { INITIAL_STATE, type BridgeState } from '../../state/store.js';
import { useMcpOperations } from '../useMcpOperations.js';

const ipc = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(), dispatch: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: ipc.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: ipc.listen }));
vi.mock('../../state/BridgeContext.js', () => ({
  useBridgeState: () => bridge,
  useBridgeDispatch: () => ipc.dispatch,
}));
let bridge: BridgeState;
let changed: (event: { payload: McpOperationsPayload }) => void;
let snapshot: McpOperationsPayload;
let history: EditHistorySummary;
let unlisten: ReturnType<typeof vi.fn>;

function record(overrides: Partial<McpOperation> = {}): McpOperation {
  return {
    requestId: 'run-1:1', sessionId: 'run-1', timestamp: 1000,
    tool: 'object_set_property', target: 'object:abcdef', summary: '値を設定',
    result: 'success', outcome: 'applied', displayGroupId: 'edit-1-7',
    completedCount: 1, pending: false, retryAllowed: false,
    automaticRetryAllowed: false, actorFinished: true, ...overrides,
  };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
async function mount() {
  const hook = renderHook(() => useMcpOperations());
  await act(async () => {});
  return hook;
}
beforeEach(() => {
  vi.resetAllMocks();
  snapshot = { sessionId: 'run-1', revision: 1, records: [record()], storageError: null };
  history = {
    generation: 1, historyRevision: 4, appliedRevision: 4,
    canUndo: true, canRedo: false, undoHeadId: 7, undoRevision: 3,
    undoGroup: { id: 'edit-1-7', name: '値を設定', source: 'mcp', count: 1, createdAt: 1000 },
    redoHeadId: null, redoRevision: 0, redoGroup: null, pending: false,
  };
  bridge = { ...INITIAL_STATE, connection: { status: 'connected' }, editHistorySummary: history };
  unlisten = vi.fn();
  ipc.listen.mockImplementation((_name, handler) => { changed = handler; return Promise.resolve(unlisten); });
  ipc.invoke.mockImplementation(async (name: string) => {
    if (name === 'get_mcp_operations') return snapshot;
    if (name === 'edit_get_history') return history;
    return undefined;
  });
  ipc.dispatch.mockImplementation((action) => { bridge = { ...bridge, editHistorySummary: action.summary }; });
});
afterEach(cleanup);

describe('useMcpOperations', () => {
  it('購読確立後に初期取得する', async () => {
    const subscription = deferred<UnlistenFn>();
    ipc.listen.mockReturnValue(subscription.promise);
    const { result } = renderHook(() => useMcpOperations());
    expect(ipc.invoke).not.toHaveBeenCalled();
    expect(result.current.loading).toBe(true);
    await act(async () => { subscription.resolve(unlisten); });
    expect(ipc.listen).toHaveBeenCalledWith(BRIDGE_EVENTS.mcpOperationsChanged, expect.any(Function));
    expect(ipc.invoke).toHaveBeenCalledWith('get_mcp_operations');
    expect(result.current.snapshot).toEqual(snapshot);
    expect(result.current.loading).toBe(false);
  });

  it('初期応答と古い通知で新しい改訂を巻き戻さず、同じ要求の確定結果を置き換える', async () => {
    const initial = deferred<McpOperationsPayload>();
    ipc.invoke.mockReturnValueOnce(initial.promise);
    const { result } = await mount();
    const pending = { ...snapshot, revision: 2, records: [record({ actorFinished: false, outcome: 'unknown' })] };
    await act(async () => { changed({ payload: pending }); initial.resolve(snapshot); });
    expect(result.current.snapshot).toEqual(pending);
    const finished = { ...snapshot, revision: 3 };
    await act(async () => { changed({ payload: finished }); changed({ payload: pending }); });
    expect(result.current.snapshot).toEqual(finished);
    expect(result.current.snapshot?.records).toHaveLength(1);
  });

  it('起動が変わった通知には低い改訂を採用し、旧起動の初期応答と遅延通知を捨てる', async () => {
    const initial = deferred<McpOperationsPayload>();
    ipc.invoke.mockReturnValueOnce(initial.promise);
    const { result } = await mount();
    await act(async () => { changed({ payload: { ...snapshot, revision: 9 } }); });
    const next = { ...snapshot, sessionId: 'run-2', revision: 0, records: [] };
    await act(async () => { changed({ payload: next }); initial.resolve(snapshot); changed({ payload: snapshot }); });
    expect(result.current.snapshot).toEqual(next);
  });

  it('ファイル失敗と復旧を通知から反映する', async () => {
    const { result } = await mount();
    await act(async () => { changed({ payload: { ...snapshot, revision: 2, storageError: '保存先に書き込めません。' } }); });
    expect(result.current.snapshot?.storageError).toContain('書き込めません');
    await act(async () => { changed({ payload: { ...snapshot, revision: 3 } }); });
    expect(result.current.snapshot?.storageError).toBeNull();
  });

  it('初期取得失敗を通知の成功で解消する', async () => {
    ipc.invoke.mockRejectedValueOnce(new Error('unavailable'));
    const { result } = await mount();
    expect(result.current.error).toContain('取得できません');
    expect(result.current.loading).toBe(false);
    await act(async () => { changed({ payload: snapshot }); });
    expect(result.current.error).toBeUndefined();
    expect(result.current.snapshot).toEqual(snapshot);
  });

  it('遅い初期取得の失敗で受信済みの記録にエラーを付けない', async () => {
    const initial = deferred<McpOperationsPayload>();
    ipc.invoke.mockReturnValueOnce(initial.promise);
    const { result } = await mount();
    await act(async () => { changed({ payload: snapshot }); initial.reject(new Error('late')); });
    expect(result.current.error).toBeUndefined();
    expect(result.current.snapshot).toEqual(snapshot);
  });

  it('購読失敗では取得を始めず、エラーを返す', async () => {
    ipc.listen.mockRejectedValueOnce(new Error('listen'));
    const { result } = await mount();
    expect(result.current.error).toContain('通知を受信できません');
    expect(ipc.invoke).not.toHaveBeenCalled();
  });

  it('先頭まとまりだけをIDと改訂付きで共通undoへ送り、連打を抑止する', async () => {
    const { result } = await mount();
    const response = deferred<void>();
    ipc.invoke.mockReturnValueOnce(response.promise);
    act(() => { void result.current.undo('run-1:1'); void result.current.undo('run-1:1'); });
    expect(ipc.invoke.mock.calls.filter(([name]) => name === 'edit_undo')).toEqual([
      ['edit_undo', { expectedHeadId: 7, expectedRevision: 3 }],
    ]);
    expect(result.current.undoUnavailableReason(record())).toContain('処理中');
    history = { ...history, canUndo: false, undoHeadId: null, undoGroup: null };
    await act(async () => { response.resolve(); });
    expect(ipc.dispatch).toHaveBeenCalledWith({ type: 'editHistorySummaryReceived', summary: history });
    expect(result.current.undoUnavailableReason(record())).toContain('現在の取り消し対象ではありません');
  });

  it('別の先頭へ変わったら古いリンクからundoを送らない', async () => {
    const { result, rerender } = await mount();
    bridge = { ...bridge, editHistorySummary: { ...history, undoGroup: { ...history.undoGroup!, id: 'edit-1-8', source: 'ui' } } };
    rerender();
    expect(result.current.undoUnavailableReason(record())).toContain('後の編集から順に');
    await act(async () => { await result.current.undo('run-1:1'); });
    expect(ipc.invoke).not.toHaveBeenCalledWith('edit_undo', expect.anything());
  });

  it('列で改訂不一致を拒否されても対象を変えて再送しない', async () => {
    const { result } = await mount();
    ipc.invoke.mockRejectedValueOnce(new Error('STALE_HISTORY'));
    await act(async () => { await result.current.undo('run-1:1'); });
    expect(result.current.undoError).toContain('自動再送は行いません');
    expect(ipc.invoke.mock.calls.filter(([name]) => name === 'edit_undo')).toHaveLength(1);
    expect(ipc.invoke).toHaveBeenLastCalledWith('edit_get_history');
  });

  it('部分失敗で履歴が保留になったら取り消しを止める', async () => {
    const { result } = await mount();
    history = { ...history, pending: true, canUndo: false, pendingGroup: {
      id: 'edit-1-7', name: '値を設定', source: 'mcp', createdAt: 1000,
      direction: 'undo', totalCount: 2, completedCount: 1, outcomeUnknown: false, retryAllowed: true,
    } };
    ipc.invoke.mockRejectedValueOnce(new Error('PARTIAL_FAILURE'));
    await act(async () => { await result.current.undo('run-1:1'); });
    expect(result.current.undoUnavailableReason(record())).toContain('保留中');
    await act(async () => { await result.current.undo('run-1:1'); });
    expect(ipc.invoke.mock.calls.filter(([name]) => name === 'edit_undo')).toHaveLength(1);
  });

  it.each(['disconnected', 'unsupported', 'noHistory', 'noUndo', 'noHead'] as const)('利用不能時は理由を表示して送らない: %s', async (kind) => {
    if (kind === 'disconnected') bridge.connection = { status: 'disconnected' };
    if (kind === 'unsupported') bridge.sceneEditUnsupported = true;
    if (kind === 'noHistory') bridge.editHistorySummary = undefined;
    if (kind === 'noUndo') history.canUndo = false;
    if (kind === 'noHead') history.undoHeadId = null;
    const { result } = await mount();
    expect(result.current.undoUnavailableReason(record())).toBeTruthy();
    await act(async () => { await result.current.undo('run-1:1'); });
    expect(ipc.invoke).not.toHaveBeenCalledWith('edit_undo', expect.anything());
  });

  it.each([
    { displayGroupId: null }, { sessionId: 'old-run' }, { actorFinished: false },
  ])('まとまりなし・別起動・未確定の記録を拒否する: %s', async (override) => {
    snapshot.records = [record(override)];
    const { result } = await mount();
    expect(result.current.undoUnavailableReason(snapshot.records[0])).toBeTruthy();
    await act(async () => { await result.current.undo('run-1:1'); });
    expect(ipc.invoke).not.toHaveBeenCalledWith('edit_undo', expect.anything());
  });

  it('undo後の履歴取得失敗は再送を抑止する', async () => {
    const { result } = await mount();
    ipc.invoke.mockResolvedValueOnce(undefined).mockRejectedValueOnce(new Error('history'));
    await act(async () => { await result.current.undo('run-1:1'); });
    expect(result.current.undoError).toContain('履歴を取得できません');
    expect(result.current.undoUnavailableReason(record())).toContain('履歴を取得できません');
    await act(async () => { await result.current.undo('run-1:1'); });
    expect(ipc.invoke.mock.calls.filter(([name]) => name === 'edit_undo')).toHaveLength(1);
  });

  it('StrictModeの遅い購読を解除し、終了後の初期取得と通知を捨てる', async () => {
    const subscription = deferred<UnlistenFn>();
    const initial = deferred<McpOperationsPayload>();
    const firstDispose = vi.fn();
    ipc.listen.mockReturnValueOnce(subscription.promise);
    ipc.invoke.mockReturnValueOnce(initial.promise);
    const { result, unmount } = renderHook(() => useMcpOperations(), { wrapper: StrictMode });
    await act(async () => { subscription.resolve(firstDispose); });
    expect(firstDispose).toHaveBeenCalledOnce();
    unmount();
    expect(unlisten).toHaveBeenCalledOnce();
    await act(async () => { initial.resolve(snapshot); changed({ payload: snapshot }); });
    expect(result.current.snapshot).toBeUndefined();
  });

  it('undo中の終了後は履歴応答を画面へ適用しない', async () => {
    const { result, unmount } = await mount();
    const response = deferred<void>();
    ipc.invoke.mockReturnValueOnce(response.promise);
    act(() => { void result.current.undo('run-1:1'); });
    unmount();
    await act(async () => { response.resolve(); });
    expect(ipc.dispatch).not.toHaveBeenCalled();
  });
});
