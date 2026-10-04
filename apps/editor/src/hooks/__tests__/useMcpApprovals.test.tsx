// @vitest-environment jsdom
import { StrictMode } from 'react';
import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { BRIDGE_EVENTS, type McpSettingsPayload, type UnlistenFn } from '@norves/bridge-ui';
import { useMcpApprovals } from '../useMcpApprovals.js';
import type { McpConfirmationRequest } from '../../shell/mcpApprovals.js';

const ipc = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: ipc.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: ipc.listen }));

let changed: (event: { payload: McpConfirmationRequest[] }) => void;
let unlisten: ReturnType<typeof vi.fn>;
let pending: McpConfirmationRequest[];
let settings: McpSettingsPayload;

function request(id = 'secret-confirmation-id'): McpConfirmationRequest {
  return {
    id, toolName: 'scene_delete_object', method: 'scene.deleteObject',
    targetIds: ['object-1'], targetCount: 1, before: { name: '変更前' }, after: null,
    source: 'ui', undoAvailable: false, clearsHistory: true,
    historyGeneration: 1, historyRevision: 2, undoHeadId: 3,
    expiresAt: Date.now() + 120_000,
  };
}

function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

async function mount(): Promise<ReturnType<typeof renderHook<ReturnType<typeof useMcpApprovals>, unknown>>> {
  const hook = renderHook(() => useMcpApprovals());
  await act(async () => {});
  return hook;
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2026-10-04T00:00:00Z'));
  vi.resetAllMocks();
  pending = [request()];
  settings = { enabled: true, port: 49770, state: 'running', writeMode: 'confirm' };
  unlisten = vi.fn();
  ipc.listen.mockImplementation((_name, handler) => {
    changed = handler;
    return Promise.resolve(unlisten);
  });
  ipc.invoke.mockImplementation(async (command: string) => {
    if (command === 'get_mcp_confirmations') return pending;
    if (command === 'get_mcp_settings') return settings;
    return undefined;
  });
});

afterEach(() => { cleanup(); vi.useRealTimers(); });

describe('useMcpApprovals', () => {
  it('購読確立後に初期一覧を取得し、通知と件数を保持する', async () => {
    const subscription = deferred<UnlistenFn>();
    ipc.listen.mockReturnValue(subscription.promise);
    const { result } = renderHook(() => useMcpApprovals());
    expect(ipc.invoke).not.toHaveBeenCalled();
    await act(async () => { subscription.resolve(unlisten); });
    expect(ipc.listen).toHaveBeenCalledWith(BRIDGE_EVENTS.mcpConfirmationsChanged, expect.any(Function));
    expect(result.current.requests).toEqual(pending);
    expect(result.current.mode).toBe('confirm');
    expect(result.current.notice).toContain('届いています');
    expect(result.current.loading).toBe(false);
  });

  it.each([true, false])('要求固有IDで承認/拒否を送り、連打を抑止する: %s', async (approved) => {
    const decision = deferred<void>();
    const command = approved ? 'approve_mcp_confirmation' : 'reject_mcp_confirmation';
    const { result } = await mount();
    ipc.invoke.mockImplementation((name) => name === command ? decision.promise : Promise.resolve(settings));
    act(() => {
      result.current.decide(pending[0].id, approved);
      result.current.decide(pending[0].id, approved);
    });
    expect(ipc.invoke.mock.calls.filter(([name]) => name === command)).toEqual([
      [command, { confirmationId: pending[0].id }],
    ]);
    expect(result.current.busyIds.has(pending[0].id)).toBe(true);
    await act(async () => { decision.resolve(); });
    expect(result.current.requests).toEqual([]);
    expect(result.current.busyIds.size).toBe(0);
    expect(result.current.notice).toContain(approved ? '承認しました' : '拒否しました');
  });

  it('120秒で表示と待ち件数を消し、期限切れの承認は送らない', async () => {
    const { result } = await mount();
    act(() => { vi.advanceTimersByTime(119_999); });
    expect(result.current.requests).toHaveLength(1);
    act(() => { vi.advanceTimersByTime(1); });
    expect(result.current.requests).toHaveLength(0);
    expect(result.current.notice).toContain('期限が切れました');
    act(() => { result.current.decide(pending[0].id, true); });
    expect(ipc.invoke).not.toHaveBeenCalledWith('approve_mcp_confirmation', expect.anything());
  });

  it('要求残時間が短い確認はその期限で消える', async () => {
    pending = [{ ...request(), expiresAt: Date.now() + 5_000 }];
    const { result } = await mount();
    act(() => { vi.advanceTimersByTime(5_000); });
    expect(result.current.requests).toHaveLength(0);
  });

  it('取り下げ通知が初期取得を追い越しても古い一覧を復活させない', async () => {
    const initial = deferred<McpConfirmationRequest[]>();
    ipc.invoke.mockImplementation((command) => command === 'get_mcp_confirmations'
      ? initial.promise : Promise.resolve(settings));
    const { result } = await mount();
    await act(async () => { changed({ payload: [] }); });
    await act(async () => { initial.resolve(pending); });
    expect(result.current.requests).toEqual([]);
    expect(result.current.loading).toBe(false);
  });

  it('取り下げで消し、再確認は新IDで承認する', async () => {
    const { result } = await mount();
    const oldId = pending[0].id;
    await act(async () => { changed({ payload: [] }); });
    expect(result.current.requests).toHaveLength(0);
    expect(result.current.notice).toContain('取り下げ');
    act(() => { result.current.decide(oldId, true); });
    expect(ipc.invoke).not.toHaveBeenCalledWith('approve_mcp_confirmation', expect.anything());
    const replacement = request('new-secret-id');
    await act(async () => { changed({ payload: [replacement] }); });
    await act(async () => { result.current.decide(replacement.id, true); });
    expect(ipc.invoke).toHaveBeenCalledWith('approve_mcp_confirmation', { confirmationId: replacement.id });
  });

  it('読み取りのみや許可取得失敗では承認せず、拒否は可能', async () => {
    settings.writeMode = 'readOnly';
    const { result } = await mount();
    act(() => { result.current.decide(pending[0].id, true); });
    expect(ipc.invoke).not.toHaveBeenCalledWith('approve_mcp_confirmation', expect.anything());
    ipc.invoke.mockRejectedValueOnce(new Error('settings-failed'));
    await act(async () => { window.dispatchEvent(new Event('focus')); });
    expect(result.current.mode).toBeUndefined();
    expect(result.current.error).toContain('許可を取得できません');
    act(() => { result.current.decide(pending[0].id, true); });
    await act(async () => { result.current.decide(pending[0].id, false); });
    expect(ipc.invoke).toHaveBeenCalledWith('reject_mcp_confirmation', { confirmationId: pending[0].id });
  });

  it('古い許可応答でread-onlyを上書きしない', async () => {
    const { result } = await mount();
    const old = deferred<McpSettingsPayload>();
    ipc.invoke.mockReturnValueOnce(old.promise);
    act(() => { window.dispatchEvent(new Event('focus')); });
    settings = { ...settings, writeMode: 'readOnly' };
    await act(async () => { changed({ payload: pending }); });
    await act(async () => { old.resolve({ ...settings, writeMode: 'enabled' }); });
    expect(result.current.mode).toBe('readOnly');
  });

  it('決定失敗を表示し、操作中の表示を解除する', async () => {
    const { result } = await mount();
    ipc.invoke.mockRejectedValueOnce(new Error('expired'));
    await act(async () => { result.current.decide(pending[0].id, true); });
    expect(result.current.error).toContain('応答に失敗');
    expect(result.current.busyIds.size).toBe(0);
  });

  it('承認中に新しい確認が届いても、古い決定の完了で消さない', async () => {
    const { result } = await mount();
    const decision = deferred<void>();
    ipc.invoke.mockReturnValueOnce(decision.promise);
    act(() => { result.current.decide(pending[0].id, true); });
    const replacement = request('new-secret-id');
    await act(async () => { changed({ payload: [replacement] }); });
    await act(async () => { decision.resolve(); });
    expect(result.current.requests).toEqual([replacement]);
  });

  it('待機中のアンマウントで購読と期限タイマーを解除する', async () => {
    const { result, unmount } = await mount();
    expect(vi.getTimerCount()).toBe(1);
    unmount();
    expect(unlisten).toHaveBeenCalledOnce();
    expect(vi.getTimerCount()).toBe(0);
    await act(async () => { changed({ payload: [] }); });
    expect(result.current.requests).toEqual(pending);
  });

  it('StrictModeの遅い購読を解除し、アンマウント後の初期取得を捨てる', async () => {
    const first = deferred<UnlistenFn>();
    const lateInitial = deferred<McpConfirmationRequest[]>();
    const firstDispose = vi.fn();
    ipc.listen.mockReturnValueOnce(first.promise);
    ipc.invoke.mockImplementation((name) => name === 'get_mcp_confirmations'
      ? lateInitial.promise : Promise.resolve(settings));
    const { result, unmount } = renderHook(() => useMcpApprovals(), { wrapper: StrictMode });
    await act(async () => { first.resolve(firstDispose); });
    expect(firstDispose).toHaveBeenCalledOnce();
    unmount();
    expect(unlisten).toHaveBeenCalledOnce();
    await act(async () => { lateInitial.resolve(pending); changed({ payload: pending }); });
    expect(result.current.requests).toEqual([]);
    expect(vi.getTimerCount()).toBe(0);
  });

  it('購読失敗時は初期取得や承認を始めない', async () => {
    ipc.listen.mockRejectedValueOnce(new Error('listen-failed'));
    const { result } = await mount();
    expect(result.current.error).toContain('確認待ちを取得できません');
    expect(result.current.mode).toBeUndefined();
    expect(ipc.invoke).not.toHaveBeenCalled();
  });
});
