// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import React from 'react';
import type { IDockviewPanelProps } from 'dockview-react';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

import * as tauriCore from '@tauri-apps/api/core';
import * as tauriEvent from '@tauri-apps/api/event';
import type {
  ConnectionStatePayload,
  EditAppliedPayload,
  EditHistorySummary,
  SceneGetTreeResult,
} from '@norves/bridge-ui';
import { BRIDGE_COMMANDS, BRIDGE_EVENTS } from '@norves/bridge-ui';
import { PropertyInspectorPanel } from '../PropertyInspectorPanel.js';
import { SceneOutlinerPanel, __resetOutlinerMemory } from '../SceneOutlinerPanel.js';
import { useBridgeActions, useBridgeSubscriptions } from '../../hooks/useBridge.js';
import { BridgeProvider, useBridgeState } from '../../state/BridgeContext.js';

function createDeferred<T>(): {
  promise: Promise<T>;
  resolve: (value: T) => void;
} {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((complete) => {
    resolve = complete;
  });
  return { promise, resolve };
}

const panelProps = {} as IDockviewPanelProps;
const emittedEvents: string[] = [];
const unlistenFns: Mock[] = [];
let currentTree: SceneGetTreeResult = {
  root: {
    id: 'root',
    name: 'Root',
    children: [{ id: 'n-1', name: 'NodeA', kind: 'object' }],
  },
};
let currentName = 'Old';
let blockNextTreeFetch = false;
let blockedTreeFetch: ReturnType<typeof createDeferred<SceneGetTreeResult>> | undefined;

const initialSummary: EditHistorySummary = {
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
};

function EventHarness(): React.JSX.Element {
  useBridgeSubscriptions();
  const state = useBridgeState();
  const actions = useBridgeActions();
  const props = panelProps;
  return (
    <>
      <output data-testid="edit-revision">{state.editAppliedRevision ?? '—'}</output>
      <output data-testid="tree-revision">{state.sceneTreeAppliedRevision ?? '—'}</output>
      <output data-testid="object-revision">{state.objectSnapshotAppliedRevision ?? '—'}</output>
      <button type="button" onClick={() => actions.selectObject('n-1')}>
        テスト用選択
      </button>
      <SceneOutlinerPanel {...props} />
      <PropertyInspectorPanel {...props} />
    </>
  );
}

function emitBridgeEvent(name: string, payload: unknown): void {
  emittedEvents.push(name);
  const call = (tauriEvent.listen as Mock).mock.calls.find((entry) => entry[0] === name);
  const handler = call?.[1] as ((event: { payload: unknown }) => void) | undefined;
  if (handler === undefined) {
    throw new Error(`イベント購読が見つかりません: ${name}`);
  }
  handler({ payload });
}

function editEvent(
  operation: EditAppliedPayload['operation'],
  revision: number,
  detail: Pick<EditAppliedPayload, 'objectId' | 'property' | 'value' | 'newId'>,
): EditAppliedPayload {
  return {
    operation,
    ...detail,
    source: 'mcp',
    groupId: `mcp-7-${revision}`,
    generation: 7,
    sequence: revision,
    historyRevision: revision,
    appliedRevision: revision,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  emittedEvents.length = 0;
  unlistenFns.length = 0;
  currentTree = {
    root: {
      id: 'root',
      name: 'Root',
      children: [{ id: 'n-1', name: 'NodeA', kind: 'object' }],
    },
  };
  currentName = 'Old';
  blockNextTreeFetch = false;
  blockedTreeFetch = undefined;
  __resetOutlinerMemory();

  (tauriEvent.listen as Mock).mockImplementation(() => {
    const unlisten = vi.fn();
    unlistenFns.push(unlisten);
    return Promise.resolve(unlisten);
  });
  (tauriCore.invoke as Mock).mockImplementation((command: string) => {
    if (command === BRIDGE_COMMANDS.editGetHistory) {
      return Promise.resolve(initialSummary);
    }
    if (command === BRIDGE_COMMANDS.sceneGetTree) {
      if (blockNextTreeFetch) {
        blockNextTreeFetch = false;
        blockedTreeFetch = createDeferred<SceneGetTreeResult>();
        return blockedTreeFetch.promise;
      }
      return Promise.resolve(currentTree);
    }
    if (command === BRIDGE_COMMANDS.schemaGetSnapshot) {
      return Promise.resolve({ types: [] });
    }
    if (command === BRIDGE_COMMANDS.objectGetSnapshot) {
      return Promise.resolve({
        objectId: 'n-1',
        name: 'NodeA',
        kind: 'object',
        properties: [{ name: 'Name', value: currentName }],
      });
    }
    return Promise.reject(new Error(`想定外のコマンドです: ${command}`));
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe('編集サービスイベントの画面反映', () => {
  it('エンジンのlive通知なしで値・改名・構造変更を両パネルへ反映する', async () => {
    const view = render(
      <BridgeProvider>
        <EventHarness />
      </BridgeProvider>,
    );
    await act(async () => {
      await Promise.resolve();
    });

    await act(async () => {
      emitBridgeEvent(BRIDGE_EVENTS.connectionState, {
        connected: true,
        sessionId: 'session-1',
      } satisfies ConnectionStatePayload);
      await Promise.resolve();
      await Promise.resolve();
    });
    await screen.findByText('NodeA');
    await waitFor(() => expect(screen.getByTestId('edit-revision').textContent).toBe('0'));
    fireEvent.click(screen.getByText('NodeA'));
    await screen.findByDisplayValue('Old');
    await waitFor(() => {
      expect(screen.getByTestId('tree-revision').textContent).toBe('0');
      expect(screen.getByTestId('object-revision').textContent).toBe('0');
    });

    currentName = 'Renamed externally';
    currentTree = {
      root: {
        id: 'root',
        name: 'Root',
        children: [{ id: 'n-1', name: currentName, kind: 'object' }],
      },
    };
    await act(async () => {
      emitBridgeEvent(
        BRIDGE_EVENTS.editApplied,
        editEvent('setProperty', 1, {
          objectId: 'n-1',
          property: 'Name',
          value: 'Renamed externally',
          newId: null,
        }),
      );
    });
    await screen.findByDisplayValue('Renamed externally');
    expect(screen.getAllByText('Renamed externally')).toHaveLength(2);
    expect(screen.getByTestId('edit-revision').textContent).toBe('1');
    expect(screen.getByTestId('tree-revision').textContent).toBe('1');
    expect(screen.getByTestId('object-revision').textContent).toBe('1');

    currentTree = {
      root: {
        id: 'root',
        name: 'Root',
        children: [
          { id: 'n-1', name: 'Renamed externally', kind: 'object' },
          { id: 'n-2', name: 'Created externally', kind: 'object' },
        ],
      },
    };
    blockNextTreeFetch = true;
    await act(async () => {
      emitBridgeEvent(
        BRIDGE_EVENTS.editApplied,
        editEvent('createObject', 2, {
          objectId: null,
          property: null,
          value: null,
          newId: 'n-2',
        }),
      );
    });
    await waitFor(() => expect(blockedTreeFetch).toBeDefined());
    expect(screen.getByTestId('edit-revision').textContent).toBe('2');
    expect(screen.getByTestId('tree-revision').textContent).toBe('1');
    expect(screen.queryByText('Created externally')).toBeNull();

    await act(async () => {
      blockedTreeFetch!.resolve(currentTree);
      await Promise.resolve();
    });
    await screen.findByText('Created externally');
    expect(screen.getByTestId('tree-revision').textContent).toBe('2');

    expect(emittedEvents).not.toContain(BRIDGE_EVENTS.sceneTreeChanged);
    expect(emittedEvents).not.toContain(BRIDGE_EVENTS.objectChanged);
    view.unmount();
    for (const unlisten of unlistenFns) {
      expect(unlisten).toHaveBeenCalledOnce();
    }
  });
});
