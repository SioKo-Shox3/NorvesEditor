// @vitest-environment jsdom

import { StrictMode } from 'react';
import { act, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { McpSettingsPayload } from '@norves/bridge-ui';
import { useMcpSettings } from '../useMcpSettings.js';

vi.mock('@norves/bridge-ui', () => ({
  getMcpSettings: vi.fn(),
  getMcpToken: vi.fn(),
  regenerateMcpToken: vi.fn(),
  setMcpSettings: vi.fn(),
  setMcpWriteAccess: vi.fn(),
}));

const commands = await import('@norves/bridge-ui');

const DISABLED: McpSettingsPayload = {
  enabled: false,
  port: 49770,
  state: 'disabled',
};

const RUNNING: McpSettingsPayload = {
  enabled: true,
  port: 49771,
  state: 'running',
  endpoint: 'http://127.0.0.1:49771/mcp',
};

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
}

function deferred<T>(): Deferred<T> {
  let resolve: (value: T) => void = () => undefined;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(commands.getMcpSettings).mockResolvedValue(DISABLED);
  vi.mocked(commands.getMcpToken).mockResolvedValue({ token: 'fixture-token' });
  vi.mocked(commands.setMcpSettings).mockResolvedValue(RUNNING);
  vi.mocked(commands.regenerateMcpToken).mockResolvedValue(DISABLED);
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('useMcpSettings', () => {
  it('許可変更の連打を抑止し、保存応答のモードと部分木を反映する', async () => {
    const pending = deferred<McpSettingsPayload>();
    vi.mocked(commands.setMcpWriteAccess).mockReturnValue(pending.promise);
    const { result } = renderHook(() => useMcpSettings());
    await waitFor(() => expect(result.current.busy).toBe(false));
    act(() => {
      result.current.setWriteModeDraft('confirm');
      result.current.setSceneRootDraft(' root-1 ');
    });
    act(() => { result.current.saveWriteAccess(); result.current.saveWriteAccess(); });
    expect(commands.setMcpWriteAccess).toHaveBeenCalledOnce();
    expect(commands.setMcpWriteAccess).toHaveBeenCalledWith('confirm', 'root-1');
    await act(async () => { pending.resolve({ ...RUNNING, writeMode: 'confirm', sceneRootId: 'root-1' }); });
    expect(result.current.settings?.writeMode).toBe('confirm');
    expect(result.current.sceneRootDraft).toBe('root-1');
  });

  it('不正な許可モードの応答は入力欄へ反映しない', async () => {
    vi.mocked(commands.getMcpSettings).mockResolvedValue({ ...DISABLED, writeMode: 'invalid' } as unknown as McpSettingsPayload);
    const { result } = renderHook(() => useMcpSettings());
    await waitFor(() => expect(result.current.busy).toBe(false));
    expect(result.current.settings).toBeUndefined();
    expect(result.current.writeModeDraft).toBe('readOnly');
    expect(result.current.error?.message).toContain('不正');
  });

  it('マウント時にバックエンドの設定を読み、保存値を入力欄へ反映する', async () => {
    const { result } = renderHook(() => useMcpSettings());

    expect(result.current.busy).toBe(true);
    await waitFor(() => {
      expect(result.current.busy).toBe(false);
    });

    expect(commands.getMcpSettings).toHaveBeenCalledOnce();
    expect(result.current.settings).toEqual(DISABLED);
    expect(result.current.enabledDraft).toBe(false);
    expect(result.current.portDraft).toBe('49770');
    expect(commands.getMcpToken).not.toHaveBeenCalled();
  });

  it('StrictModeの古い取得応答は、後続の設定変更を上書きしない', async () => {
    const gets = [deferred<McpSettingsPayload>(), deferred<McpSettingsPayload>()];
    const update = deferred<McpSettingsPayload>();
    let index = 0;
    vi.mocked(commands.getMcpSettings).mockImplementation(() => gets[index++]!.promise);
    vi.mocked(commands.setMcpSettings).mockReturnValue(update.promise);
    const { result } = renderHook(() => useMcpSettings(), { wrapper: StrictMode });

    expect(commands.getMcpSettings).toHaveBeenCalledTimes(2);
    await act(async () => {
      gets[1]!.resolve(RUNNING);
    });
    await waitFor(() => {
      expect(result.current.busy).toBe(false);
    });

    act(() => result.current.setPortDraft('49772'));
    act(() => result.current.saveSettings());
    expect(commands.setMcpSettings).toHaveBeenCalledOnce();
    expect(commands.setMcpSettings).toHaveBeenCalledWith(true, 49772);

    await act(async () => {
      gets[0]!.resolve(DISABLED);
    });
    expect(result.current.busy).toBe(true);
    expect(result.current.settings).toEqual(RUNNING);

    await act(async () => {
      update.resolve({ ...RUNNING, port: 49772, endpoint: 'http://127.0.0.1:49772/mcp' });
    });
    await waitFor(() => {
      expect(result.current.busy).toBe(false);
    });
    expect(result.current.settings?.port).toBe(49772);
  });

  it('設定適用の連打はバックエンドへ1回だけ送る', async () => {
    const update = deferred<McpSettingsPayload>();
    vi.mocked(commands.setMcpSettings).mockReturnValue(update.promise);
    const { result } = renderHook(() => useMcpSettings());
    await waitFor(() => expect(result.current.busy).toBe(false));

    act(() => {
      result.current.setEnabledDraft(true);
    });
    act(() => {
      result.current.saveSettings();
      result.current.saveSettings();
    });

    expect(commands.setMcpSettings).toHaveBeenCalledOnce();
    expect(commands.setMcpSettings).toHaveBeenCalledWith(true, 49770);
    await act(async () => {
      update.resolve(RUNNING);
    });
  });

  it('トークン表示をキャンセルすると、後から届いた秘密を画面へ出さない', async () => {
    const pendingToken = deferred<{ token: string }>();
    vi.mocked(commands.getMcpToken).mockReturnValue(pendingToken.promise);
    const { result } = renderHook(() => useMcpSettings());
    await waitFor(() => expect(result.current.busy).toBe(false));

    act(() => result.current.showToken());
    expect(result.current.tokenBusy).toBe(true);
    expect(commands.getMcpToken).toHaveBeenCalledOnce();

    act(() => result.current.hideToken());
    expect(result.current.tokenBusy).toBe(false);
    expect(result.current.token).toBeUndefined();
    await act(async () => {
      pendingToken.resolve({ token: 'late-fixture-token' });
    });
    expect(result.current.token).toBeUndefined();
  });

  it('ポートが範囲外なら設定変更を送らない', async () => {
    const { result } = renderHook(() => useMcpSettings());
    await waitFor(() => expect(result.current.busy).toBe(false));

    act(() => result.current.setPortDraft('65536'));
    act(() => result.current.saveSettings());

    expect(commands.setMcpSettings).not.toHaveBeenCalled();
    expect(result.current.error?.message).toContain('1〜65535');
  });
});
