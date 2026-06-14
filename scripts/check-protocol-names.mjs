#!/usr/bin/env node
// check-protocol-names.mjs
//
// Cross-language equality guard for Tauri IPC name constants.
//
// Reads:
//   bridge/ts/packages/bridge-ui/src/commands.ts  -> TS command values
//   bridge/ts/packages/bridge-ui/src/events.ts    -> TS event values
//   apps/editor/src-tauri/src/protocol_names.rs   -> Rust command + event values
//
// Asserts that the TS command-value set === Rust commands-module value set,
// and TS event-value set === Rust events-module value set.
//
// Exit 0 : in sync
// Exit 1 : drift detected (diff printed to stderr)

import { readFileSync } from 'fs';
import { fileURLToPath } from 'url';
import { dirname, join } from 'path';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const REPO_ROOT = join(__dirname, '..');

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Extract all single-quoted or double-quoted string literals from source. */
function extractStringLiterals(src) {
  const results = [];
  // Single-quoted strings (TS: 'foo_bar', 'bridge:event-name')
  const singleRe = /'([^'\\]+)'/g;
  // Double-quoted strings (Rust: "foo_bar", "bridge:event-name")
  const doubleRe = /"([^"\\]+)"/g;
  let m;
  while ((m = singleRe.exec(src)) !== null) results.push(m[1]);
  while ((m = doubleRe.exec(src)) !== null) results.push(m[1]);
  return results;
}

/**
 * Extract the string values that appear on the RIGHT side of the `= '...'`
 * assignments in a TS `as const` object, i.e. only the IPC name strings.
 *
 * Strategy: grab every quoted string that looks like an IPC name
 * (snake_case commands or bridge:-prefixed events) and deduplicate.
 * Key strings (camelCase logical keys) do not match either pattern.
 */
function extractTsValues(src) {
  const all = extractStringLiterals(src);
  // IPC names are either:
  //   - snake_case: only [a-z_] e.g. bridge_connect, get_status
  //   - bridge:kebab-case: starts with "bridge:" e.g. bridge:connection-state
  return new Set(
    all.filter((s) => /^[a-z][a-z_]*$/.test(s) || s.startsWith('bridge:'))
  );
}

/**
 * Extract Rust &str literal values from a specific module block.
 *
 * Strategy: locate the `pub mod <name> { ... }` block by brace counting,
 * then extract all double-quoted string literals within it.
 */
function extractRustModValues(src, modName) {
  // Find the start of `pub mod <modName> {`
  const modStart = src.indexOf(`pub mod ${modName} {`);
  if (modStart === -1) {
    throw new Error(`Rust: pub mod ${modName} not found in protocol_names.rs`);
  }

  // Walk forward, counting braces to find the closing `}`.
  let depth = 0;
  let i = modStart;
  let bodyStart = -1;
  while (i < src.length) {
    if (src[i] === '{') {
      depth++;
      if (depth === 1) bodyStart = i + 1;
    } else if (src[i] === '}') {
      depth--;
      if (depth === 0) {
        const body = src.slice(bodyStart, i);
        return new Set(extractStringLiterals(body));
      }
    }
    i++;
  }
  throw new Error(`Rust: unterminated pub mod ${modName}`);
}

// ---------------------------------------------------------------------------
// Load files
// ---------------------------------------------------------------------------

const commandsTsPath = join(REPO_ROOT, 'bridge/ts/packages/bridge-ui/src/commands.ts');
const eventsTsPath   = join(REPO_ROOT, 'bridge/ts/packages/bridge-ui/src/events.ts');
const rustPath       = join(REPO_ROOT, 'apps/editor/src-tauri/src/protocol_names.rs');

const commandsTsSrc = readFileSync(commandsTsPath, 'utf8');
const eventsTsSrc   = readFileSync(eventsTsPath,   'utf8');
const rustSrc       = readFileSync(rustPath,        'utf8');

// ---------------------------------------------------------------------------
// Extract value sets
// ---------------------------------------------------------------------------

const tsCommandValues = extractTsValues(commandsTsSrc);
const tsEventValues   = extractTsValues(eventsTsSrc);

const rustCommandValues = extractRustModValues(rustSrc, 'commands');
const rustEventValues   = extractRustModValues(rustSrc, 'events');

// ---------------------------------------------------------------------------
// Compare sets
// ---------------------------------------------------------------------------

function setDiff(a, b) {
  return { onlyInA: [...a].filter((x) => !b.has(x)), onlyInB: [...b].filter((x) => !a.has(x)) };
}

let ok = true;

function report(label, tsSet, rustSet) {
  const { onlyInA: onlyInTs, onlyInB: onlyInRust } = setDiff(tsSet, rustSet);
  if (onlyInTs.length === 0 && onlyInRust.length === 0) {
    console.log(`[OK]  ${label}: ${tsSet.size} name(s) in sync`);
    return;
  }
  ok = false;
  console.error(`[DRIFT] ${label}:`);
  if (onlyInTs.length > 0) {
    console.error(`  Only in TS   : ${onlyInTs.sort().join(', ')}`);
  }
  if (onlyInRust.length > 0) {
    console.error(`  Only in Rust : ${onlyInRust.sort().join(', ')}`);
  }
}

console.log('check-protocol-names: comparing TS <-> Rust IPC name constants...');
report('commands', tsCommandValues, rustCommandValues);
report('events',   tsEventValues,   rustEventValues);

if (!ok) {
  console.error('');
  console.error('[FAIL] TS and Rust IPC name constants are out of sync.');
  console.error('       Fix: update protocol_names.rs to match commands.ts / events.ts');
  console.error('       (or vice-versa), then re-run this script.');
  process.exit(1);
}

console.log('');
console.log('[PASS] All IPC name constants are in sync.');
