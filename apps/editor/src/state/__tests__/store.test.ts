/**
 * Pure reducer unit tests.
 *
 * These tests cover state transitions for all action types.
 * They are the substitute for a real GUI round-trip test;
 * true end-to-end testing requires a running Tauri process + engine
 * (see plan §10 manual acceptance).
 */

import { describe, it, expect } from 'vitest';
import { bridgeReducer, INITIAL_STATE, type BridgeState, type BridgeAction } from '../store.js';

// -------------------------------------------------------------------------
// Helpers
// -------------------------------------------------------------------------

function applyAction(action: BridgeAction, state: BridgeState = INITIAL_STATE): BridgeState {
  return bridgeReducer(state, action);
}

// -------------------------------------------------------------------------
// connectionStateChanged
// -------------------------------------------------------------------------

describe('connectionStateChanged', () => {
  it('sets status to connected and fills session info', () => {
    const next = applyAction({
      type: 'connectionStateChanged',
      payload: {
        connected: true,
        sessionId: 'sess-1',
        serverName: 'MockEngine',
        endpoint: 'ws://127.0.0.1:9001',
      },
    });
    expect(next.connection.status).toBe('connected');
    expect(next.connection.sessionId).toBe('sess-1');
    expect(next.connection.serverName).toBe('MockEngine');
    expect(next.connection.endpoint).toBe('ws://127.0.0.1:9001');
  });

  it('sets status to disconnected when connected is false', () => {
    const next = applyAction({
      type: 'connectionStateChanged',
      payload: { connected: false, reason: 'Socket closed' },
    });
    expect(next.connection.status).toBe('disconnected');
    expect(next.connection.reason).toBe('Socket closed');
  });

  it('clears lastError on successful connection', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      lastError: { message: 'old error' },
    };
    const next = applyAction(
      { type: 'connectionStateChanged', payload: { connected: true } },
      state,
    );
    expect(next.lastError).toBeUndefined();
  });

  it('preserves lastError on disconnect', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      lastError: { message: 'old error' },
      connection: { status: 'connected' },
    };
    const next = applyAction(
      { type: 'connectionStateChanged', payload: { connected: false } },
      state,
    );
    expect(next.lastError).toEqual({ message: 'old error' });
  });
});

// -------------------------------------------------------------------------
// commandPending
// -------------------------------------------------------------------------

describe('commandPending', () => {
  it('moves disconnected -> connecting', () => {
    const next = applyAction({ type: 'commandPending' });
    expect(next.connection.status).toBe('connecting');
  });

  it('does not overwrite connected status', () => {
    const state: BridgeState = { ...INITIAL_STATE, connection: { status: 'connected' } };
    const next = applyAction({ type: 'commandPending' }, state);
    expect(next.connection.status).toBe('connected');
  });

  it('does not overwrite error status', () => {
    const state: BridgeState = { ...INITIAL_STATE, connection: { status: 'error' } };
    const next = applyAction({ type: 'commandPending' }, state);
    expect(next.connection.status).toBe('error');
  });
});

// -------------------------------------------------------------------------
// statusUpdated
// -------------------------------------------------------------------------

describe('statusUpdated', () => {
  it('sets engineState and runtimeState', () => {
    const next = applyAction({
      type: 'statusUpdated',
      payload: {
        engineState: 'running',
        runtimeState: 'playing',
        engineName: 'NorvesLib',
        title: 'My Scene',
      },
    });
    expect(next.engineState).toBe('running');
    expect(next.runtimeState).toBe('playing');
    expect(next.engineName).toBe('NorvesLib');
    expect(next.title).toBe('My Scene');
  });
});

// -------------------------------------------------------------------------
// logAppended
// -------------------------------------------------------------------------

describe('logAppended', () => {
  it('appends a log entry with the given id', () => {
    const next = applyAction({
      type: 'logAppended',
      payload: { level: 'info', message: 'Hello', timestamp: '2026-06-14T10:00:00Z' },
      id: 42,
    });
    expect(next.logs).toHaveLength(1);
    expect(next.logs[0]).toMatchObject({ id: 42, level: 'info', message: 'Hello' });
  });

  it('caps logs at 1000 entries (drops oldest)', () => {
    // Start with 999 existing entries
    const baseState: BridgeState = {
      ...INITIAL_STATE,
      logs: Array.from({ length: 999 }, (_, i) => ({
        id: i,
        level: 'debug' as const,
        message: `msg${i}`,
      })),
    };

    // Append one more (total = 1000, no cap yet)
    let s = applyAction(
      { type: 'logAppended', payload: { level: 'info', message: 'entry-1000' }, id: 1000 },
      baseState,
    );
    expect(s.logs).toHaveLength(1000);

    // Append another (total would be 1001 -> cap triggers, drops oldest)
    s = applyAction(
      { type: 'logAppended', payload: { level: 'warn', message: 'entry-1001' }, id: 1001 },
      s,
    );
    expect(s.logs).toHaveLength(1000);
    expect(s.logs[0]?.id).toBe(1);       // oldest dropped is id=0
    expect(s.logs[999]?.id).toBe(1001);  // newest is 1001
  });
});

// -------------------------------------------------------------------------
// runtimeStateChanged
// -------------------------------------------------------------------------

describe('runtimeStateChanged', () => {
  it('updates runtimeState', () => {
    const next = applyAction({
      type: 'runtimeStateChanged',
      payload: { state: 'playing', previous: 'edit' },
    });
    expect(next.runtimeState).toBe('playing');
  });
});

// -------------------------------------------------------------------------
// engineStatusChanged
// -------------------------------------------------------------------------

describe('engineStatusChanged', () => {
  it('updates engineState', () => {
    const next = applyAction({
      type: 'engineStatusChanged',
      payload: { engineState: 'running', runtimeState: 'edit' },
    });
    expect(next.engineState).toBe('running');
    expect(next.runtimeState).toBe('edit');
  });

  it('preserves existing runtimeState if not provided', () => {
    const state: BridgeState = { ...INITIAL_STATE, runtimeState: 'playing' };
    const next = applyAction(
      { type: 'engineStatusChanged', payload: { engineState: 'ready' } },
      state,
    );
    expect(next.runtimeState).toBe('playing');
  });
});

// -------------------------------------------------------------------------
// errorReported
// -------------------------------------------------------------------------

describe('errorReported', () => {
  it('sets lastError and status to error', () => {
    const next = applyAction({
      type: 'errorReported',
      payload: { error: { code: 'SOME_ERR', message: 'Something went wrong' } },
    });
    expect(next.lastError).toEqual({ kind: 'SOME_ERR', message: 'Something went wrong' });
    expect(next.connection.status).toBe('error');
  });
});

// -------------------------------------------------------------------------
// engineProcessExited
// -------------------------------------------------------------------------

describe('engineProcessExited', () => {
  it('clears engineState, runtimeState and sets connection to disconnected', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      engineState: 'running',
      runtimeState: 'playing',
      connection: { status: 'connected' },
    };
    const next = applyAction(
      { type: 'engineProcessExited', payload: { exitCode: 0 } },
      state,
    );
    expect(next.engineState).toBeUndefined();
    expect(next.runtimeState).toBeUndefined();
    expect(next.connection.status).toBe('disconnected');
  });
});

// -------------------------------------------------------------------------
// dismissError
// -------------------------------------------------------------------------

describe('dismissError', () => {
  it('clears lastError when set', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      lastError: { kind: 'process', message: 'engine executable not found' },
    };
    const next = applyAction({ type: 'dismissError' }, state);
    expect(next.lastError).toBeUndefined();
  });

  it('leaves connection unchanged when clearing lastError', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      connection: { status: 'error' },
      lastError: { kind: 'connect', message: 'refused' },
    };
    const next = applyAction({ type: 'dismissError' }, state);
    // lastError cleared, connection.status unchanged
    expect(next.lastError).toBeUndefined();
    expect(next.connection.status).toBe('error');
  });

  it('is a no-op when lastError is already undefined', () => {
    const next = applyAction({ type: 'dismissError' });
    expect(next.lastError).toBeUndefined();
    expect(next.connection.status).toBe('disconnected');
  });
});

// -------------------------------------------------------------------------
// objectSelected
// -------------------------------------------------------------------------

describe('objectSelected', () => {
  it('sets selectedObjectId to the given id', () => {
    const next = applyAction({ type: 'objectSelected', id: 'obj-123' });
    expect(next.selectedObjectId).toBe('obj-123');
  });

  it('sets selectedObjectId to undefined (deselect)', () => {
    const state: BridgeState = { ...INITIAL_STATE, selectedObjectId: 'obj-123' };
    const next = applyAction({ type: 'objectSelected', id: undefined }, state);
    expect(next.selectedObjectId).toBeUndefined();
  });

  it('overwrites a previous selection', () => {
    const state: BridgeState = { ...INITIAL_STATE, selectedObjectId: 'obj-111' };
    const next = applyAction({ type: 'objectSelected', id: 'obj-222' }, state);
    expect(next.selectedObjectId).toBe('obj-222');
  });

  it('does not mutate other state fields', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      engineState: 'running',
      runtimeState: 'playing',
    };
    const next = applyAction({ type: 'objectSelected', id: 'obj-abc' }, state);
    expect(next.engineState).toBe('running');
    expect(next.runtimeState).toBe('playing');
    expect(next.selectedObjectId).toBe('obj-abc');
  });
});

// -------------------------------------------------------------------------
// sceneTreeLoaded / sceneTreeUnsupported
// -------------------------------------------------------------------------

describe('sceneTreeLoaded', () => {
  it('stores the root node and clears any unsupported marker', () => {
    const state: BridgeState = { ...INITIAL_STATE, sceneUnsupported: true };
    const next = applyAction(
      { type: 'sceneTreeLoaded', root: { id: 'n-0', name: 'Root', children: [{ id: 'n-1' }] } },
      state,
    );
    expect(next.sceneTree?.id).toBe('n-0');
    expect(next.sceneTree?.children?.[0]?.id).toBe('n-1');
    expect(next.sceneUnsupported).toBe(false);
  });
});

describe('sceneTreeUnsupported', () => {
  it('sets the unsupported marker and drops any stale tree', () => {
    const state: BridgeState = { ...INITIAL_STATE, sceneTree: { id: 'n-0' } };
    const next = applyAction({ type: 'sceneTreeUnsupported' }, state);
    expect(next.sceneUnsupported).toBe(true);
    expect(next.sceneTree).toBeUndefined();
  });
});

describe('scene snapshot is cleared on disconnect', () => {
  it('connectionStateChanged(connected:false) drops sceneTree + selection', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      sceneTree: { id: 'n-0' },
      selectedObjectId: 'n-0',
    };
    const next = applyAction(
      { type: 'connectionStateChanged', payload: { connected: false, reason: 'closed' } },
      state,
    );
    expect(next.sceneTree).toBeUndefined();
    expect(next.selectedObjectId).toBeUndefined();
  });

  it('connectionStateChanged(connected:true) clears a stale unsupported marker', () => {
    // A fresh connection re-probes the engine, so the prior "unsupported" verdict
    // must not leak across connections.
    const state: BridgeState = { ...INITIAL_STATE, sceneUnsupported: true };
    const next = applyAction(
      { type: 'connectionStateChanged', payload: { connected: true, sessionId: 's' } },
      state,
    );
    expect(next.sceneUnsupported).toBe(false);
  });

  it('connectionStateChanged(connected:true) preserves an existing tree', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      sceneTree: { id: 'n-0' },
      selectedObjectId: 'n-0',
    };
    const next = applyAction(
      { type: 'connectionStateChanged', payload: { connected: true, sessionId: 's' } },
      state,
    );
    expect(next.sceneTree?.id).toBe('n-0');
    expect(next.selectedObjectId).toBe('n-0');
  });

  it('engineProcessExited drops sceneTree + selection', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      sceneTree: { id: 'n-0' },
      selectedObjectId: 'n-0',
    };
    const next = applyAction(
      { type: 'engineProcessExited', payload: { exitCode: 0 } },
      state,
    );
    expect(next.sceneTree).toBeUndefined();
    expect(next.selectedObjectId).toBeUndefined();
  });
});

// -------------------------------------------------------------------------
// objectSnapshotLoaded / schemaSnapshotLoaded / objectSnapshotUnsupported
// -------------------------------------------------------------------------

describe('objectSnapshotLoaded', () => {
  it('stores the snapshot and clears any unsupported marker', () => {
    const state: BridgeState = { ...INITIAL_STATE, objectUnsupported: true };
    const next = applyAction(
      {
        type: 'objectSnapshotLoaded',
        snapshot: { objectId: 'n-1', name: 'NodeA', properties: [{ name: 'x', value: 1 }] },
      },
      state,
    );
    expect(next.objectSnapshot?.objectId).toBe('n-1');
    expect(next.objectSnapshot?.properties[0]?.value).toBe(1);
    expect(next.objectUnsupported).toBe(false);
  });
});

describe('schemaSnapshotLoaded', () => {
  it('stores the type descriptors', () => {
    const next = applyAction({
      type: 'schemaSnapshotLoaded',
      types: [{ typeName: 'TypeA', properties: [{ name: 'fieldOfView', valueType: 'number' }] }],
    });
    expect(next.schemaTypes?.[0]?.typeName).toBe('TypeA');
  });
});

describe('objectSnapshotUnsupported', () => {
  it('sets the unsupported marker and drops any stale snapshot', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      objectSnapshot: { objectId: 'n-1', properties: [] },
    };
    const next = applyAction({ type: 'objectSnapshotUnsupported' }, state);
    expect(next.objectUnsupported).toBe(true);
    expect(next.objectSnapshot).toBeUndefined();
  });
});

describe('objectPropertyApplied', () => {
  const seeded: BridgeState = {
    ...INITIAL_STATE,
    objectSnapshot: {
      objectId: 'n-1',
      name: 'NodeA',
      properties: [
        { name: 'fieldOfView', value: 60, valueType: 'number' },
        { name: 'position', value: [0, 0, 0], valueType: 'vector3' },
      ],
    },
  };

  it('replaces the named property value with the applied value', () => {
    const next = applyAction(
      { type: 'objectPropertyApplied', objectId: 'n-1', property: 'fieldOfView', appliedValue: 75 },
      seeded,
    );
    expect(next.objectSnapshot?.properties[0]?.value).toBe(75);
    // Other properties are untouched.
    expect(next.objectSnapshot?.properties[1]?.value).toEqual([0, 0, 0]);
  });

  it('applies a structured (array) value', () => {
    const next = applyAction(
      {
        type: 'objectPropertyApplied',
        objectId: 'n-1',
        property: 'position',
        appliedValue: [9, 8, 7],
      },
      seeded,
    );
    expect(next.objectSnapshot?.properties[1]?.value).toEqual([9, 8, 7]);
  });

  it('is a no-op when the snapshot objectId differs (late ack guard)', () => {
    const next = applyAction(
      { type: 'objectPropertyApplied', objectId: 'n-2', property: 'fieldOfView', appliedValue: 75 },
      seeded,
    );
    expect(next).toBe(seeded);
  });

  it('is a no-op when there is no snapshot', () => {
    const next = applyAction(
      { type: 'objectPropertyApplied', objectId: 'n-1', property: 'fieldOfView', appliedValue: 75 },
      INITIAL_STATE,
    );
    expect(next).toBe(INITIAL_STATE);
  });

  it('is a no-op when the property is absent', () => {
    const next = applyAction(
      { type: 'objectPropertyApplied', objectId: 'n-1', property: 'missing', appliedValue: 1 },
      seeded,
    );
    expect(next).toBe(seeded);
  });
});

describe('objectSelected clears the prior object snapshot', () => {
  it('drops objectSnapshot when the selection changes to a different id', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      selectedObjectId: 'n-1',
      objectSnapshot: { objectId: 'n-1', properties: [] },
    };
    const next = applyAction({ type: 'objectSelected', id: 'n-2' }, state);
    expect(next.selectedObjectId).toBe('n-2');
    expect(next.objectSnapshot).toBeUndefined();
  });

  it('drops objectSnapshot on deselect (id undefined)', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      selectedObjectId: 'n-1',
      objectSnapshot: { objectId: 'n-1', properties: [] },
    };
    const next = applyAction({ type: 'objectSelected', id: undefined }, state);
    expect(next.selectedObjectId).toBeUndefined();
    expect(next.objectSnapshot).toBeUndefined();
  });

  it('keeps objectSnapshot when re-selecting the same id', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      selectedObjectId: 'n-1',
      objectSnapshot: { objectId: 'n-1', properties: [] },
    };
    const next = applyAction({ type: 'objectSelected', id: 'n-1' }, state);
    expect(next.objectSnapshot?.objectId).toBe('n-1');
  });
});

describe('inspector data is cleared on disconnect / process exit', () => {
  it('connectionStateChanged(connected:false) drops objectSnapshot + schema', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      objectSnapshot: { objectId: 'n-1', properties: [] },
      schemaTypes: [{ typeName: 'TypeA' }],
    };
    const next = applyAction(
      { type: 'connectionStateChanged', payload: { connected: false, reason: 'closed' } },
      state,
    );
    expect(next.objectSnapshot).toBeUndefined();
    expect(next.schemaTypes).toBeUndefined();
  });

  it('connectionStateChanged(connected:true) clears a stale objectUnsupported marker', () => {
    const state: BridgeState = { ...INITIAL_STATE, objectUnsupported: true };
    const next = applyAction(
      { type: 'connectionStateChanged', payload: { connected: true, sessionId: 's' } },
      state,
    );
    expect(next.objectUnsupported).toBe(false);
  });

  it('engineProcessExited drops objectSnapshot + schema', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      connection: { status: 'connected' },
      objectSnapshot: { objectId: 'n-1', properties: [] },
      schemaTypes: [{ typeName: 'TypeA' }],
    };
    const next = applyAction({ type: 'engineProcessExited', payload: { exitCode: 0 } }, state);
    expect(next.objectSnapshot).toBeUndefined();
    expect(next.schemaTypes).toBeUndefined();
  });
});

// -------------------------------------------------------------------------
// viewportStateChanged
// -------------------------------------------------------------------------

describe('viewportStateChanged', () => {
  it('updates viewportState from the event payload', () => {
    const next = applyAction({
      type: 'viewportStateChanged',
      payload: { state: 'focused', previous: 'hidden' },
    });
    expect(next.viewportState).toBe('focused');
  });

  it('overwrites a previously set viewportState', () => {
    const state: BridgeState = { ...INITIAL_STATE, viewportState: 'focused' };
    const next = applyAction(
      { type: 'viewportStateChanged', payload: { state: 'minimized' } },
      state,
    );
    expect(next.viewportState).toBe('minimized');
  });

  it('does not mutate other state fields', () => {
    const state: BridgeState = {
      ...INITIAL_STATE,
      engineState: 'running',
      runtimeState: 'playing',
    };
    const next = applyAction(
      { type: 'viewportStateChanged', payload: { state: 'visible' } },
      state,
    );
    expect(next.engineState).toBe('running');
    expect(next.runtimeState).toBe('playing');
    expect(next.viewportState).toBe('visible');
  });
});
