#!/usr/bin/env node
// PreToolUse guard (NorvesEditor working agreement).
//
// CLAUDE.md model policy: implementation is delegated to Codex by default.
// The main thread PLANS, INTEGRATES, and REVIEWS; Codex writes the code
// (via the `codex` plugin / `codex:rescue`) — "delegate the typing, never
// the judgment."
//
// Text in CLAUDE.md alone does not hold (proven on Sembazuru: a session typed
// load-bearing code directly), so this hook makes the harness enforce it: an
// Edit/Write/MultiEdit/NotebookEdit targeting implementation source is
// BLOCKED, with instructions to route the change through Codex instead.
//
// Deliberate, operator-approved override for one session: set the environment
// variable NORVESEDITOR_ALLOW_DIRECT_EDIT=1 before launching Claude Code.
//
// Exit codes: 2 => block the tool call and surface stderr to the model; 0 =>
// allow. Any internal error fails OPEN (exit 0) so the guard can never wedge
// the session — its job is to redirect, not to brick the tool.

import process from "node:process";

// ---- per-project configuration -------------------------------------------
const PROJECT = "NorvesEditor";
const OVERRIDE_ENV = "NORVESEDITOR_ALLOW_DIRECT_EDIT";
// Implementation source the main thread must not TYPE: Rust, TypeScript/JS
// (UI), and the C++ SDK. Docs (.md), config (.json/.toml/.yaml), and protocol
// fixtures are NOT blocked — the main thread legitimately edits those.
const IMPL_SOURCE_RE = /\.(rs|ts|tsx|js|jsx|cpp|cc|cxx|c|hpp|hh|hxx|h)$/i;
// ---------------------------------------------------------------------------

let raw = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => {
  raw += chunk;
});
process.stdin.on("end", () => {
  try {
    const data = JSON.parse(raw || "{}");
    const ti = (data && data.tool_input) || {};
    const filePath = ti.file_path || ti.notebook_path || "";

    // A deliberate, operator-approved escape hatch (one session).
    const override = /^(1|true|yes|on)$/i.test(
      process.env[OVERRIDE_ENV] || "",
    );
    if (override) {
      process.exit(0);
    }

    if (IMPL_SOURCE_RE.test(filePath)) {
      process.stderr.write(
        `BLOCKED by the ${PROJECT} workflow guard: the main thread must NOT ` +
          "type implementation code directly.\n\n" +
          "CLAUDE.md model policy: implementation is delegated to Codex; the " +
          "main thread plans, integrates, and REVIEWS. Every non-trivial " +
          "change is a Codex + Claude double-review.\n\n" +
          `Refused edit: ${filePath}\n\n` +
          "Do this instead:\n" +
          "  1. Hand the change to Codex (Skill `codex:rescue`) with the " +
          "design/spec and the exact files + invariants to implement.\n" +
          "  2. Review Codex's diff (git diff --stat first, then line-by-line); " +
          "run an independent Claude review pass (impl-reviewer / verifier).\n" +
          "  3. Only then stage + commit.\n\n" +
          "Deliberate, user-approved override for ONE session: relaunch " +
          `Claude Code with env ${OVERRIDE_ENV}=1.\n`,
      );
      process.exit(2);
    }

    process.exit(0);
  } catch {
    // Fail open: never let a guard bug wedge the tool.
    process.exit(0);
  }
});
