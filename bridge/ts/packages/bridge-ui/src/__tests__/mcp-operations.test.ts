import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  approveMcpConfirmation,
  BRIDGE_EVENTS,
  getMcpConfirmations,
  getMcpOperations,
  rejectMcpConfirmation,
  subscribeEvent,
  type McpOperationsPayload,
} from '../index.js';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

describe('MCP操作記録と確認の公開入口', () => {
  beforeEach(() => vi.clearAllMocks());

  it('保存失敗を含む初期取得と通知を同じ型で受け、購読を解除する', async () => {
    const payload: McpOperationsPayload = {
      sessionId: 'session-1', revision: 2, records: [], storageError: '保存できません。',
    };
    vi.mocked(invoke).mockResolvedValue(payload);
    expect(await getMcpOperations()).toEqual(payload);
    expect(invoke).toHaveBeenCalledWith('get_mcp_operations');
    const stop = vi.fn();
    vi.mocked(listen).mockResolvedValue(stop);
    const received = vi.fn();
    const unlisten = await subscribeEvent<McpOperationsPayload>(BRIDGE_EVENTS.mcpOperationsChanged, received);
    const [name, callback] = vi.mocked(listen).mock.calls[0]!;
    expect(name).toBe('bridge:mcp-operations-changed');
    callback({ event: name, id: 1, payload });
    expect(received).toHaveBeenCalledWith(payload);
    unlisten();
    expect(stop).toHaveBeenCalledOnce();
  });

  it('確認取得と承認・拒否を既存commandへ渡す', async () => {
    await getMcpConfirmations();
    await approveMcpConfirmation('one-use-id');
    await rejectMcpConfirmation('one-use-id');
    expect(invoke).toHaveBeenNthCalledWith(1, 'get_mcp_confirmations');
    expect(invoke).toHaveBeenNthCalledWith(2, 'approve_mcp_confirmation', { confirmationId: 'one-use-id' });
    expect(invoke).toHaveBeenNthCalledWith(3, 'reject_mcp_confirmation', { confirmationId: 'one-use-id' });
  });
});
