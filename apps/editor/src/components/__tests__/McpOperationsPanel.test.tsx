// @vitest-environment jsdom
import React, { useEffect, useMemo, useState } from 'react';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { DockviewApi, DockviewReadyEvent, IDockviewPanelProps } from 'dockview-react';
import { BRIDGE_EVENTS, type EditHistorySummary, type McpConfirmationRequest, type McpOperation, type McpOperationsPayload, type McpSettingsPayload } from '@norves/bridge-ui';
import { INITIAL_STATE, type BridgeState } from '../../state/store.js';
import { AppLayout } from '../AppLayout.js';

const ipc = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(), dispatch: vi.fn(), activate: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: ipc.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: ipc.listen }));
vi.mock('../../state/BridgeContext.js', () => ({
  useBridgeState: () => bridge,
  useBridgeDispatch: () => ipc.dispatch,
}));

// パネルの登録・再表示と実際の購読を検査し、配置APIだけを置き換える。
vi.mock('dockview-react', () => ({
  DockviewReact: ({ components, onReady }: {
    components: Record<string, React.FunctionComponent<IDockviewPanelProps>>;
    onReady: (event: DockviewReadyEvent) => void;
  }) => {
    const [ids, setIds] = useState<string[]>([]);
    const api = useMemo(() => {
      const panels = new Map<string, { id: string; api: { setActive: () => void } }>();
      return {
        width: 1200,
        getPanel: (id: string) => panels.get(id),
        addPanel: ({ id }: { id: string }) => {
          const panel = { id, api: { setActive: ipc.activate } };
          panels.set(id, panel); setIds([...panels.keys()]); return panel;
        },
        removePanel: (id: string) => { panels.delete(id); setIds([...panels.keys()]); },
        getEdgeGroup: () => undefined,
        addEdgeGroup: () => ({ id: 'logEdgeGroup' }),
        onDidLayoutChange: () => ({ dispose: () => {} }),
      };
    }, []);
    useEffect(() => { onReady({ api: api as unknown as DockviewApi }); }, [api, onReady]);
    return <div>{ids.filter((id) => id === 'mcpOperations' || id === 'mcpApprovals').map((id) => {
      const Panel = components[id];
      return <div key={id}>
        <button onClick={() => api.removePanel(id)}>パネルを閉じる</button>
        <Panel {...({} as IDockviewPanelProps)} />
      </div>;
    })}</div>;
  },
}));

let bridge: BridgeState;
let history: EditHistorySummary;
let snapshot: McpOperationsPayload;
let settings: McpSettingsPayload;
let pending: McpConfirmationRequest[];
const listeners = new Map<string, (event: { payload: unknown }) => void>();
const disposers = new Map<string, ReturnType<typeof vi.fn>>();
function record(overrides: Partial<McpOperation> = {}): McpOperation {
  return {
    requestId: 'run-1:1', sessionId: 'run-1', timestamp: 1_791_072_000_000,
    tool: 'object_set_property', target: 'object:abcdef', summary: '値を設定',
    result: 'success', outcome: 'applied', displayGroupId: 'edit-1-7',
    completedCount: 1, pending: false, retryAllowed: false,
    automaticRetryAllowed: false, actorFinished: true, ...overrides,
  };
}
async function emit(name: string, payload: unknown) {
  await act(async () => { listeners.get(name)?.({ payload }); });
}
async function openPanel() {
  fireEvent.click(screen.getByRole('button', { name: 'AI の操作' }));
  await screen.findByText('run-1:1');
}
beforeEach(() => {
  vi.resetAllMocks(); listeners.clear(); disposers.clear();
  vi.stubGlobal('localStorage', { getItem: () => null, removeItem: () => {} });
  history = {
    generation: 1, historyRevision: 4, appliedRevision: 4, canUndo: true, canRedo: false,
    undoHeadId: 7, undoRevision: 3,
    undoGroup: { id: 'edit-1-7', name: '値を設定', source: 'mcp', count: 1, createdAt: 1000 },
    redoHeadId: null, redoRevision: 0, redoGroup: null, pending: false,
  };
  bridge = { ...INITIAL_STATE, connection: { status: 'connected' }, editHistorySummary: history };
  snapshot = { sessionId: 'run-1', revision: 1, records: [record()], storageError: null };
  settings = { enabled: true, port: 49770, state: 'running', writeMode: 'confirm' };
  pending = [{
    id: 'secret-confirmation', toolName: 'scene_delete_object', method: 'scene.deleteObject',
    targetIds: ['object-1'], targetCount: 1, before: null, after: null,
    source: 'mcp', undoAvailable: false, clearsHistory: true,
    historyGeneration: 1, historyRevision: 4, undoHeadId: 7, expiresAt: Date.now() + 120_000,
  }];
  ipc.listen.mockImplementation((name, handler) => {
    listeners.set(name, handler);
    const dispose = vi.fn(); disposers.set(name, dispose); return Promise.resolve(dispose);
  });
  ipc.invoke.mockImplementation(async (name: string) => {
    if (name === 'get_mcp_operations') return snapshot;
    if (name === 'get_mcp_confirmations') return pending;
    if (name === 'get_mcp_settings') return settings;
    if (name === 'edit_get_history') return history;
    return undefined;
  });
  ipc.dispatch.mockImplementation((action) => { bridge = { ...bridge, editHistorySummary: action.summary }; });
});
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

describe('AIの操作パネル', () => {
  it('dockviewへ一つだけ開き、各項目と確認待ちを表示して閉じると購読を解除する', async () => {
    const { container } = render(<AppLayout />);
    await openPanel();
    expect(screen.getByRole('region', { name: 'AIの操作' })).toBeTruthy();
    const entry = within(screen.getByRole('listitem'));
    for (const value of ['道具', '時刻', '対象', '要約', '結果', 'まとまりID', '要求ID', 'object_set_property', 'object:abcdef', '値を設定', '成功 / 適用済み', 'edit-1-7']) {
      expect(entry.getByText(value)).toBeTruthy();
    }
    expect(container.querySelector('time')?.getAttribute('datetime')).toBe(new Date(snapshot.records[0].timestamp).toISOString());
    expect(screen.getByRole('region', { name: 'MCPの書き込み確認' })).toBeTruthy();
    expect(screen.getByRole('article').textContent).toContain('scene_delete_object');
    expect(container.innerHTML).not.toContain('secret-confirmation');
    fireEvent.click(screen.getByRole('button', { name: 'AI の操作' }));
    expect(ipc.activate).toHaveBeenCalledOnce();
    expect(screen.getAllByRole('region', { name: 'AIの操作' })).toHaveLength(1);
    fireEvent.click(screen.getByRole('button', { name: 'パネルを閉じる' }));
    expect(disposers.get(BRIDGE_EVENTS.mcpOperationsChanged)).toHaveBeenCalledOnce();
    expect(disposers.get(BRIDGE_EVENTS.mcpConfirmationsChanged)).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: 'MCP 確認待ち（1件）' })).toBeTruthy();
    await openPanel();
    expect(ipc.invoke.mock.calls.filter(([name]) => name === 'get_mcp_operations')).toHaveLength(2);
  });

  it('履歴リンクから先頭のID・改訂を送り、古いまとまりは理由付きで無効にする', async () => {
    snapshot.records.unshift(record({ requestId: 'run-1:0', displayGroupId: 'edit-1-6' }));
    render(<AppLayout />);
    await openPanel();
    const entries = screen.getAllByRole('listitem');
    expect(entries[0].textContent).toContain('run-1:1');
    const head = within(entries[0]).getByRole('button', { name: 'まとまりを取り消す' });
    const old = within(entries[1]).getByRole('button', { name: 'まとまりを取り消す' });
    expect((old as HTMLButtonElement).disabled).toBe(true);
    expect(entries[1].textContent).toContain('後の編集から順に');
    fireEvent.click(old);
    expect(ipc.invoke).not.toHaveBeenCalledWith('edit_undo', expect.anything());
    fireEvent.click(head);
    await waitFor(() => expect(ipc.invoke).toHaveBeenCalledWith('edit_undo', { expectedHeadId: 7, expectedRevision: 3 }));
    await waitFor(() => expect(ipc.dispatch).toHaveBeenCalled());
  });

  it('拒否・部分失敗・時間切れ後の適用・結果不明を区別して表示する', async () => {
    render(<AppLayout />);
    await openPanel();
    history = { ...history, pending: true };
    bridge = { ...bridge, editHistorySummary: history };
    await emit(BRIDGE_EVENTS.mcpOperationsChanged, { ...snapshot, revision: 2, records: [
      record({ result: 'rejected', outcome: 'notApplied', completedCount: 0, pending: true, retryAllowed: true }),
      record({ requestId: 'run-1:2', result: 'partial', outcome: 'partial', pending: true, retryAllowed: true }),
      record({ requestId: 'run-1:3', result: 'timedOut', outcome: 'applied' }),
      record({ requestId: 'run-1:4', result: 'unknown', outcome: 'unknown', actorFinished: false }),
    ] });
    expect(screen.getByText('拒否 / 未送信・未適用')).toBeTruthy();
    expect(screen.getByText('部分成功 / 一部適用')).toBeTruthy();
    expect(screen.getByText('時間切れ / 適用済み')).toBeTruthy();
    expect(screen.getByText('結果不明 / 適用結果不明')).toBeTruthy();
    expect(screen.getAllByText(/自動再送は行いません/).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/画面上部で状態確認/).length).toBeGreaterThan(0);
    expect(screen.getAllByRole('button', { name: 'まとまりを取り消す' }).every((button) => (button as HTMLButtonElement).disabled)).toBe(true);
  });

  it('undo拒否と部分失敗の保留を表示し、通常undoを再送しない', async () => {
    render(<AppLayout />);
    await openPanel();
    history = { ...history, pending: true };
    ipc.invoke.mockRejectedValueOnce(new Error('PARTIAL_FAILURE'));
    fireEvent.click(screen.getByRole('button', { name: 'まとまりを取り消す' }));
    await screen.findByText(/取り消しに失敗しました/);
    await waitFor(() => expect(screen.getByText(/履歴の処理が保留中/)).toBeTruthy());
    expect((screen.getByRole('button', { name: 'まとまりを取り消す' }) as HTMLButtonElement).disabled).toBe(true);
    expect(ipc.invoke.mock.calls.filter(([name]) => name === 'edit_undo')).toHaveLength(1);
  });

  it('保存失敗を表示し、通知で復旧したら消す。要約はHTMLとして解釈しない', async () => {
    snapshot.storageError = '記録ファイルに書き込めません。';
    snapshot.records[0].summary = '<script>操作を実行</script>';
    const { container } = render(<AppLayout />);
    await openPanel();
    expect(screen.getByRole('alert').textContent).toContain('ファイル保存に失敗');
    expect(screen.getByText('<script>操作を実行</script>')).toBeTruthy();
    expect(container.querySelector('script')).toBeNull();
    await emit(BRIDGE_EVENTS.mcpOperationsChanged, { ...snapshot, revision: 2, storageError: null });
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('初期取得失敗を表示し、その後の空の通知も表示する', async () => {
    render(<AppLayout />);
    await screen.findByRole('button', { name: 'MCP 確認待ち（1件）' });
    ipc.invoke.mockRejectedValueOnce(new Error('initial'));
    fireEvent.click(screen.getByRole('button', { name: 'AI の操作' }));
    await screen.findByText(/操作記録を取得できません/);
    await emit(BRIDGE_EVENTS.mcpOperationsChanged, { ...snapshot, records: [] });
    expect(screen.queryByRole('alert')).toBeNull();
    expect(screen.getByText('操作記録はありません。')).toBeTruthy();
  });

  it('操作パネルの確認待ちから拒否できる', async () => {
    render(<AppLayout />);
    await openPanel();
    fireEvent.click(screen.getByRole('button', { name: '拒否' }));
    await waitFor(() => expect(screen.queryByRole('article')).toBeNull());
    expect(ipc.invoke).toHaveBeenCalledWith('reject_mcp_confirmation', { confirmationId: 'secret-confirmation' });
    expect(screen.getByRole('status').textContent).toContain('拒否しました');
  });

  it('許可エラーで新しい確認通知を隠さず、再取得成功後はエラーだけ消す', async () => {
    render(<AppLayout />);
    await openPanel();
    ipc.invoke.mockRejectedValueOnce(new Error('settings'));
    await act(async () => { window.dispatchEvent(new Event('focus')); });
    expect(screen.queryByRole('button', { name: '今回だけ承認' })).toBeNull();
    expect(screen.getAllByRole('alert')[0].textContent).toContain('許可を取得できません');
    let resolve!: (value: McpSettingsPayload) => void;
    ipc.invoke.mockReturnValueOnce(new Promise<McpSettingsPayload>((done) => { resolve = done; }));
    await emit(BRIDGE_EVENTS.mcpConfirmationsChanged, [{ ...pending[0], id: 'new-secret' }]);
    expect(screen.getByRole('status').textContent).toContain('確認が届いています');
    expect(screen.queryByRole('button', { name: '今回だけ承認' })).toBeNull();
    await act(async () => { resolve(settings); });
    expect(screen.queryByRole('alert')).toBeNull();
    expect(screen.getByRole('button', { name: '今回だけ承認' })).toBeTruthy();
    expect(screen.getByRole('status').textContent).toContain('確認が届いています');
  });
});
