// @vitest-environment jsdom
import React, { useEffect, useMemo, useState } from 'react';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { DockviewApi, DockviewReadyEvent, IDockviewPanelProps } from 'dockview-react';
import type { McpSettingsPayload } from '@norves/bridge-ui';
import type { McpConfirmationRequest } from '../../shell/mcpApprovals.js';
import { INITIAL_STATE } from '../../state/store.js';
import { AppLayout } from '../AppLayout.js';

const ipc = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
const edit = vi.hoisted(() => ({
  getObjectSnapshot: vi.fn(), getSchemaSnapshot: vi.fn(), setObjectProperty: vi.fn(),
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: ipc.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: ipc.listen }));
vi.mock('../../hooks/useBridge.js', () => ({ useBridgeActions: () => edit }));
vi.mock('../../state/BridgeContext.js', () => ({
  useBridgeState: () => ({
    ...INITIAL_STATE,
    connection: { status: 'connected' },
    selectedObjectId: 'object-1',
    objectSnapshot: {
      objectId: 'object-1', name: '対象', kind: 'object',
      properties: [{ name: 'label', value: '変更前', valueType: 'string' }],
    },
  }),
  useBridgeDispatch: () => vi.fn(),
}));

// mainの実配線とContextを保ったまま、dockviewの配置操作だけを置き換える。
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
          const panel = { id, api: { setActive: () => {} } };
          panels.set(id, panel);
          setIds([...panels.keys()]);
          return panel;
        },
        removePanel: ({ id }: { id: string }) => { panels.delete(id); setIds([...panels.keys()]); },
        getEdgeGroup: () => undefined,
        addEdgeGroup: () => ({ id: 'logEdgeGroup' }),
        onDidLayoutChange: () => ({ dispose: () => {} }),
      };
    }, []);
    useEffect(() => { onReady({ api: api as unknown as DockviewApi }); }, [api, onReady]);
    return <div>
      {ids.filter((id) => id === 'mcpApprovals' || id === 'propertyInspector').map((id) => {
        const Panel = components[id];
        return <div key={id}>
          {id === 'mcpApprovals' && <button onClick={() => api.removePanel({ id })}>確認パネルを閉じる</button>}
          <Panel {...({} as IDockviewPanelProps)} />
        </div>;
      })}
    </div>;
  },
}));

let changed: (event: { payload: McpConfirmationRequest[] }) => void;
let settings: McpSettingsPayload;
let pending: McpConfirmationRequest[];
const unlisten = vi.fn();

beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal('localStorage', { getItem: () => null, removeItem: () => {} });
  settings = { enabled: true, port: 49770, state: 'running', writeMode: 'enabled' };
  pending = [{
    id: 'secret-request-id', toolName: 'scene_delete_object', method: 'scene.deleteObject',
    targetIds: ['object-1'], targetCount: 3,
    before: { name: '<b>削除前</b>' }, after: { deleted: true },
    source: 'ui', undoAvailable: false, clearsHistory: true,
    historyGeneration: 1, historyRevision: 2, undoHeadId: 3, expiresAt: Date.now() + 120_000,
  }];
  ipc.listen.mockImplementation((_name, handler) => { changed = handler; return Promise.resolve(unlisten); });
  ipc.invoke.mockImplementation(async (name: string) => {
    if (name === 'get_mcp_confirmations') return pending;
    if (name === 'get_mcp_settings') return settings;
    return undefined;
  });
  edit.setObjectProperty.mockResolvedValue({ accepted: true, appliedValue: '変更後' });
});

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

async function openPanel(): Promise<void> {
  await screen.findByRole('button', { name: 'MCP 確認待ち（1件）' });
  fireEvent.click(screen.getByRole('button', { name: 'MCP 確認待ち（1件）' }));
}

describe('MCP確認パネルとmain画面', () => {
  it('閉じた状態で件数と通知を出し、クリックでdockviewへ開き、再開で重複しない', async () => {
    render(<AppLayout />);
    await screen.findByRole('button', { name: 'MCP 確認待ち（1件）' });
    expect(screen.queryByRole('region', { name: 'MCPの書き込み確認' })).toBeNull();
    expect(screen.getByRole('status').textContent).toContain('確認が届いています');
    await openPanel();
    fireEvent.click(screen.getByRole('button', { name: 'MCP 確認待ち（1件）' }));
    expect(screen.getAllByRole('region', { name: 'MCPの書き込み確認' })).toHaveLength(1);
    fireEvent.click(screen.getByRole('button', { name: '確認パネルを閉じる' }));
    expect(screen.queryByRole('region', { name: 'MCPの書き込み確認' })).toBeNull();
    expect(screen.getByRole('button', { name: 'MCP 確認待ち（1件）' })).toBeTruthy();
    expect(unlisten).not.toHaveBeenCalled();
    await act(async () => { changed({ payload: [] }); });
    expect(screen.getByRole('button', { name: 'MCP 確認待ち（0件）' })).toBeTruthy();
    await act(async () => { changed({ payload: pending }); });
    await openPanel();
    expect(screen.getAllByRole('region', { name: 'MCPの書き込み確認' })).toHaveLength(1);
    expect(ipc.listen).toHaveBeenCalledOnce();
  });

  it('対象の省略、前後、履歴の出どころ、取り消し不可、全履歴破棄を表示する', async () => {
    const { container } = render(<AppLayout />);
    await openPanel();
    expect(within(screen.getByRole('article')).getByText('object-1')).toBeTruthy();
    expect(screen.getByText('3件')).toBeTruthy();
    expect(screen.getByText('対象IDは一部を表示しています。')).toBeTruthy();
    expect(screen.getByText(/"name": "<b>削除前<\/b>"/)).toBeTruthy();
    expect(container.querySelector('b')).toBeNull();
    expect(screen.getByText(/"deleted": true/)).toBeTruthy();
    expect(screen.getByText('人の編集')).toBeTruthy();
    expect(screen.getByText('取り消せません')).toBeTruthy();
    expect(screen.getByRole('note').textContent).toContain('人の編集を含む取り消し・やり直しの全履歴が破棄');
    expect(screen.getByText(/最大120秒/)).toBeTruthy();
    expect(container.innerHTML).not.toContain(pending[0].id);
  });

  it.each([true, false])('ボタンから今回の要求だけを決定する: %s', async (approved) => {
    render(<AppLayout />);
    await openPanel();
    fireEvent.click(screen.getByRole('button', { name: approved ? '今回だけ承認' : '拒否' }));
    await waitFor(() => expect(screen.queryByRole('article')).toBeNull());
    expect(ipc.invoke).toHaveBeenCalledWith(approved ? 'approve_mcp_confirmation' : 'reject_mcp_confirmation', {
      confirmationId: pending[0].id,
    });
  });

  it('読み取りのみへの変更で承認ボタンを消し、取り下げ通知で一覧を消す', async () => {
    render(<AppLayout />);
    await openPanel();
    expect(screen.getByRole('button', { name: '今回だけ承認' })).toBeTruthy();
    settings = { ...settings, writeMode: 'readOnly' };
    await act(async () => { window.dispatchEvent(new Event('focus')); });
    expect(screen.queryByRole('button', { name: '今回だけ承認' })).toBeNull();
    expect(screen.getByRole('button', { name: '拒否' })).toBeTruthy();
    await act(async () => { changed({ payload: [] }); });
    expect(screen.queryByRole('article')).toBeNull();
    expect(screen.getByText('確認待ちはありません。')).toBeTruthy();
  });

  it('初期設定が読み取りのみなら確認が残っていても承認ボタンを出さない', async () => {
    settings = { ...settings, writeMode: 'readOnly' };
    render(<AppLayout />);
    await openPanel();
    expect(screen.queryByRole('button', { name: '今回だけ承認' })).toBeNull();
    expect(screen.getByRole('button', { name: '拒否' })).toBeTruthy();
  });

  it('確認待ちでも実際のInspectorから値の編集を送れる', async () => {
    render(<AppLayout />);
    await openPanel();
    const input = screen.getByDisplayValue('変更前');
    expect((input as HTMLInputElement).disabled).toBe(false);
    fireEvent.change(input, { target: { value: '変更後' } });
    fireEvent.blur(input);
    await waitFor(() => expect(edit.setObjectProperty).toHaveBeenCalledWith('object-1', 'label', '変更後'));
    expect(screen.getByRole('article')).toBeTruthy();
    expect(ipc.invoke).not.toHaveBeenCalledWith('approve_mcp_confirmation', expect.anything());
  });

  it('取り消せる操作は可能と表示し、履歴破棄の警告を付けない', async () => {
    pending = [{ ...pending[0], undoAvailable: true, clearsHistory: false, source: 'mcp' }];
    render(<AppLayout />);
    await openPanel();
    expect(screen.getByText('履歴から取り消し可能')).toBeTruthy();
    expect(screen.queryByRole('note')).toBeNull();
  });
});
