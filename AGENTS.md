# Project Context

Stack: **Rust** (workspace, edition 2021+). Target Alpine Linux / musl. Decisions: `docs/architecture/overview.md`.

- Backend daemon: `ursined` (tokio, unix socket, JSON lines)
- Desktop GUI: `ursine` (libcosmic — COSMIC toolkit, iced-based Rust UI; decision 2026-10-19: security over visual coherence with the GTK Wayle shell)
- Web: static assets in `src/web/`, served by daemon; framework TBD (any JS, zero server logic)
- Shared crates: `ursine-api` (types), `ursine-core` (modules)
- Layout (binding): code `src/` (members `src/ursine-api`, `src/ursine-core`, `src/ursined`, `src/ursine`; shipped tools `src/tools/`; web assets `src/web/`), tests `tests/`, generated `artifacts/` (contents gitignored, folder tracked via `.gitkeep`). **Build output in `artifacts/target`** — `.cargo/config.toml` sets `target-dir`; no `target/` at root. Temp files: `artifacts/<tool-or-task>/` only, never host `/tmp`

## Workflow

**Chain (complex)**: @orchestrator → @planner (plan→`state/session-state.json`) → approve → @codebase (reads session state, impl) → Pre-Commit Review Gate (Pattern 8: freeze diff → Tier 1 `cargo build`+`test`+`clippy -D warnings`+`fmt --check` → Tier 2 parallel @review lens briefs `requirements|code|security|ux-responsive` on frozen diff, triage, max 2 cycles) → @debugger → @docs → save summary. @review security lens loads `security-audit`.

**Skill-chain (simple)**: 0 context+recall → 1 interview-me / idea-refine → 2 spec → 3 plan (+decision-grounding) → 4 code-change-impact blast-radius, then implement (incremental-implementation | subagent-driven-development) → 4.1 parallel(@review @code-reviewer on git diff) → 5 test-driven-development → 6 debugging → 7 code-review-and-quality + security-and-hardening + performance-optimization (parallel) → 8 code-simplification → 9 grill-with-docs (+adr for architecture decisions) → 10 git-workflow-and-versioning → 11 ci-cd-and-automation → 12 deprecation-and-migration → 13 save summary.

**Direct** (simple task): @codebase or specialist direct.

**Plan traceability (binding)**: before proposing or implementing, read `docs/intent/ursine.md`, `ROADMAP.md`, `docs/planning/features-v1-v2.md`, and the relevant `docs/specs/SPEC-*.md`. Every feature must map to an explicit plan/spec line. Classify it as required, optional candidate, or deferred; never invent scope from a generic admin-tool comparison. A user decision overrides stale docs: update the plan/spec and record the decision before changing code. Current overrides: scanners are configuration-only (`SPEC-scanners`; no image scan), autologin is out of scope, and Services supports OpenRC+dinit.

**Package progress (binding)**: package mutations (`apk`, `flatpak`, `nix`) must expose the process output as terminal text, not a percentage bar. Use a daemon-side job log with `job_output` polling (RAM job registry; `/run/ursine/jobs/<id>.log`) or a future streaming protocol. The frontend shows live output and a **Close** button only after the job completes. Implemented 2026-10-27 (backend + GUI); keep any new mutation on this path.

**Static analysis (binding)**: canonical commands in `docs/guides/build-and-test.md`; policy and gates in `docs/STATIC-ANALYSIS.md`. All build/test/static work runs in containers (`ursine-ci:1`, `ursine-static:1`, `nixos/nix`). Never run cargo/nix/scanners on the host. `scripts/static-check.sh` is the single static entry point.

**Review closure (binding)**: fix every confirmed finding — blocker, high, medium and low — before commit. "Perfect code" means zero known defects at commit time, not zero detected defects. Never silently downgrade or defer a finding: a deferral requires explicit user approval and must be recorded in `state/session-state.json` with owner and rationale. A fix cycle revalidates the full gate; findings introduced by a fix count as new defects. Do not commit while any finding is open without that recorded approval.

**Parity rule (binding)**: every user-facing function ships backend first, then frontend immediately, in the same task. Do not start the next feature until daemon and frontend reach functional parity. Applies to the GUI (`ursine`) and web (`web/`). A daemon action with no frontend control is incomplete work, not a finished milestone. Deferral requires explicit user approval recorded in `state/session-state.json`.

**UI reference rule (binding)**: take interaction and layout inspiration from established admin tools: **COSMIC Settings** (`cosmic-settings`) for native COSMIC structure and controls, **Webmin** for dense module/action coverage, **Cockpit** for capability gating, status visibility, and confirmation of destructive actions. Copy patterns, not code. Ursine terminology and thin-frontend rules still win.

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

No host toolchain. Run everything in the dev image (see getting-started):

```bash
podman build -t ursine-ci:1 -f ci/Containerfile.dev .   # once
podman run --rm --network=host -v "$PWD:/work" -w /work \
  -e CARGO_HOME=/work/artifacts/cargo-home ursine-ci:1 cargo build --workspace
podman run --rm --network=host -v "$PWD:/work" -w /work \
  -e CARGO_HOME=/work/artifacts/cargo-home ursine-ci:1 cargo test --workspace --all-features
podman run --rm --network=host -v "$PWD:/work" -w /work \
  -e CARGO_HOME=/work/artifacts/cargo-home ursine-ci:1 sh -c \
  "cargo clippy --workspace --all-features -- -D warnings && cargo fmt --check"
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

## Milestones

- **2026-10-29 — theming fixes + logs viewer (0.22.0, tagged)**: VM-verified GTK3 chain in code (`gtk-theme-name=adw-gtk3-dark` when installed, `/* */` CSS marker — `#` made GTK discard the stylesheet, shell-palette import wrapper with legacy `theme_*` aliases, eog canvas gsettings target, galculator display target, GUI minimize toggle) + new `logs` module and Logs screen (dynamic `/var/log` scan, editable tail count default 200, load-previous chunks, tail -f polling, hardened paths). Gate: 276 tests + clippy/fmt + static + docs green. dinit boot fix and ursinectl plan ride along.
- **2026-10-28 — theming (0.21.0, tagged)**: `theming` module + GUI section — reads Wayle/DMS shell config (running shell of the session uid wins, else DMS; inactive without a shell) and propagates one theme to GTK3/4/2, qtct, KDE, Flatpak, COSMIC cosmic-config (systemic), environment.d, Firefox, VSCodium, terminals, cava, Electron; apply behind confirmation with per-target report; markers never clobber foreign files. Permanent three-button baseline: `gtk-decoration-layout=:minimize,maximize,close`, uniform window-control restyle, kdeglobals `[WM] ButtonsOnRight=IAX`. Plan: `ursine-titlebar` separate repo (niri-only, per-window layer-shell buttons, NOT scaffolded). Tier-2 review (4 lenses, ~45 findings incl. 3 blockers) closed; gate 262 tests + clippy/fmt + static PASS + docs PASS + live DMS probe PASS. Also: dinit boot fix (pushed with 0.20.0 ancestry), `ursinectl` planned in v3.
- **2026-10-27 — package progress (0.20.0, tagged)**: daemon job registry (`src/ursine-core/src/jobs.rs`; RAM, `/run/ursine/jobs/<id>.log`, 1 MiB cap, TTL/lost sweep, audit on start/complete outside the lock, poll not audited, principal-bound) + `Action::JobOutput` + protocol v3; APK/Flatpak/Nix mutations return `job_id` and stream to the job log (apk `--no-progress` removed); GUI terminal box (monospace tail, snap-to-bottom, Close only after completion/detach, single tracked job across panels). Tier-2 review (requirements/code/security/ux) closed, all confirmed findings fixed. Gate: 210 tests (core 135, GUI 37, API 32, integration 6), clippy/fmt, static all PASS, docs PASS. Commit `611f263`; tag `v0.20.0`; not pushed.
- **2026-10-23 — sidebar + APK/Flatpak (uncommitted)**: GNOME Settings-style sidebar shell; module-mode dropdowns; Niri/store preset dropdowns; APK backend (list/search/refresh/install/remove/upgrade) and Flatpak backend (system/user list/search/install/remove/update), both capability-gated; GUI panels with source/scope dropdowns and confirmations. Gate: 172 tests (GUI 35, API 28, integration 6, core 103), clippy/fmt, static all PASS, docs PASS. Not pushed.
- **2026-10-23 — users + niri baseline (0.7.0, released)**: Users panel via accountsservice (list/create/delete/password/groups, uid + self-wheel guards); Niri managed-section editor (layout gaps, animations, touchpad, cursor) with KDL round-trip, `niri validate`, atomic user-scoped write/backup, fail-closed passwd lookup; GUI panels for both. Gate: 161 tests (GUI 35, API 26, integration 6, core 94), clippy/fmt, static all PASS, docs PASS. Main CI 39981968110 green; tag CI 39986691808 green; Nix pin 86d90e2; local profile 0.7.0. Dynamic group tooling: usermod → addgroup/delgroup → gpasswd. Remaining niri form groups tracked in docs/TASKS.md.
- **2026-10-22 — snapshots (btrbk)**: raw btrbk parser (list/stats), snapshot creation, config read/init, restore preview/apply via `btrfs subvolume snapshot` (btrbk 0.32 has no restore; approved fallback). GUI panel with list, stats, create, config init, restore confirmation. 150 tests green; clippy/fmt clean; CI run 39861294723 green; released v0.5.0 and v0.6.0 (tag CI 39944669834, auto-pin b78c159); local profile 0.6.0. Real btrfs smoke pending root.
- **2026-10-22 — static analysis gate (v0.4.0, pushed)**: strict workspace lints (clippy all/pedantic/nursery/cargo + restrictions, `unsafe_code=forbid`, rustdoc `-D warnings`), repo linters, gitleaks, cargo-deny/audit/machete/geiger, SBOM, report-only duplicates; Alpine 3.24 images `ursine-ci`/`ursine-static`/`ursine-docs`; `scripts/dev.sh` local entry point; CI builds `ursine-ci` once and uses it from GHCR for build/test/lint/gui/package; docs and analyzer jobs use their own images. Gate: 143 tests, static all PASS, docs PASS, clippy/fmt/doc, nix linters, zizmor clean. Version 0.4.0 pushed; CI run 39833951848 green.
- **2026-10-21 — store parity hardening (uncommitted, review-closed)**: three review cycles closed every finding incl. low. Shared installable policy; XDG-aware prune as profile owner with exact preview binding; regex preview/apply by exact names; streaming output caps + process-group kill; absolute `su`/`nix`; PAM timeout + bounded semaphore; auth rate-limit ordering; per-uid socket cap; config read/write guards; shared timeouts; search truncation flag; bounded client connects; long-action tick. Gate: 143 tests (GUI 31, API 26, integration 6, core 80) + clippy `-D warnings` + fmt green (`artifacts/final-gate-2/gate.log`). No open review findings. Next: real daemon+GUI smoke, commit, then snapshots. Harness note: `steps:` removed from `.opencode/agents/codebase.md` only; subagent max-steps still triggers — cap elsewhere.
- **2026-10-21 — workflow + store parity**: binding backend-first/frontend-immediately parity rule and UI reference rule (cosmic-settings structure, Webmin action coverage, Cockpit gating/confirmations). GUI wired to every implemented daemon action: full `packages_nix` store mutations + `asUser`, `system.status`, `config.init`. 100 tests green; clippy `-D warnings` + fmt clean; uncommitted. Next: snapshots backend first, then GUI panel in the same feature.
- **2026-10-20 — packaging primitives**: cross-distro `install.sh` (path overrides, `DESTDIR`, auth gate, strict paths), `/etc/pam.d/ursine` (auth + account), systemd/OpenRC/dinit units (FeralOS wrapper/config for dinit); release tarball + CI staging smoke for all three inits. Alpine real PAM (including expired account) green; Rust gate green; review-hardened. FeralOS target login still pending.
- **2026-10-20 — v0.3 GUI**: login, capability-gated home, store view; 81 tests green; uncommitted.
- **2026-10-19 — v0.1/v0.2**: workspace, daemon, PAM handshake, packages_nix module, release v0.1.0 and CI.

## Deferred (stack decided — remaining)

GUI web asset toolchain decision (when web/ starts).
