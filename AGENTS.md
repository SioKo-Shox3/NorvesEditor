# CLAUDE.md — NorvesEditor working agreement

Read on every session. This file encodes **how we work**, not the full spec.
Keep it short. The detailed rules live in `docs/agent-guide/`; link to them, do
not inline them here. When a plan exposes a wrong assumption, fix it *here* (and
in `AGENTS.md`) so the next session inherits the correction.

> `CLAUDE.md` and `AGENTS.md` are kept **byte-identical**. Claude reads
> `CLAUDE.md`, Codex reads `AGENTS.md`. Edit one, mirror the other in the same
> change. These two files are the only top-level instructions; nothing else
> overrides them. Under Claude Code a PostToolUse mirror-guard hook detects
> drift; under Codex a Stop hook (`.codex/hooks/mirror-guard.mjs`, wired
> 2026-07-07) blocks finishing the turn while they drift — but Codex hooks are
> **silently inert** without trust entries in `~/.codex/config.toml`. Hooks are
> a net: after editing either file, immediately `cp` + `diff` the mirror
> yourself as part of completing that edit.

## Language

Converse with the user in **Japanese** — explanations, plans, reviews, risks,
questions, proposals, and **commit messages** all in Japanese, even when prompted
in English. Switch only if the user explicitly asks.

Keep **English** for code, identifiers, API/type names, commands, file paths,
error messages, code fragments, and these governance files. Other artifacts
(docs, comments) follow the existing convention of their layer. Never force-
translate technical terms.

## What this project is

NorvesEditor is a Tauri / Rust / TypeScript desktop game editor. The first alpha
launches/attaches to an external C++ engine process, connects over a Bridge
(WebSocket + JSON control channel), and drives it from a Game View panel
(launch / stop / reconnect / play / pause / stop / focus, plus status & logs).
No native viewport embedding in alpha — the engine owns its own window.

`NorvesBridge` lives here as the `bridge/` subsystem, not a separate repo.
`NorvesLib` is the first reference engine; the generic bridge / C++ SDK must
never depend on it. Full plan: `docs/alpha-project-plan.md`,
`docs/vision.md`. Architecture detail: `docs/agent-guide/architecture.md`.

## Architecture boundaries (do not cross)

- `bridge/` is a layer inside the editor, **separate from UI**. The C++ engine
  SDK must not depend on Tauri / TypeScript / React or NorvesLib. The generic
  bridge must not include NorvesLib headers.
- UI never touches a raw WebSocket; it goes through Tauri command/event wrappers.
  The **Rust backend owns** the Bridge connection and process lifecycle.
- Engine live memory is never sent over the transport — convert to snapshots/DTOs
  first. See `docs/memory-buffer-policy.md`.

## Workflow (how every non-trivial change happens)

The main session is the **orchestrator**. It splits work, decides design,
sequences phases, integrates, verifies, and owns branch/commit boundaries — and
**delegates implementation to subagents; it does not write
implementation itself.** The orchestrator does not author the detailed phase
plan or review code it supervised; those go to subagents.

**Scope of this section (revised 2026-07-11):** the workflow steps and the
declaration / scope-check / advisor rituals are **orchestrator-only**. An agent
spawned as a subagent (explicitly or via autonomous delegation) follows only
these 3 rules: (1) stay inside the assigned write paths — if an out-of-scope
change becomes necessary, report back instead of editing; (2) run the
verification commands and return real output; (3) stop after 2 failures of the
same approach and return the evidence to the parent. Subagents never write the
5-line declaration, never run check-scope.mjs, never call ask-advisor — the
orchestrator verifies the INTEGRATED diff once with check-scope.mjs (measured
2026-07-11: children running their own declarations/checks multiplied ritual
cost fleet-wide).

**Size triage — once, before starting (over-processing is a cost accident;
added 2026-07-12).** The numbered steps below are the STANDARD path, not a tax
on every task. Classify first, declare the path, escalate one tier when unsure:

- **LIGHT** — local, reversible, small: ≤2 files, ~≤50 changed lines, no
  load-bearing area, no public API/schema/ownership change. → ONE `implementer`
  directly (no researcher/planner/test-designer; embed conventions +
  verification commands in the brief) → `verifier` gates → commit. The
  cross-AI second review MAY BE SKIPPED — state "light path" in the report.
- **STANDARD** — beyond the LIGHT thresholds → steps 1-7 below, but the
  cross-AI second review runs ONCE on the task's integrated diff, not per phase.
- **HEAVY** — load-bearing areas, public API, data formats, concurrency,
  ownership → all gates + heavy-artillery on load-bearing phases.

When the user asks for speed, default to LIGHT and say so — never gold-plate
silently.

1. **Research** (subagent, read-only) — understand the code/conventions first.
2. **Plan** (subagent) — a concrete, reviewable phase plan.
3. **Plan review** (a *different* subagent) — boundaries, ownership, lifetime,
   thread safety, permissions, protocol compatibility, verification — plus an
   independent **Codex second review via direct CLI** (double check) before
   approval.
4. **User approval** — present the reviewed plan and get the user's OK **before
   writing code**.
5. **Implement** — the orchestrator never types code. Implementation goes to
   the **`implementer` subagent** (higher model for load-bearing areas;
   include the no-loop clause: stop after 2 failed attempts of one approach
   and report back). **Cross-CLI implementation handoff is abolished
   (2026-07-12)** — the partner AI only does second reviews and
   consultations. Hand over the phase goal, allowed/forbidden write paths,
   layer conventions, and the expected report; treat the output as a
   *proposal* — check `git diff --stat` for scope creep before accepting.
   Plugin-based delegation stays banned (2026-07-05).
6. **Implementation review** (double, mandatory gate) — a top-model
   `impl-reviewer` subagent that is **not** the implementer (the author never
   grades their own work) checks the real diff vs the approved plan, plus an
   independent **Codex second review via direct CLI**
   (`codex exec --sandbox read-only ...`, synchronous — never via the codex
   plugin) in a clean context. Reconcile both before proceeding; if the Codex
   CLI call fails, substitute the `verifier` refute pass and report the
   skipped gate.
7. **Integrate / verify / commit** (orchestrator) — run the gates, commit on a
   work branch, merge.

Show evidence, not assertions: paste the command and its real output / the diff.
Defined subagents live in `.claude/agents/` (researcher, planner, plan-reviewer,
implementer, impl-reviewer, verifier). Full rules:
`docs/agent-guide/orchestration.md`.

## Model policy (who thinks, who types)

- **Main session / orchestrator: the top model available** (pick via `/model`;
  version-pinned model IDs are deliberately not written here). Keep the
  strongest model where being wrong is most expensive.
- **Quality follows the main session (`model: inherit`, set 2026-07-04):**
  planner, plan-reviewer, impl-reviewer, verifier always run on whatever the
  orchestrator runs — model generation changes need no edits here. If the main
  session is deliberately run cheap, spawn quality agents with the top-tier
  alias explicitly.
- **Implementation goes to the `implementer` subagent** (see Workflow step 5;
  escalate its spawn model for load-bearing areas). Cross-CLI implementation
  handoff is abolished (2026-07-12). Research and mechanical work stay on
  Sonnet or cheaper.
- **Escalate Claude-side implementation to the top model** for
  load-bearing/high-risk work: protocol schema/compatibility, Tauri
  process/security permissions, Rust async task lifecycle, WebSocket
  transport/reconnect, C++ SDK public API, buffer/memory ownership, thread
  affinity, NorvesLib adapter, viewport strategy.
- Model availability drifts; check `/model` and update these strings if it
  changes. Detail: `docs/agent-guide/orchestration.md`.

## Dual-main operation (set 2026-07-04)

- **The orchestrator is whichever CLI the user launched.** Claude Code reads
  `CLAUDE.md`, Codex reads `AGENTS.md` (identical mirror) — no switching action
  exists; instruct Codex and Codex is the main under this same agreement.
- **The second review always goes to the non-main AI** (Claude main → Codex
  second; Codex main → Claude second), guaranteeing a cross-vendor check —
  called via **direct CLI** (`codex exec` / `claude -p`), never via plugins.
- **When Codex is main:** orchestrate with the `.codex/agents/` pod; use Claude
  headless for consultation and second reviews, e.g.
  `claude -p "<brief>" --model opus --permission-mode plan` (read-only), or
  hand it `.claude/agents/impl-reviewer.md` to adopt for a review.
  **Measured 2026-07-06: `claude -p` fails inside Codex's sandbox (network
  blocked) — run it via the "outside the sandbox" approval in interactive
  sessions; it cannot be called from non-interactive `codex exec`.**
- **Mutual help:** after 2 refutes/reworks on one phase, consult the partner AI
  with a structured brief before burning more attempts. Advisory only — the
  main decides.

## Harness enforcement

(Claude-main hooks below; Codex-main equivalents live in `.codex/hooks.json` —
an `apply_patch` implementation guard, a Stop mirror-guard, and a SessionStart
reminder (wired 2026-07-07). Codex hooks fire only once trusted in
`~/.codex/config.toml` [hooks.state]; re-trust via
`.codex/hooks/trust-hooks.mjs` after any hooks.json change.)

- A `PreToolUse` guard (`.claude/hooks/enforce-codex-impl.mjs`, wired in
  `.claude/settings.local.json`) **blocks** main-thread `Edit`/`Write` of
  implementation source (`.rs/.ts/.tsx/.js/.cpp/.h` etc.) so implementation is
  physically routed to the `implementer` subagent. Docs, config, and protocol
  fixtures are not blocked.
- A `SessionStart` hook re-surfaces this policy at the top of every session.
- **Never create a new workspace (git worktree add) without user approval**
  (Codex-main: `.codex/hooks/workspace-guard.mjs` blocks it — added 2026-07-12).
  A fresh tree carries a STALE mirror snapshot and its hooks.json path has no
  trust entries, so the harness goes silently inert (measured). If isolation is
  genuinely needed, re-establish the harness (fresh mirrors + trust-hooks.mjs +
  session_start probe) as part of the same task. Approved override:
  `NORVESEDITOR_ALLOW_WORKSPACE=1`.
- Deliberate one-session override (rare, user-approved only): relaunch with
  `NORVESEDITOR_ALLOW_DIRECT_EDIT=1`.
- `settings.local.json` is machine-local; redeploy from
  `../claude-workflow-template` (outside this repo) on other machines.

## Quality gates (must pass before a phase is "Done", with evidence)

Use `scripts/verify.ps1` as the preferred aggregate runner:

```powershell
./scripts/verify.ps1          # fixtures + Rust (C++ skipped unless -Cpp)
./scripts/verify.ps1 -Cpp     # also run cmake/ctest (requires build/cpp configured)
```

Individual gates for reference:

```powershell
# Protocol fixtures
python scripts/validate-bridge-fixtures.py
# Rust (root workspace = bridge crates only; apps/editor/src-tauri is a separate workspace)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
# C++ engine SDK / mock engine (after cmake configure)
ctest --test-dir build/cpp -C Debug --output-on-failure
# Frontend (pnpm -r --if-present tolerates zero packages)
pnpm -r --if-present typecheck
```

Run only the gates relevant to what changed, and say which you ran. Never commit
generated/build output (`node_modules`, Cargo `target/`, CMake build dirs, Tauri
generated schema, `__pycache__`). `pnpm-lock.yaml` and `Cargo.lock` ARE
committed. Detail: `docs/agent-guide/build-and-verify.md`.

## Branches & commits

- Never commit directly to `main`. Branch per logical theme
  (`feature/…`, `fix/…`, `docs/…`, `chore/…`, `refactor/…`, `spike/…`), keep one
  theme per branch, integrate via PR/merge.
- Commits: Japanese, single logical change, imperative subject that names the
  target and intent (not just "修正"/"更新"). Add a body for non-trivial changes,
  and **always** for: protocol schema/fixtures, Tauri permissions, process
  launch/kill, Bridge public API, C++ ownership/buffer/thread, Rust async
  lifecycle, NorvesLib adapter, viewport strategy.
- End commit messages with the required `Co-Authored-By` trailer.

Detail: `docs/agent-guide/branching-and-commits.md`.

## Reference map (`docs/agent-guide/`)

Read the one relevant to your layer/phase before working in it — see
`docs/agent-guide/README.md` for the index: `architecture`, `protocol-schema`,
`rust`, `typescript`, `cpp`, `norveslib-adapter`,
`tauri-security`, `coding-style`, `branching-and-commits`, `orchestration`,
`build-and-verify`, `external-references`.
