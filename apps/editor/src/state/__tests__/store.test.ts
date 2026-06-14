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
