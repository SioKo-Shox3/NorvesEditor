// @vitest-environment jsdom
/**
 * SettingsPanel tests — layout-reset relay (P6).
 *
 * The Settings window cannot reset the MAIN window's layout directly, so its
 * reset button must emit a cross-window request (requestLayoutReset) and must
 * NOT touch its own localStorage or reload itself. layoutReset is mocked so we
 * assert only that the right relay function is called.
 */

import { describe, it, expect, vi, beforeEach, afterEach, type Mock } from 'vitest';
import { StrictMode } from 'react';
import { render, screen, cleanup, fireEvent, waitFor, act } from '@testing-library/react';
import { SettingsPanel } from '../SettingsPanel.js';
import type { IDockviewPanelProps } from 'dockview-react';
import { LAYOUT_STORAGE_KEY } from '../shell/layoutKey.js';
import { BridgeProvider } from '../../state/BridgeContext.js';

// -------------------------------------------------------------------------
// Mock the layoutReset relay so we can assert on requestLayoutReset.
// -------------------------------------------------------------------------

vi.mock('../../shell/layoutReset.js', () => ({
  requestLayoutReset: vi.fn(() => Promise.resolve()),
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

const { requestLayoutReset } = await import('../../shell/layoutReset.js');
const tauriCore = await import('@tauri-apps/api/core');

// -------------------------------------------------------------------------
// In-memory localStorage so we can prove the panel never touches it.
// -------------------------------------------------------------------------

function installMemoryLocalStorage(): Map<string, string> {
  const map = new Map<string, string>();
  vi.stubGlobal('localStorage', {
    getItem: (k: string): string | null => (map.has(k) ? map.get(k)! : null),
    setItem: (k: string, v: string): void => { map.set(k, String(v)); },
    removeItem: (k: string): void => { map.delete(k); },
    clear: (): void => { map.clear(); },
    key: (i: number): string | null => Array.from(map.keys())[i] ?? null,
    get length(): number { return map.size; },
  });
  return map;
}

const DEFAULT_ENGINE = {
  effectivePath: 'C:/Default/Engine.exe',
  source: 'default',
  savedPath: null,
  savedArgs: [],
};

beforeEach(() => {
  vi.clearAllMocks();
  // SettingsPanel はマウント時に workspace_get で復元する。レイアウトのリセットのテストが
  // その呼び出しに左右されないよう、既定は「ワークスペースなし」にする。
  (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
    if (cmd === 'workspace_get') return Promise.resolve(null);
    if (cmd === 'get_engine_settings') return Promise.resolve(DEFAULT_ENGINE);
    return Promise.resolve(undefined);
  });
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function renderPanel(options: { strict?: boolean } = {}): void {
  const panel = (
    <BridgeProvider>
      <SettingsPanel {...({} as IDockviewPanelProps)} />
    </BridgeProvider>
  );
  render(options.strict === true ? <StrictMode>{panel}</StrictMode> : panel);
}

describe('SettingsPanel layout reset (P6)', () => {
  it('emits a layout-reset request when the reset button is clicked', () => {
    renderPanel();
    fireEvent.click(screen.getByRole('button', { name: 'レイアウトをリセット' }));
    expect(requestLayoutReset as Mock).toHaveBeenCalledOnce();
  });

  it('does NOT clear its own localStorage or reload the window on reset', () => {
    const store = installMemoryLocalStorage();
    store.set(LAYOUT_STORAGE_KEY, '{"saved":true}');
    const reload = vi.fn();
    vi.stubGlobal('location', { ...window.location, reload });

    renderPanel();
    fireEvent.click(screen.getByRole('button', { name: 'レイアウトをリセット' }));

    // The reset is relayed to the main window; this window leaves its own
    // storage untouched and does not reload itself.
    expect(store.get(LAYOUT_STORAGE_KEY)).toBe('{"saved":true}');
    expect(reload).not.toHaveBeenCalled();
    expect(requestLayoutReset as Mock).toHaveBeenCalledOnce();
  });

  it('opens a workspace from the path input and displays the backend payload', async () => {
    const workspace = {
      rootPath: 'C:/Project',
      assetsRoot: 'C:/Project/Assets',
      name: 'Project',
    };
    // Mount rehydrate sees no workspace; opening returns the backend payload.
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'workspace_get') return Promise.resolve(null);
      if (cmd === 'workspace_open') return Promise.resolve(workspace);
      return Promise.resolve(undefined);
    });

    renderPanel();
    fireEvent.change(screen.getByLabelText('Workspace root'), {
      target: { value: 'C:/Project' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Open Workspace' }));

    await waitFor(() => {
      expect(screen.getByText('Project')).toBeTruthy();
    });
    expect(tauriCore.invoke as Mock).toHaveBeenCalledWith(
      'workspace_open',
      { rootPath: 'C:/Project' },
    );
    expect(screen.getByText('C:/Project/Assets')).toBeTruthy();
  });

  it('closes the current workspace', async () => {
    const workspace = {
      rootPath: 'C:/Project',
      assetsRoot: 'C:/Project/Assets',
      name: 'Project',
    };
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'workspace_get') return Promise.resolve(null);
      if (cmd === 'workspace_open') return Promise.resolve(workspace);
      return Promise.resolve(undefined);
    });

    renderPanel();
    fireEvent.change(screen.getByLabelText('Workspace root'), {
      target: { value: 'C:/Project' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Open Workspace' }));

    await waitFor(() => {
      expect(screen.getByText('Project')).toBeTruthy();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Close Workspace' }));

    await waitFor(() => {
      expect(screen.getByText('None')).toBeTruthy();
    });
    expect(tauriCore.invoke as Mock).toHaveBeenCalledWith('workspace_close');
  });

  it('rehydrates the current workspace from the backend on mount', async () => {
    const workspace = {
      rootPath: 'C:/Existing',
      assetsRoot: 'C:/Existing/Assets',
      name: 'Existing',
    };
    (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
      if (cmd === 'workspace_get') return Promise.resolve(workspace);
      return Promise.resolve(undefined);
    });

    renderPanel();

    // No user interaction: the mount rehydrate alone must surface the backend
    // workspace and enable Close.
    await waitFor(() => {
      expect(screen.getByText('Existing')).toBeTruthy();
    });
    expect(tauriCore.invoke as Mock).toHaveBeenCalledWith('workspace_get');
    expect(
      (screen.getByRole('button', { name: 'Close Workspace' }) as HTMLButtonElement).disabled,
    ).toBe(false);
  });
});

// -------------------------------------------------------------------------
// エンジン欄。Settings は別ウィンドウなので、値は store ではなく
// get_engine_settings から直接取る。
// -------------------------------------------------------------------------

interface EnginePayload {
  effectivePath: string;
  source: 'env' | 'settings' | 'default';
  savedPath: string | null;
  savedArgs: string[];
}

const SAVED_ENGINE: EnginePayload = {
  effectivePath: 'C:/Saved/Engine.exe',
  source: 'settings',
  savedPath: 'C:/Saved/Engine.exe',
  savedArgs: [],
};

function mockEngineCommands(handlers: Record<string, () => Promise<unknown>>): void {
  (tauriCore.invoke as Mock).mockImplementation((cmd: string) => {
    if (cmd === 'workspace_get') return Promise.resolve(null);
    const handler = handlers[cmd];
    if (handler !== undefined) return handler();
    return Promise.resolve(undefined);
  });
}

function commandCalls(cmd: string): number {
  return (tauriCore.invoke as Mock).mock.calls.filter((c: unknown[]) => c[0] === cmd).length;
}

async function waitForEnginePath(path: string): Promise<void> {
  await waitFor(() => {
    expect(screen.getByTestId('engine-effective-path').textContent).toBe(path);
  });
}

function button(name: string): HTMLButtonElement {
  return screen.getByRole('button', { name }) as HTMLButtonElement;
}

describe('SettingsPanel のエンジン欄', () => {
  it('マウント時にバックエンドから取得した有効なパスと出所を表示する', async () => {
    mockEngineCommands({ get_engine_settings: () => Promise.resolve(SAVED_ENGINE) });
    renderPanel();

    await waitForEnginePath('C:/Saved/Engine.exe');
    expect(screen.getByTestId('engine-path-source').textContent).toBe('保存した設定');
    expect(commandCalls('get_engine_settings')).toBe(1);
    // 環境変数で上書きされていなければ優先の注記は出さない。
    expect(screen.queryByRole('note')).toBeNull();
  });

  it('環境変数で上書きされているときは環境変数が優先されている旨と保存済みのパスを表示する', async () => {
    mockEngineCommands({
      get_engine_settings: () =>
        Promise.resolve({
          effectivePath: 'D:/Env/Engine.exe',
          source: 'env',
          savedPath: 'C:/Saved/Engine.exe',
          savedArgs: [],
        }),
    });
    renderPanel();

    await waitForEnginePath('D:/Env/Engine.exe');
    expect(screen.getByTestId('engine-path-source').textContent).toBe(
      '環境変数 NORVES_ENGINE_PATH',
    );
    const note = screen.getByRole('note');
    expect(note.textContent).toContain('環境変数が優先されています');
    expect(note.textContent).toContain('C:/Saved/Engine.exe');
  });

  it('「参照…」で pick_engine_path を呼び、返った設定で表示を置き換える', async () => {
    mockEngineCommands({
      get_engine_settings: () => Promise.resolve(DEFAULT_ENGINE),
      pick_engine_path: () => Promise.resolve(SAVED_ENGINE),
    });
    renderPanel();
    await waitForEnginePath('C:/Default/Engine.exe');

    fireEvent.click(button('参照…'));

    await waitForEnginePath('C:/Saved/Engine.exe');
    expect(screen.getByTestId('engine-path-source').textContent).toBe('保存した設定');
    expect(tauriCore.invoke as Mock).toHaveBeenCalledWith('pick_engine_path');
  });

  it('ダイアログのキャンセル(現在の設定が返る)では表示が変わらない', async () => {
    mockEngineCommands({
      get_engine_settings: () => Promise.resolve(SAVED_ENGINE),
      pick_engine_path: () => Promise.resolve({ ...SAVED_ENGINE }),
    });
    renderPanel();
    await waitForEnginePath('C:/Saved/Engine.exe');

    fireEvent.click(button('参照…'));
    expect(button('参照…').disabled).toBe(true);

    await waitFor(() => {
      expect(button('参照…').disabled).toBe(false);
    });
    expect(commandCalls('pick_engine_path')).toBe(1);
    expect(screen.getByTestId('engine-effective-path').textContent).toBe('C:/Saved/Engine.exe');
    expect(screen.getByTestId('engine-path-source').textContent).toBe('保存した設定');
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('「既定に戻す」で clear_engine_path を呼び、既定値の表示に戻す', async () => {
    mockEngineCommands({
      get_engine_settings: () => Promise.resolve(SAVED_ENGINE),
      clear_engine_path: () => Promise.resolve(DEFAULT_ENGINE),
    });
    renderPanel();
    await waitForEnginePath('C:/Saved/Engine.exe');

    fireEvent.click(button('既定に戻す'));

    await waitForEnginePath('C:/Default/Engine.exe');
    expect(screen.getByTestId('engine-path-source').textContent).toBe('既定値');
    expect(tauriCore.invoke as Mock).toHaveBeenCalledWith('clear_engine_path');
    // 保存済みのパスが無くなったので、戻す操作は無効になる。
    expect(button('既定に戻す').disabled).toBe(true);
  });

  it('コマンドが失敗したらエラーを表示し、表示中のパスは保つ', async () => {
    mockEngineCommands({
      get_engine_settings: () => Promise.resolve(SAVED_ENGINE),
      pick_engine_path: () =>
        Promise.reject({ kind: 'settings', message: '設定ファイルを書き込めません' }),
    });
    renderPanel();
    await waitForEnginePath('C:/Saved/Engine.exe');

    fireEvent.click(button('参照…'));

    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('settings');
    expect(alert.textContent).toContain('設定ファイルを書き込めません');
    expect(screen.getByTestId('engine-effective-path').textContent).toBe('C:/Saved/Engine.exe');
    // 失敗のあとはまた押せる。
    await waitFor(() => {
      expect(button('参照…').disabled).toBe(false);
    });
  });

  it('初回の取得に失敗したらエラーを表示する', async () => {
    mockEngineCommands({
      get_engine_settings: () => Promise.reject({ kind: 'settings', message: '読めません' }),
    });
    renderPanel();

    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('読めません');
    expect(screen.queryByTestId('engine-effective-path')).toBeNull();
  });

  it('処理中はボタンを押せず、続けて押しても pick_engine_path は1回しか呼ばれない', async () => {
    let resolvePick: (v: EnginePayload) => void = () => undefined;
    mockEngineCommands({
      get_engine_settings: () => Promise.resolve(SAVED_ENGINE),
      pick_engine_path: () =>
        new Promise<EnginePayload>((resolve) => {
          resolvePick = resolve;
        }),
    });
    renderPanel();
    await waitForEnginePath('C:/Saved/Engine.exe');

    const browse = button('参照…');
    const reset = button('既定に戻す');
    fireEvent.click(browse);
    fireEvent.click(browse);
    fireEvent.click(reset);

    expect(browse.disabled).toBe(true);
    expect(reset.disabled).toBe(true);
    expect(commandCalls('pick_engine_path')).toBe(1);
    expect(commandCalls('clear_engine_path')).toBe(0);

    resolvePick(SAVED_ENGINE);
    await waitFor(() => {
      expect(browse.disabled).toBe(false);
    });
    expect(reset.disabled).toBe(false);
  });
});

// -------------------------------------------------------------------------
// 応答の順序。StrictMode の二重マウントで初回の取得が2件出ても、古い応答で
// 処理中を解除しない。
// -------------------------------------------------------------------------

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
}

function deferred<T>(): Deferred<T> {
  let resolve: (value: T) => void = () => undefined;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

function deferredCommand(list: Deferred<EnginePayload>[]): () => Promise<EnginePayload> {
  return () => {
    const d = deferred<EnginePayload>();
    list.push(d);
    return d.promise;
  };
}

function argsInput(): HTMLTextAreaElement {
  return screen.getByLabelText('起動引数(1 行に 1 つ)') as HTMLTextAreaElement;
}

describe('SettingsPanel のエンジン欄の応答順序', () => {
  it('古い取得の応答が後から届いても処理中を解除せず、pick_engine_path は1回だけ呼ばれる', async () => {
    const gets: Deferred<EnginePayload>[] = [];
    const picks: Deferred<EnginePayload>[] = [];
    mockEngineCommands({
      get_engine_settings: deferredCommand(gets),
      pick_engine_path: deferredCommand(picks),
    });
    renderPanel({ strict: true });
    await waitFor(() => {
      expect(gets.length).toBe(2);
    });

    // 新しい取得が先に終わる。
    await act(async () => {
      gets[1]!.resolve(SAVED_ENGINE);
    });
    await waitFor(() => {
      expect(button('参照…').disabled).toBe(false);
    });
    fireEvent.click(button('参照…'));
    expect(commandCalls('pick_engine_path')).toBe(1);

    // pick の途中で古い取得が終わっても、処理中のまま。
    await act(async () => {
      gets[0]!.resolve({ ...DEFAULT_ENGINE, savedArgs: ['--stale'] } as EnginePayload);
    });
    expect(button('参照…').disabled).toBe(true);
    fireEvent.click(button('参照…'));
    expect(commandCalls('pick_engine_path')).toBe(1);
    // 古い応答の値は表示に出さない。
    expect(screen.getByTestId('engine-effective-path').textContent).toBe('C:/Saved/Engine.exe');
    expect(argsInput().value).toBe('');

    await act(async () => {
      picks[0]!.resolve(SAVED_ENGINE);
    });
    await waitFor(() => {
      expect(button('参照…').disabled).toBe(false);
    });
  });

  it('古い取得が先に終わっても、新しい取得が終わるまでは押せない', async () => {
    const gets: Deferred<EnginePayload>[] = [];
    mockEngineCommands({
      get_engine_settings: deferredCommand(gets),
      pick_engine_path: () => Promise.resolve(SAVED_ENGINE),
    });
    renderPanel({ strict: true });
    await waitFor(() => {
      expect(gets.length).toBe(2);
    });

    await act(async () => {
      gets[0]!.resolve(DEFAULT_ENGINE as EnginePayload);
    });
    fireEvent.click(button('参照…'));
    expect(commandCalls('pick_engine_path')).toBe(0);
    expect(screen.queryByTestId('engine-effective-path')).toBeNull();

    await act(async () => {
      gets[1]!.resolve(SAVED_ENGINE);
    });
    await waitForEnginePath('C:/Saved/Engine.exe');
    expect(button('参照…').disabled).toBe(false);
  });
});

// -------------------------------------------------------------------------
// 起動引数。入力欄の 1 行が 1 引数。確かめるのはバックエンド。
// -------------------------------------------------------------------------

describe('SettingsPanel の起動引数', () => {
  it('保存済みの引数を 1 行 1 引数で入力欄に出す', async () => {
    mockEngineCommands({
      get_engine_settings: () =>
        Promise.resolve({ ...SAVED_ENGINE, savedArgs: ['--scene', 'a b.scene'] }),
    });
    renderPanel();

    await waitFor(() => {
      expect(argsInput().value).toBe('--scene\na b.scene');
    });
  });

  it('「引数を保存」で行ごとに分けて set_engine_args を呼び、保存後の値と結果を表示する', async () => {
    mockEngineCommands({
      get_engine_settings: () => Promise.resolve(SAVED_ENGINE),
      set_engine_args: () =>
        Promise.resolve({ ...SAVED_ENGINE, savedArgs: ['--scene', 'main.scene'] }),
    });
    renderPanel();
    await waitForEnginePath('C:/Saved/Engine.exe');

    fireEvent.change(argsInput(), { target: { value: '--scene\r\n\nmain.scene\n' } });
    fireEvent.click(button('引数を保存'));

    expect(tauriCore.invoke as Mock).toHaveBeenCalledWith('set_engine_args', {
      args: ['--scene', '', 'main.scene', ''],
    });
    const status = await screen.findByRole('status');
    expect(status.textContent).toBe('起動引数を保存しました');
    // 空行を捨てた後の、保存された並びに置き換わる。
    expect(argsInput().value).toBe('--scene\nmain.scene');

    // 入力し直したら結果の表示は消える。
    fireEvent.change(argsInput(), { target: { value: '--other' } });
    expect(screen.queryByRole('status')).toBeNull();
  });

  it('保存が拒否されたらエラーを表示し、入力欄の内容は残す', async () => {
    mockEngineCommands({
      get_engine_settings: () => Promise.resolve(SAVED_ENGINE),
      set_engine_args: () =>
        Promise.reject({
          kind: 'settings',
          message: '1 件目: --bridge-port はエディタが渡すので指定できません',
        }),
    });
    renderPanel();
    await waitForEnginePath('C:/Saved/Engine.exe');

    fireEvent.change(argsInput(), { target: { value: '--bridge-port=1' } });
    fireEvent.click(button('引数を保存'));

    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('--bridge-port はエディタが渡すので指定できません');
    expect(argsInput().value).toBe('--bridge-port=1');
    expect(screen.queryByRole('status')).toBeNull();
    await waitFor(() => {
      expect(button('引数を保存').disabled).toBe(false);
    });
  });

  it('パスの操作では書きかけの引数を消さない', async () => {
    mockEngineCommands({
      get_engine_settings: () => Promise.resolve(DEFAULT_ENGINE),
      pick_engine_path: () => Promise.resolve({ ...SAVED_ENGINE, savedArgs: ['--old'] }),
    });
    renderPanel();
    await waitForEnginePath('C:/Default/Engine.exe');

    fireEvent.change(argsInput(), { target: { value: '--draft' } });
    fireEvent.click(button('参照…'));

    await waitForEnginePath('C:/Saved/Engine.exe');
    expect(argsInput().value).toBe('--draft');
  });

  it('処理中は保存できず、続けて押しても set_engine_args は1回しか呼ばれない', async () => {
    const saves: Deferred<EnginePayload>[] = [];
    mockEngineCommands({
      get_engine_settings: () => Promise.resolve(SAVED_ENGINE),
      set_engine_args: deferredCommand(saves),
    });
    renderPanel();
    await waitForEnginePath('C:/Saved/Engine.exe');

    fireEvent.change(argsInput(), { target: { value: '--a' } });
    fireEvent.click(button('引数を保存'));
    fireEvent.click(button('引数を保存'));
    fireEvent.click(button('参照…'));
    expect(button('引数を保存').disabled).toBe(true);
    expect(commandCalls('set_engine_args')).toBe(1);
    expect(commandCalls('pick_engine_path')).toBe(0);

    await act(async () => {
      saves[0]!.resolve({ ...SAVED_ENGINE, savedArgs: ['--a'] });
    });
    await waitFor(() => {
      expect(button('引数を保存').disabled).toBe(false);
    });
  });

  it('savedArgs の無い応答は不正な応答として扱う', async () => {
    mockEngineCommands({
      get_engine_settings: () =>
        Promise.resolve({ effectivePath: 'C:/x.exe', source: 'default', savedPath: null }),
    });
    renderPanel();

    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('エンジンの設定の応答が不正です');
  });
});
