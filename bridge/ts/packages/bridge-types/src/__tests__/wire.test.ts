import { describe, it, expect } from 'vitest';
import {
  LOG_LEVELS,
  ENGINE_STATES,
  RUNTIME_STATES,
  VIEWPORT_STATES,
  ORIGINS,
  KINDS,
} from '../index.js';

describe('LOG_LEVELS wire strings', () => {
  it('contains all five lowercase levels', () => {
    expect(LOG_LEVELS).toContain('trace');
    expect(LOG_LEVELS).toContain('debug');
    expect(LOG_LEVELS).toContain('info');
    expect(LOG_LEVELS).toContain('warn');
    expect(LOG_LEVELS).toContain('error');
  });

  it('has exactly 5 members', () => {
    expect(LOG_LEVELS).toHaveLength(5);
  });

  it('does not contain non-wire strings', () => {
    expect(LOG_LEVELS).not.toContain('fatal');
    expect(LOG_LEVELS).not.toContain('WARNING');
  });
});

describe('ENGINE_STATES wire strings', () => {
  it('contains all four states', () => {
    expect(ENGINE_STATES).toContain('initializing');
    expect(ENGINE_STATES).toContain('ready');
    expect(ENGINE_STATES).toContain('running');
    expect(ENGINE_STATES).toContain('error');
  });

  it('has exactly 4 members', () => {
    expect(ENGINE_STATES).toHaveLength(4);
  });
});

describe('RUNTIME_STATES wire strings', () => {
  it('contains all five states', () => {
    expect(RUNTIME_STATES).toContain('edit');
    expect(RUNTIME_STATES).toContain('playing');
    expect(RUNTIME_STATES).toContain('paused');
    expect(RUNTIME_STATES).toContain('stopped');
    expect(RUNTIME_STATES).toContain('unknown');
  });

  it('has exactly 5 members', () => {
    expect(RUNTIME_STATES).toHaveLength(5);
  });
});

describe('VIEWPORT_STATES wire strings', () => {
  it('contains all five states', () => {
    expect(VIEWPORT_STATES).toContain('focused');
    expect(VIEWPORT_STATES).toContain('visible');
    expect(VIEWPORT_STATES).toContain('hidden');
    expect(VIEWPORT_STATES).toContain('minimized');
    expect(VIEWPORT_STATES).toContain('unknown');
  });

  it('has exactly 5 members', () => {
    expect(VIEWPORT_STATES).toHaveLength(5);
  });
});

describe('ORIGINS wire strings', () => {
  it('contains engine', () => {
    expect(ORIGINS).toContain('engine');
  });

  it('contains editor-backend (kebab-case)', () => {
    expect(ORIGINS).toContain('editor-backend');
  });

  it('does not contain camelCase variant', () => {
    expect(ORIGINS).not.toContain('editorBackend');
  });

  it('has exactly 2 members', () => {
    expect(ORIGINS).toHaveLength(2);
  });
});

describe('KINDS wire strings', () => {
  it('contains request, response, event', () => {
    expect(KINDS).toContain('request');
    expect(KINDS).toContain('response');
    expect(KINDS).toContain('event');
  });

  it('has exactly 3 members', () => {
    expect(KINDS).toHaveLength(3);
  });

  it('does not contain error as a kind (error is a response payload, not a kind)', () => {
    expect(KINDS).not.toContain('error');
  });
});
