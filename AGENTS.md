# Project Context

Stack: **Rust** (workspace, edition 2021+). Target Alpine Linux / musl. Decisions: `docs/architecture/overview.md`.

- Backend daemon: `ursined` (tokio, unix socket, JSON lines)
- Desktop GUI: `ursine` (GTK4 + libadwaita via relm4/gtk4-rs — Wayle shell stack family)
- Web: static assets in `web/`, served by daemon; framework TBD (any JS, zero server logic)
- Shared crates: `ursine-api` (types), `ursine-core` (modules)
- Layout (binding): code `src/` (members `src/ursine-api`, `src/ursine-core`, `src/ursined`, `src/ursine`), tests `tests/`, generated `artifacts/` (contents gitignored, folder tracked via `.gitkeep`). **Build output in `artifacts/target`** — `.cargo/config.toml` sets `target-dir`; no `target/` at root. Temp files: `artifacts/<tool-or-task>/` only, never host `/tmp`

## Workflow

**Chain (complex)**: @orchestrator → @planner (plan→`state/session-state.json`) → approve → @codebase (reads session state, impl) → Pre-Commit Review Gate (Pattern 8: freeze diff → Tier 1 `cargo build`+`test`+`clippy -D warnings`+`fmt --check` → Tier 2 parallel @review lens briefs `requirements|code|security|ux-responsive` on frozen diff, triage, max 2 cycles) → @debugger → @docs → save summary. @review security lens loads `security-audit`.

**Skill-chain (simple)**: 0 context+recall → 1 interview-me / idea-refine → 2 spec → 3 plan (+decision-grounding) → 4 code-change-impact blast-radius, then implement (incremental-implementation | subagent-driven-development) → 4.1 parallel(@review @code-reviewer on git diff) → 5 test-driven-development → 6 debugging → 7 code-review-and-quality + security-and-hardening + performance-optimization (parallel) → 8 code-simplification → 9 grill-with-docs (+adr for architecture decisions) → 10 git-workflow-and-versioning → 11 ci-cd-and-automation → 12 deprecation-and-migration → 13 save summary.

**Direct** (simple task): @codebase or specialist direct.

- Parallel: independent features, agents-in-step, review+tests, policy. Never parallel: tests (need code first).
- Session: mandatory — start `memory.sh context` + read repo files; end `memory.sh save` + update files (see Session protocol).
- Git: conventional `type(scope):desc`; pre-commit: build+tests+lint.

State: `state/session-state.json`.

## Session protocol (memory.sh + repo files)

`session-memory` skill works here via bash (`memory.sh`, not the .ps1). Session memory = skill memory (`.agent-memory/` JSONL) + repo state files. Update both at session start and end:

```bash
# Session START — read, in order:
.agents/skills/session-memory/scripts/memory.sh context   # recent memories
1. handoff/latest.md             # where we left off, next steps
2. state/session-state.json      # goals, decisions, risks
3. docs/architecture/overview.md # all architecture decisions (do not violate)
4. CONTEXT.md                    # domain vocabulary

# Session END — update:
.agents/skills/session-memory/scripts/memory.sh save "summary" "Today: X, next: Y"
1. handoff/latest.md             # new session entry: done, decisions, next steps
2. state/session-state.json      # next_actions, key_decisions, risks
3. docs/TASKS.md                 # sprint checkboxes
4. CHANGELOG.md                  # user-visible changes only
```

## Stack commands

```bash
cargo build --workspace          # build
cargo test --workspace           # test
cargo clippy --workspace -- -D warnings   # lint (CI gate)
cargo fmt --check                # format check
```

`pam` = cargo feature (no libpam headers on dev machine). Dev: `cargo build` (stub auth). Release/FeralOS: `cargo build --features pam`.

## Host isolation (ABSOLUTE RULE)

Host machine NEVER modified by agents or scripts: no installs, no nix gc/profile changes, no global tooling, no system commands. All build/test/dedup/security/dependency checks and maintenance inside Docker containers (alpine = cargo, nixos/nix = flake). Container mounts: project root READ-ONLY, only `artifacts/` writable. Temp files: `artifacts/<tool-or-task>/` only, never host `/tmp`. Retention: last 3 artifact versions per subfolder (`scripts/clean-artifacts.sh`). Nix store GC: via nixos/nix container, NEVER `-d` (deletes host profile generations).

Repo content: ENGLISH only (code, docs, scripts, comments, commits).

## Git hooks (installed)

```bash
git config core.hooksPath .githooks   # already run on this clone
```

Pre-commit (template + Ursine): secrets/PII scan + `cargo fmt --check` on staged Rust code.

## Agents

### User-created
- `researcher`
- `information-scan`
- `vulnerability-scan`

## Skills

- `.opencode/` — full agents-opencode harness: `agents/` (20), `commands/`, `instructions/` (incl. `rust.instructions.md`), `plugins/agents-opencode.ts`, `skills/` (incl. `rust`), `rules/` (5 template + `ursine.md` project invariants)
- `.agents/skills/` — universal workflow skills present (idea-refine, spec-driven-development, interview-me, planning-and-task-breakdown, incremental-implementation, test-driven-development, security scanning, …)
- `session-memory` — bash version works (`memory.sh`, see Session protocol); .ps1 dead here
- `skills-lock.json` — valid, matches `.agents/skills/`; update via `npx skills update`

### User-created
None.

External skills:
- `customize-opencode` → built-in

## Docs

- `docs/architecture/overview.md` — **all architecture decisions + rationale (read first)**
- `docs/PROJECT.md` — vision, users, scope, risks
- `docs/agents-guide.md` — agents and daily workflow
- `docs/TASKS.md` — current sprint
- `docs/global-setup.md` — plugin paths, opencode environment
- `docs/update.md` — update commands for all components

## Deferred (stack decided — remaining)

GUI web asset toolchain decision (when web/ starts), CI activation (uncomment ci.yml), APKBUILDs in feralos-aports (v1.0).
