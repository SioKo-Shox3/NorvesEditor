# CLAUDE.md — NorvesEditor working agreement

Read on every session. This file encodes **how we work**, not the full spec.
Keep it short. The detailed rules live in `docs/agent-guide/`; link to them, do
not inline them here. When a plan exposes a wrong assumption, fix it *here* (and
in `AGENTS.md`) so the next session inherits the correction.

> `CLAUDE.md` and `AGENTS.md` are kept **byte-identical**. Claude reads
> `CLAUDE.md`, Codex reads `AGENTS.md`. Edit one, mirror the other in the same
> change. These two files are the only top-level instructions; nothing else
> overrides them.

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
  first. See `docs/agent-guide/memory-buffer.md`.

## Workflow (how every non-trivial change happens)

The main session is the **orchestrator**. It splits work, decides design,
sequences phases, integrates, verifies, and owns branch/commit boundaries — and
**delegates implementation to subagents; it does not write implementation
itself.** The orchestrator does not author the detailed phase plan or review code
it supervised; those go to subagents.

1. **Research** (subagent, read-only) — understand the code/conventions first.
2. **Plan** (subagent) — a concrete, reviewable phase plan.
3. **Plan review** (a *different* subagent) — boundaries, ownership, lifetime,
   thread safety, permissions, protocol compatibility, verification.
4. **User approval** — present the reviewed plan and get the user's OK **before
   writing code**.
5. **Implement** (subagent) — execute the approved plan only.
6. **Implementation review** (a subagent that is **not** the implementer — the
   author never grades their own work) — real diff vs approved plan.
7. **Integrate / verify / commit** (orchestrator) — run the gates, commit on a
   work branch, merge.

Show evidence, not assertions: paste the command and its real output / the diff.
Defined subagents live in `.claude/agents/` (researcher, planner, plan-reviewer,
implementer, impl-reviewer, verifier). Full rules:
`docs/agent-guide/orchestration.md`.

## Model policy (who thinks, who types)

- **Main session / orchestrator: Opus (`claude-opus-4-8`).** Keep the strongest
  model where being wrong is most expensive.
- **Quality on Opus:** planner, plan-reviewer, impl-reviewer, verifier.
- **Volume on Sonnet (`claude-sonnet-4-6`):** researcher, implementer.
- **Escalate the implementer to Opus** for load-bearing/high-risk work: protocol
  schema/compatibility, Tauri process/security permissions, Rust async task
  lifecycle, WebSocket transport/reconnect, C++ SDK public API, buffer/memory
  ownership, thread affinity, NorvesLib adapter, viewport strategy.
- Model availability drifts; check `/model` and update these strings if it
  changes. Detail: `docs/agent-guide/orchestration.md`.

## Quality gates (must pass before a phase is "Done", with evidence)

```powershell
# Protocol fixtures
python scripts/validate-bridge-fixtures.py
# Rust
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
# C++ engine SDK / mock engine
ctest --test-dir build/cpp -C Debug --output-on-failure
```

Prefer `scripts/verify.ps1` once it exists. Run only the gates relevant to what
changed, and say which you ran. Never commit generated/build output
(`node_modules`, Cargo `target/`, CMake build dirs, Tauri generated schema,
`__pycache__`). Detail: `docs/agent-guide/build-and-verify.md`.

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
`memory-buffer`, `rust`, `typescript`, `cpp`, `norveslib-adapter`,
`tauri-security`, `coding-style`, `branching-and-commits`, `orchestration`,
`build-and-verify`, `external-references`.
