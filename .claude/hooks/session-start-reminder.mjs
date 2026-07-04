#!/usr/bin/env node
// SessionStart hook (NorvesEditor working agreement): surface the
// model-policy workflow at the top of every session so it is never just buried
// in CLAUDE.md. It also tells the agent that the PreToolUse guard will
// physically block main-thread code edits, so the agent routes implementation
// through Codex from the start instead of discovering the wall mid-task.
//
// Emits the reminder as additionalContext. Fails open (exit 0, no context) on
// any error so it can never wedge session startup.

import process from "node:process";

const context = [
  "NorvesEditor workflow policy (ENFORCED by hooks, not just CLAUDE.md):",
  "- The main session is the ORCHESTRATOR: it splits phases, decides, integrates, and reviews — it does not write implementation itself.",
  "- Implementation runs in the `implementer` SUBAGENT — Claude-only by default (set 2026-07-05); do not delegate implementation to Codex via plugin. The main thread plans, integrates, and reviews; it never types code.",
  "- A PreToolUse guard BLOCKS main-thread Edit/Write of implementation source (.rs/.ts/.tsx/.js/.cpp/.h). Do NOT try to type code directly — hand it to the implementer subagent.",
  "- Every non-trivial change gets a DOUBLE review: top-model Claude first review (plan-reviewer / impl-reviewer, never the author) + a MANDATORY independent Codex second review via DIRECT CLI: `codex exec --sandbox read-only ...` (synchronous, no polling). NEVER via plugin (`codex:rescue`) — Agent-hooking plugins (context-mode) break it. If the CLI call fails, fall back to impl-reviewer + verifier and REPORT the skipped gate.",
  "- Quality roles (planner, plan-reviewer, impl-reviewer, verifier) run on the top model; research/mechanical work runs cheap. Escalate on high-risk areas (protocol, Tauri permissions, async lifecycle, C++ SDK API).",
  "- Show evidence, not assertions: paste commands and real output.",
  "- Deliberate one-session override (rare, user-approved only): relaunch with env NORVESEDITOR_ALLOW_DIRECT_EDIT=1.",
].join("\n");

try {
  process.stdout.write(
    JSON.stringify({
      hookSpecificOutput: {
        hookEventName: "SessionStart",
        additionalContext: context,
      },
    }),
  );
} catch {
  // fail open
}
process.exit(0);
