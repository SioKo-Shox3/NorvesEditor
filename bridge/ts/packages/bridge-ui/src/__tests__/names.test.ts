import { describe, it, expect } from 'vitest';
import { BRIDGE_COMMANDS, BRIDGE_EVENTS } from '../index.js';

// ---------------------------------------------------------------------------
// Literal value assertions — any accidental string edit is caught here.
// ---------------------------------------------------------------------------

describe('BRIDGE_COMMANDS literal values', () => {
  it('connect = bridge_connect', () => {
    expect(BRIDGE_COMMANDS.connect).toBe('bridge_connect');
  });
  it('disconnect = bridge_disconnect', () => {
    expect(BRIDGE_COMMANDS.disconnect).toBe('bridge_disconnect');
  });
  it('reconnect = bridge_reconnect', () => {
    expect(BRIDGE_COMMANDS.reconnect).toBe('bridge_reconnect');
  });
  it('getStatus = get_status', () => {
    expect(BRIDGE_COMMANDS.getStatus).toBe('get_status');
  });
  it('sceneGetTree = scene_get_tree', () => {
    expect(BRIDGE_COMMANDS.sceneGetTree).toBe('scene_get_tree');
  });
  it('objectGetSnapshot = object_get_snapshot', () => {
    expect(BRIDGE_COMMANDS.objectGetSnapshot).toBe('object_get_snapshot');
  });
  it('objectSetProperty = object_set_property', () => {
    expect(BRIDGE_COMMANDS.objectSetProperty).toBe('object_set_property');
  });
  it('schemaGetSnapshot = schema_get_snapshot', () => {
    expect(BRIDGE_COMMANDS.schemaGetSnapshot).toBe('schema_get_snapshot');
  });
  it('runtimePlay = runtime_play', () => {
    expect(BRIDGE_COMMANDS.runtimePlay).toBe('runtime_play');
  });
  it('runtimePause = runtime_pause', () => {
    expect(BRIDGE_COMMANDS.runtimePause).toBe('runtime_pause');
  });
  it('runtimeStop = runtime_stop', () => {
    expect(BRIDGE_COMMANDS.runtimeStop).toBe('runtime_stop');
  });
  it('focusViewport = focus_viewport', () => {
    expect(BRIDGE_COMMANDS.focusViewport).toBe('focus_viewport');
  });
  it('launchEngine = launch_engine', () => {
    expect(BRIDGE_COMMANDS.launchEngine).toBe('launch_engine');
  });
  it('stopEngine = stop_engine', () => {
    expect(BRIDGE_COMMANDS.stopEngine).toBe('stop_engine');
  });
  it('workspaceOpen = workspace_open', () => {
    expect(BRIDGE_COMMANDS.workspaceOpen).toBe('workspace_open');
  });
  it('workspaceGet = workspace_get', () => {
    expect(BRIDGE_COMMANDS.workspaceGet).toBe('workspace_get');
  });
  it('workspaceClose = workspace_close', () => {
    expect(BRIDGE_COMMANDS.workspaceClose).toBe('workspace_close');
  });
});

describe('BRIDGE_EVENTS literal values', () => {
  it('connectionState = bridge:connection-state', () => {
    expect(BRIDGE_EVENTS.connectionState).toBe('bridge:connection-state');
  });
  it('statusChanged = bridge:status-changed', () => {
    expect(BRIDGE_EVENTS.statusChanged).toBe('bridge:status-changed');
  });
  it('runtimeStateChanged = bridge:runtime-state-changed', () => {
    expect(BRIDGE_EVENTS.runtimeStateChanged).toBe('bridge:runtime-state-changed');
  });
  it('logMessage = bridge:log-message', () => {
    expect(BRIDGE_EVENTS.logMessage).toBe('bridge:log-message');
  });
  it('errorReported = bridge:error-reported', () => {
    expect(BRIDGE_EVENTS.errorReported).toBe('bridge:error-reported');
  });
  it('engineProcessExited = bridge:engine-process-exited', () => {
    expect(BRIDGE_EVENTS.engineProcessExited).toBe('bridge:engine-process-exited');
  });
  it('viewportStateChanged = bridge:viewport-state-changed', () => {
    expect(BRIDGE_EVENTS.viewportStateChanged).toBe('bridge:viewport-state-changed');
  });
  it('bridgeConnected = bridge:bridge-connected', () => {
    expect(BRIDGE_EVENTS.bridgeConnected).toBe('bridge:bridge-connected');
  });
  it('bridgeDisconnected = bridge:bridge-disconnected', () => {
    expect(BRIDGE_EVENTS.bridgeDisconnected).toBe('bridge:bridge-disconnected');
  });
});

// ---------------------------------------------------------------------------
// No-duplicate guards — values within each set must be unique.
// ---------------------------------------------------------------------------

describe('BRIDGE_COMMANDS has no duplicate values', () => {
  it('all command name strings are distinct', () => {
    const values = Object.values(BRIDGE_COMMANDS);
    const unique = new Set(values);
    expect(unique.size).toBe(values.length);
  });
});

describe('BRIDGE_EVENTS has no duplicate values', () => {
  it('all event name strings are distinct', () => {
    const values = Object.values(BRIDGE_EVENTS);
    const unique = new Set(values);
    expect(unique.size).toBe(values.length);
  });
});
