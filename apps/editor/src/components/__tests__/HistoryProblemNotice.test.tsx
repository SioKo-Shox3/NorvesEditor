// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import React from 'react';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

import * as tauriCore from '@tauri-apps/api/core';
import * as tauriEvent from '@tauri-apps/api/event';
import type { EditHistorySummary } from '@norves/bridge-ui';
import { BRIDGE_COMMANDS, BRIDGE_EVENTS } from '@norves/bridge-ui';
import { HistoryProblemNotice } from '../HistoryProblemNotice.js';
import { useBridgeActions, useBridgeSubscriptions } from '../../hooks/useBridge.js';
import { useUndoRedoKeybindings } from '../../hooks/useUndoRedoKeybindings.js';
import { BridgeProvider } from '../../state/BridgeContext.js';

const emptySummary: EditHistorySummary = {
  generation: 7,
  historyRevision: 0,
  appliedRevision: 0,
  canUndo: false,
  canRedo: false,
  undoHeadId: null,
  undoRevision: 0,
  undoGroup: null,
  redoHeadId: null,
  redoRevision: 0,
  redoGroup: null,
  pending: false,
  pendingGroup: null,
};

const knownPendingSummary: EditHistorySummary = {
  ...emptySummary,
  historyRevision: 1,
  pending: true,
  undoGroup: {
    id: 'edit-7-1',
    name: '3つの値を変更',
    source: 'ui',
    count: 3,
    createdAt: 10,
  },
  pendingGroup: {
    id: 'edit-7-1',
    name: '3つの値を変更',
    direction: 'undo',
    source: 'ui',
    createdAt: 10,
    totalCount: 3,
    completedCount: 1,
    outcomeUnknown: false,
    retryAllowed: true,
  },
};

const unknownPendingSummary: EditHistorySummary = {
  ...knownPendingSummary,
  historyRevision: 2,
  pendingGroup: {
    ...knownPendingSummary.pendingGroup!,
    outcomeUnknown: true,
    retryAllowed: false,
  },
};

const listeners: Array<{
  name: string;
  handler: (event: { payload: unknown }) => void;
}> = [];
let currentSummary = emptySummary;
let discardResult = {
  groupId: 'edit-7-1',
  completedCount: 1,
  totalCount: 3,
  outcomeUnknown: false,
  changesRemain: true,
};

function NoticeHarness(): React.JSX.Element {
  useBridgeSubscriptions();
  useUndoRedoKeybindings();
  const actions = useBridgeActions();
  return (
    <>
      <button type="button" data-testid="test-normal-edit" onClick={() => void actions.createObject()}>
        テスト用の編集
      </button>
      <HistoryProblemNotice />
    </>
  );
}

function emit(name: string, payload: unknown): void {
  const entry = listeners.find((listener) => listener.name === name);
  if (entry === undefined) {
    throw new Error(`イベント購読が見つかりません: ${name}`);
  }
  entry.handler({ payload });
}

async function renderConnectedNotice(): Promise<void> {
  render(
    <BridgeProvider>
      <div className="app-shell">
        <div className="app-shell__body">
          <NoticeHarness />
        </div>
      </div>
    </BridgeProvider>,
  );
  await waitFor(() =>
    expect(listeners.some((listener) => listener.name === BRIDGE_EVENTS.connectionState)).toBe(true),
  );
  await act(async () => {
    emit(BRIDGE_EVENTS.connectionState, { connected: true, sessionId: 'session-7' });
  });
  await waitFor(() =>
    expect(listeners.some((listener) => listener.name === BRIDGE_EVENTS.editHistoryChanged)).toBe(true),
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  listeners.length = 0;
  currentSummary = emptySummary;
  discardResult = {
    groupId: 'edit-7-1',
    completedCount: 1,
    totalCount: 3,
    outcomeUnknown: false,
    changesRemain: true,
  };
  (tauriEvent.listen as Mock).mockImplementation((name, handler) => {
    listeners.push({
      name: String(name),
      handler: handler as (event: { payload: unknown }) => void,
    });
    return Promise.resolve(vi.fn());
  });
  (tauriCore.invoke as Mock).mockImplementation((command: string) => {
    if (command === BRIDGE_COMMANDS.editGetHistory) {
      return Promise.resolve(currentSummary);
    }
    if (command === BRIDGE_COMMANDS.editRetry) {
      currentSummary = { ...emptySummary, historyRevision: 3 };
      return Promise.resolve({ accepted: true });
    }
    if (command === BRIDGE_COMMANDS.editDiscard) {
      currentSummary = { ...emptySummary, historyRevision: 3 };
      return Promise.resolve(discardResult);
    }
    return Promise.resolve(undefined);
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe('部分失敗した編集まとまりの通知', () => {
  it('要約イベントから名前・出どころ・件数を表示し、未処理分を再試行する', async () => {
    await renderConnectedNotice();
    currentSummary = knownPendingSummary;
    await act(async () => {
      emit(BRIDGE_EVENTS.editHistoryChanged, knownPendingSummary);
    });

    expect(await screen.findByText('3つの値を変更')).toBeTruthy();
    expect(screen.getByText('出どころ: 画面操作')).toBeTruthy();
    expect(screen.getByText('1 / 3 件を処理済み')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: '再試行' }));

    await waitFor(() =>
      expect(
        (tauriCore.invoke as Mock).mock.calls.some(([command]) => command === BRIDGE_COMMANDS.editRetry),
      ).toBe(true),
    );
    await waitFor(() => expect(screen.queryByRole('alertdialog')).toBeNull());
  });

  it('結果不明では再試行を出さず、状態確認で要約を再取得する', async () => {
    await renderConnectedNotice();
    currentSummary = unknownPendingSummary;
    await act(async () => {
      emit(BRIDGE_EVENTS.editHistoryChanged, unknownPendingSummary);
    });

    expect(await screen.findByText(/結果は不明です/)).toBeTruthy();
    expect(screen.queryByRole('button', { name: '再試行' })).toBeNull();
    const historyCallsBefore = (tauriCore.invoke as Mock).mock.calls.filter(
      ([command]) => command === BRIDGE_COMMANDS.editGetHistory,
    ).length;
    fireEvent.click(screen.getByRole('button', { name: '状態を確認' }));
    await waitFor(() => {
      const historyCallsAfter = (tauriCore.invoke as Mock).mock.calls.filter(
        ([command]) => command === BRIDGE_COMMANDS.editGetHistory,
      ).length;
      expect(historyCallsAfter).toBe(historyCallsBefore + 1);
    });
  });

  it('保留中は通常編集とCtrl+Z/Yから編集コマンドを送らない', async () => {
    await renderConnectedNotice();
    currentSummary = knownPendingSummary;
    await act(async () => {
      emit(BRIDGE_EVENTS.editHistoryChanged, knownPendingSummary);
    });

    expect(document.querySelector<HTMLElement>('.app-shell__body')?.inert).toBe(true);
    fireEvent.click(screen.getByTestId('test-normal-edit'));
    fireEvent.keyDown(window, { key: 'z', ctrlKey: true, bubbles: true, cancelable: true });
    fireEvent.keyDown(window, { key: 'y', ctrlKey: true, bubbles: true, cancelable: true });

    expect(
      (tauriCore.invoke as Mock).mock.calls.some(
        ([command]) =>
          command === BRIDGE_COMMANDS.sceneCreateObject ||
          command === BRIDGE_COMMANDS.editUndo ||
          command === BRIDGE_COMMANDS.editRedo,
      ),
    ).toBe(false);
  });

  it('破棄後に一部変更が残ることを通知する', async () => {
    await renderConnectedNotice();
    currentSummary = unknownPendingSummary;
    discardResult = {
      groupId: 'edit-7-1',
      completedCount: 1,
      totalCount: 3,
      outcomeUnknown: false,
      changesRemain: true,
    };
    await act(async () => {
      emit(BRIDGE_EVENTS.editHistoryChanged, unknownPendingSummary);
    });

    fireEvent.click(await screen.findByRole('button', { name: '破棄' }));
    await waitFor(() =>
      expect(
        (tauriCore.invoke as Mock).mock.calls.some(([command]) => command === BRIDGE_COMMANDS.editDiscard),
      ).toBe(true),
    );
    expect(await screen.findByText(/処理済み 1 \/ 3 件の変更は元に戻らず/)).toBeTruthy();
  });
});
