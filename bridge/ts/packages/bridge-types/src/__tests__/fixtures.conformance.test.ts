/**
 * Fixture <-> TS DTO conformance check (plan deliverable M3).
 *
 * Strategy: vitest runtime validation.
 *
 * Why vitest instead of compile-time `satisfies`:
 *   JSON imports via `import fixture from '...' assert { type: 'json' }` widen
 *   string literals to `string`. For example, `{ engineState: "running" }` would
 *   have type `{ engineState: string }`, making `satisfies GetStatusResult`
 *   fail at compile time even for a valid fixture (because `string` is not
 *   assignable to the narrow `EngineState` union). A `as const` assertion on
 *   a JSON import is also not valid TypeScript.
 *
 *   Instead, we read fixture files at runtime with fs/path (Node / Vitest context)
 *   and validate them structurally using the exported const enum arrays
 *   (ENGINE_STATES, RUNTIME_STATES, etc.). This catches all meaningful drift:
 *   - A required field missing from a fixture -> assertion fails.
 *   - An enum value not in the TS union -> assertion fails.
 *   - An unknown field added to a fixture -> we check the known fields are present
 *     (forward-compat: extra fixture fields are allowed by the schema).
 *
 * Negative proofs (compile-time `@ts-expect-error`):
 *   Placed below the vitest suite. They prove the DTOs actually bite at type-check
 *   time. They are type-level assertions only; they do not produce runtime tests.
 *
 * Gate:
 *   `pnpm --filter @norves/bridge-types test` runs vitest which includes this file.
 */

import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import {
  ENGINE_STATES,
  RUNTIME_STATES,
  LOG_LEVELS,
  VIEWPORT_STATES,
  ORIGINS,
} from '../index.js';

// -------------------------------------------------------------------------
// Fixture loader helper
// -------------------------------------------------------------------------

// Fixtures live at <repo-root>/bridge/spec/fixtures/...
// __dirname in ESM vitest is not available; use import.meta.url.
// This file is at: bridge/ts/packages/bridge-types/src/__tests__/
// Traversing 6 levels up (../../../../../../) reaches repo root.
const FIXTURES_ROOT = new URL(
  '../../../../../../bridge/spec/fixtures',
  import.meta.url,
).pathname.replace(/^\/([A-Za-z]:)/, '$1'); // Fix Windows path: /C:/... -> C:/...

function loadFixture(relPath: string): unknown {
  const abs = join(FIXTURES_ROOT, relPath);
  const raw = readFileSync(abs, 'utf-8');
  return JSON.parse(raw) as unknown;
}

function paramsOf(fixture: unknown): Record<string, unknown> {
  const f = fixture as { params?: Record<string, unknown> };
  if (f.params === null || typeof f.params !== 'object') {
    throw new Error(`Fixture has no params object: ${JSON.stringify(fixture)}`);
  }
  return f.params as Record<string, unknown>;
}

function resultOf(fixture: unknown): Record<string, unknown> {
  const f = fixture as { result?: Record<string, unknown> };
  if (f.result === null || typeof f.result !== 'object') {
    throw new Error(`Fixture has no result object: ${JSON.stringify(fixture)}`);
  }
  return f.result as Record<string, unknown>;
}

// -------------------------------------------------------------------------
// events/engine.statusChanged -- EngineStatusChangedEvent
// -------------------------------------------------------------------------

describe('events/engine.statusChanged positive fixtures', () => {
  it('event-engine-valid: engineState is a known EngineState', () => {
    const f = loadFixture('events/engine.statusChanged/positive/event-engine-valid.json');
    const params = paramsOf(f);
    expect(params).toHaveProperty('engineState');
    expect(ENGINE_STATES).toContain(params['engineState']);
  });

  it('event-engine-valid: runtimeState is a known RuntimeState when present', () => {
    const f = loadFixture('events/engine.statusChanged/positive/event-engine-valid.json');
    const params = paramsOf(f);
    if (params['runtimeState'] !== undefined) {
      expect(RUNTIME_STATES).toContain(params['runtimeState']);
    }
  });
});

// -------------------------------------------------------------------------
// events/runtime.stateChanged -- RuntimeStateChangedEvent
// -------------------------------------------------------------------------

describe('events/runtime.stateChanged positive fixtures', () => {
  it('event-engine-valid: state is a known RuntimeState', () => {
    const f = loadFixture('events/runtime.stateChanged/positive/event-engine-valid.json');
    const params = paramsOf(f);
    expect(params).toHaveProperty('state');
    expect(RUNTIME_STATES).toContain(params['state']);
  });

  it('event-engine-no-previous: state present, no previous required', () => {
    const f = loadFixture('events/runtime.stateChanged/positive/event-engine-no-previous.json');
    const params = paramsOf(f);
    expect(params).toHaveProperty('state');
    expect(RUNTIME_STATES).toContain(params['state']);
  });
});

// -------------------------------------------------------------------------
// events/log.message -- LogMessageEvent
// -------------------------------------------------------------------------

describe('events/log.message positive fixtures', () => {
  it('event-engine-valid: level is a known LogLevel and message is a string', () => {
    const f = loadFixture('events/log.message/positive/event-engine-valid.json');
    const params = paramsOf(f);
    expect(params).toHaveProperty('level');
    expect(LOG_LEVELS).toContain(params['level']);
    expect(params).toHaveProperty('message');
    expect(typeof params['message']).toBe('string');
  });
});

// -------------------------------------------------------------------------
// events/bridge.connected -- BridgeConnectedEvent
// -------------------------------------------------------------------------

describe('events/bridge.connected positive fixtures', () => {
  it('event-synthesized-valid: endpoint is a string and origin (if present) is known', () => {
    const f = loadFixture('events/bridge.connected/positive/event-synthesized-valid.json');
    const params = paramsOf(f);
    expect(params).toHaveProperty('endpoint');
    expect(typeof params['endpoint']).toBe('string');
    if (params['origin'] !== undefined) {
      expect(ORIGINS).toContain(params['origin']);
    }
  });
});

// -------------------------------------------------------------------------
// events/bridge.disconnected -- BridgeDisconnectedEvent
// -------------------------------------------------------------------------

describe('events/bridge.disconnected positive fixtures', () => {
  it('event-synthesized-valid: reason is a string', () => {
    const f = loadFixture('events/bridge.disconnected/positive/event-synthesized-valid.json');
    const params = paramsOf(f);
    expect(params).toHaveProperty('reason');
    expect(typeof params['reason']).toBe('string');
  });
});

// -------------------------------------------------------------------------
// events/error.reported -- ErrorReportedEvent
// -------------------------------------------------------------------------

describe('events/error.reported positive fixtures', () => {
  const cases = [
    'events/error.reported/positive/event-engine-valid.json',
    'events/error.reported/positive/event-synthesized-valid.json',
  ];

  for (const path of cases) {
    it(`${path}: error.code and error.message are strings`, () => {
      const f = loadFixture(path);
      const params = paramsOf(f);
      expect(params).toHaveProperty('error');
      const err = params['error'] as Record<string, unknown>;
      expect(typeof err['code']).toBe('string');
      expect(typeof err['message']).toBe('string');
    });
  }
});

// -------------------------------------------------------------------------
// events/engine.processExited -- EngineProcessExitedEvent
// -------------------------------------------------------------------------

describe('events/engine.processExited positive fixtures', () => {
  it('event-synthesized-valid: exitCode is a number', () => {
    const f = loadFixture('events/engine.processExited/positive/event-synthesized-valid.json');
    const params = paramsOf(f);
    expect(params).toHaveProperty('exitCode');
    expect(typeof params['exitCode']).toBe('number');
    expect(Number.isInteger(params['exitCode'])).toBe(true);
  });
});

// -------------------------------------------------------------------------
// events/viewport.stateChanged -- ViewportStateChangedEvent
// -------------------------------------------------------------------------

describe('events/viewport.stateChanged positive fixtures', () => {
  it('event-engine-valid: state is a known ViewportState', () => {
    const f = loadFixture('events/viewport.stateChanged/positive/event-engine-valid.json');
    const params = paramsOf(f);
    expect(params).toHaveProperty('state');
    expect(VIEWPORT_STATES).toContain(params['state']);
  });

  it('event-engine-no-previous: state present, no previous required', () => {
    const f = loadFixture('events/viewport.stateChanged/positive/event-engine-no-previous.json');
    const params = paramsOf(f);
    expect(params).toHaveProperty('state');
    expect(VIEWPORT_STATES).toContain(params['state']);
  });
});

// -------------------------------------------------------------------------
// methods/engine.getStatus -- GetStatusResult (response params)
// -------------------------------------------------------------------------

describe('methods/engine.getStatus positive fixtures', () => {
  it('request-valid: params is empty object', () => {
    const f = loadFixture('methods/engine.getStatus/positive/request-valid.json');
    const params = paramsOf(f);
    // GetStatusParams = Record<string, never> -> empty
    expect(Object.keys(params)).toHaveLength(0);
  });
});

// -------------------------------------------------------------------------
// methods/bridge.hello -- HelloResult
// -------------------------------------------------------------------------

describe('methods/bridge.hello positive fixtures', () => {
  it('response-valid: result has sessionId, protocolVersion, server.name', () => {
    const f = loadFixture('methods/bridge.hello/positive/response-valid.json');
    const result = resultOf(f);
    expect(typeof result['sessionId']).toBe('string');
    expect(typeof result['protocolVersion']).toBe('string');
    const server = result['server'] as Record<string, unknown>;
    expect(typeof server['name']).toBe('string');
  });
});

// -------------------------------------------------------------------------
// methods/runtime.play -- PlayResult
// -------------------------------------------------------------------------

describe('methods/runtime.play positive fixtures', () => {
  const cases = [
    'methods/runtime.play/positive/response-valid.json',
    'methods/runtime.play/positive/response-minimal.json',
  ];
  for (const path of cases) {
    it(`${path}: accepted is boolean`, () => {
      const f = loadFixture(path);
      const result = resultOf(f);
      expect(typeof result['accepted']).toBe('boolean');
    });
  }

  it('response-valid: requestedState (if present) is a known RuntimeState', () => {
    const f = loadFixture('methods/runtime.play/positive/response-valid.json');
    const result = resultOf(f);
    if (result['requestedState'] !== undefined) {
      expect(RUNTIME_STATES).toContain(result['requestedState']);
    }
  });
});

// -------------------------------------------------------------------------
// methods/runtime.focusViewport -- FocusViewportResult
// -------------------------------------------------------------------------

describe('methods/runtime.focusViewport positive fixtures', () => {
  const cases = [
    'methods/runtime.focusViewport/positive/response-focused.json',
    'methods/runtime.focusViewport/positive/response-not-focused.json',
  ];
  for (const path of cases) {
    it(`${path}: focused is boolean`, () => {
      const f = loadFixture(path);
      const result = resultOf(f);
      expect(typeof result['focused']).toBe('boolean');
    });
  }
});

// -------------------------------------------------------------------------
// methods/scene.getTree -- SceneGetTreeResult (result = { root: SceneNode })
// -------------------------------------------------------------------------

describe('methods/scene.getTree positive fixtures', () => {
  it('request-valid: params carries optional rootId/maxDepth (both optional on the wire)', () => {
    const f = loadFixture('methods/scene.getTree/positive/request-valid.json');
    const params = paramsOf(f);
    // rootId and maxDepth are optional; the fixture exercises both present.
    expect(typeof params['rootId']).toBe('string');
    expect(typeof params['maxDepth']).toBe('number');
  });

  it('response-valid: result has a single root sceneNode with id', () => {
    const f = loadFixture('methods/scene.getTree/positive/response-valid.json');
    const result = resultOf(f);
    // Wire shape is { root }, not an array.
    expect(result).toHaveProperty('root');
    expect(Array.isArray(result['root'])).toBe(false);
    const root = result['root'] as Record<string, unknown>;
    expect(typeof root['id']).toBe('string');
    // Nested children recurse via the same sceneNode shape.
    const children = root['children'] as unknown[];
    expect(Array.isArray(children)).toBe(true);
    for (const child of children) {
      expect(typeof (child as Record<string, unknown>)['id']).toBe('string');
    }
  });

  it('response-minimal: root with only an id is valid (leaf, no children)', () => {
    const f = loadFixture('methods/scene.getTree/positive/response-minimal.json');
    const result = resultOf(f);
    const root = result['root'] as Record<string, unknown>;
    expect(typeof root['id']).toBe('string');
  });
});

// -------------------------------------------------------------------------
// Enum-widening guard (type-level, not runtime)
// Tests that EngineState/RuntimeState etc. are exactly the const array unions.
// -------------------------------------------------------------------------

// These compile-time checks live here as type assertions; they don't generate
// runtime test cases but are checked by tsc during typecheck.

import type { EngineState, RuntimeState, LogLevel, ViewportState, Origin } from '../index.js';

// Bidirectional type equality helper.
// Fails if A has a member not in B, OR B has a member not in A.
// Uses the conditional-type trick: (<T>() => T extends A ? 1 : 2) must be
// assignable to (<T>() => T extends B ? 1 : 2) in both directions for A == B.
type Equals<A, B> =
  (<T>() => T extends A ? 1 : 2) extends (<T>() => T extends B ? 1 : 2) ? true : false;

// Each assertion below MUST equal `true`; assigning to a `true` literal
// variable is the canonical compile-time enforcement pattern.
const _engineExact: Equals<EngineState, (typeof ENGINE_STATES)[number]> = true;
const _runtimeExact: Equals<RuntimeState, (typeof RUNTIME_STATES)[number]> = true;
const _logLevelExact: Equals<LogLevel, (typeof LOG_LEVELS)[number]> = true;
const _viewportExact: Equals<ViewportState, (typeof VIEWPORT_STATES)[number]> = true;
const _originExact: Equals<Origin, (typeof ORIGINS)[number]> = true;

// Suppress "unused variable" warnings — values are used only at type-check time.
void _engineExact; void _runtimeExact; void _logLevelExact; void _viewportExact; void _originExact;

// -------------------------------------------------------------------------
// Negative proofs: DTO constraints must bite
// These @ts-expect-error lines prove the types actually enforce shape/enums.
// -------------------------------------------------------------------------

import type { GetStatusResult, HelloResult, SceneGetTreeResult } from '../index.js';

function _negativeProofs(): void {
  // SceneGetTreeResult must require a `root` node.
  // @ts-expect-error missing root
  const _badScene1: SceneGetTreeResult = {};

  // root must be a single SceneNode object, not an array.
  // @ts-expect-error root is an array, not a SceneNode
  const _badScene2: SceneGetTreeResult = { root: [{ id: 'n-0' }] };

  // A SceneNode must have an id.
  // @ts-expect-error root node missing required id
  const _badScene3: SceneGetTreeResult = { root: { name: 'no id' } };

  void _badScene1; void _badScene2; void _badScene3;

  // Misspelled enum value must be rejected
  // @ts-expect-error 'runing' is not a valid EngineState
  const _bad1: GetStatusResult = { engineState: 'runing', runtimeState: 'edit' };

  // Missing required field (runtimeState) must be rejected
  // @ts-expect-error missing runtimeState
  const _bad2: GetStatusResult = { engineState: 'ready' };

  // Missing required field (engineState) must be rejected
  // @ts-expect-error missing engineState
  const _bad3: GetStatusResult = { runtimeState: 'edit' };

  // HelloResult missing server field
  // @ts-expect-error missing server
  const _bad4: HelloResult = { sessionId: 's', protocolVersion: '0.1' };

  // Wrong type for required field
  // @ts-expect-error number is not string
  const _bad5: GetStatusResult = { engineState: 42, runtimeState: 'edit' };

  // Completely empty object for GetStatusResult
  // @ts-expect-error missing both required fields
  const _bad6: GetStatusResult = {};

  void _bad1; void _bad2; void _bad3; void _bad4; void _bad5; void _bad6;
}

void _negativeProofs;
