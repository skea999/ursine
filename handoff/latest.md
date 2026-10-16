# Handoff

**Active state**: `state/session-state.json`
**Milestone log**: `AGENTS.md`

See `state/session-state.json` for current goals, decisions, risks, and next actions.
See `AGENTS.md` for project context and milestone history.

## Session 2026-10-16 — INTENT CONFIRMED (NO CODE YET)

**Confirmed Statement of Intent**: `docs/intent/ursine.md` — explicit yes from skea999. Version plan:

- **v1**: backend skeleton + GUI GTK4/libadwaita (mandatory): nix packages, users, snapshots, niri editor
- **v2**: apk + flatpak; printers, scanners, bluetooth, network, firewall, disks (all D-Bus/CLI, capability-gated) — no OIDC
- **v3**: OIDC + web (web = LAST frontend)
- **v4**: Incus UI
- **v5**: ZFS + Pirateus
- **v6**: other distros (pkg/init abstraction, glibc revisited)

**UI principles (binding)**: all UIs look as similar as possible (one design language, same layout/navigation/icons/terminology per CONTEXT.md); user-friendly (libadwaita HIG, capability-gated panels never broken-looking, confirm destructive actions, plain language).

Phasing ≠ exclusion: everything is in scope; only AppImage and glibc (pre-v6) are out.

Also decided this session: agents wired (`opencode.json` instructions + `.opencode/rules/ursine.md` invariants); build policy documented (musl everywhere incl. nix via pkgsMusl/pkgsStatic; local builds Docker+Alpine `rust:alpine`; CI test matrix per cargo feature + flake build/publish + Release artifacts on tag).

### Environment notes (dev machine)

- rustc/cargo NOT installed — use Docker (`rust:alpine`) or install rustup before first build.
- libpam headers NOT present → PAM sits behind cargo feature `pam`; dev builds compile stub auth; FeralOS package builds with `--features pam`.
- Template leftovers: `src/` dirs, `skills-lock.json` (stale), see `docs/repo-setup.md`.

### Next steps (in order)

1. Init cargo workspace: `src/ursine-api`, `src/ursine-core`, `src/ursined`, `src/ursine` (GUI placeholder), `web/` placeholder; remove template `src/` subdirs (components/services/types/utils).
2. `ursine-api`: serde types — Request/Response envelope (newline-delimited JSON), ModuleId, Capability, TriState config, Error, Principal, `asUser`.
3. `ursined` skeleton: config load, capabilities scan, unix socket accept loop, peer creds, PAM handshake behind feature (stub in dev), JSONL audit, dispatch skeleton returning NotImplemented.
4. `flake.nix` musl static + `.github/workflows/ci.yml` (test matrix per feature, flake check/build, Release on tag).
5. Tests: serde roundtrip, tri-state config, capabilities scan.
6. Downstream per interview-me: spec (spec-driven-development) or task planning (@planner) — user's choice.

### Spec phase status (2026-10-16, same session)

- Capability map APPROVED with corrections: `docs/capability-map.md`.
- 9 module specs written in `docs/specs/` (protocol, daemon-core, capabilities, packages-nix, snapshots, users, niri-editor, gui, packaging-infra) — corrections applied:
  - Layout binding: code `src/` (members src/ursine-api, src/ursine-core, src/ursined, src/ursine), tests `tests/`, generated `artifacts/` (gitignored, .gitkeep created). **Build output in `artifacts/target`** — `.cargo/config.toml` sets target-dir; no `target/` at root. Harness re-aligned from skea-store (all hidden dirs present); `.github/workflows/ci.yml` created fully COMMENTED (activate with skeleton); ALL CI jobs in Docker (alpine:3.22 cargo, nixos/nix flake/release).
  - Snapshots: btrbk integration with READ-WRITE config — change subvolumes + scheduler/retention from GUI, `btrbk dryrun` before every write, `initConfig` creates btrbk.conf (hourly 24 / daily 14).
  - Niri: schema-driven forms ONLY (no raw KDL typing).
  - Socket root:root 0666; token TTL 10 min; x86_64 only; no oidc feature in v1 (trait seam only).
  - Open answers: nix search slow accepted (v1) + progress; users avatars/autologin v2; kdl comments lossy+warning (a); app-id org.feralos.Ursine; system theme.
- AWAITING: spec approval → then plan (Phase 2) + tasks (Phase 3) — only on user's explicit go. DO NOT START DEVELOPMENT until skea999 says so.

### Harness re-alignment #2 (2026-10-16, after skea999's template update)

- NEW agents copied: `compliance-auditor`, `legacy-modernizer`, `smart-contract-auditor` (+ manifest/lock files) — 23 agents total in `.opencode/agents/`
- UPDATED: `researcher.md` (rewritten: Context7 cross-ref, mode all), `rules/workflow.md` (security lens, decision-grounding, adr, blast-radius steps)
- `opencode.json` merged: template's new `permission.bash` patterns + template `agent` block (orchestrator can now dispatch the 3 new agents); our `instructions` block kept; model overrides REMOVED (subagents use default model)
- `docs/agents-guide.md` + `docs/global-setup.md` replaced with updated template versions
- Agents fixed: `hidden: true` added to 15 subagents (16/23 hidden — subagents no longer selectable in Tab); `code-reviewer.md` description quoted (was invalid YAML); all 23 frontmatters validated
- Workflow aligned in AGENTS.md#Workflow: Pattern 8 review gate (Tier 1 cargo gate → Tier 2 lens briefs), skill-chain with decision-grounding/code-change-impact/adr; 18/18 referenced skills verified present
- Protected and NOT overwritten (ours): AGENTS.md, CONTEXT.md, ROADMAP.md, CHANGELOG.md, package.json, .gitignore, .cargo/, ci.yml, pre-commit, rules/ursine.md, our 4 skills, docs/{architecture,specs,intent,PROJECT,TASKS,capability-map,getting-started,update,repo-setup}, handoff, state
- Template .agents/skills unchanged (43 = already latest from today's `npx skills update`)
- OPEN: the same agent fixes (hidden/quote/no-model) are NOT yet in the harness template `/home/gab/projects/skea-store` — 15 files differ; skea999 decides when to promote upstream
- STATUS: **READY TO WRITE CODE** — pending spec approval + explicit go signal

### Post-spec corrections (2026-10-16, same session)

- HOST ISOLATION absolute rule added (AGENTS.md + .opencode/rules/ursine.md): never modify host — everything in Docker containers, volumes under artifacts/, nix GC via container never with -d. clean-artifacts.sh rewritten (keeps last 3 nix out-links + container gc; first run freed 443 MB).
- Build output: `.cargo/config.toml` target-dir = `artifacts/target`; nix builds `--out-link artifacts/result` (nothing at repo root).
- CI: `.github/workflows/ci.yml` written fully COMMENTED (double-hash header survives uncomment; activate with v0.1 skeleton). All jobs in Docker: alpine:3.22 (cargo matrix) + nixos/nix (flake/release).
- GUI stack CHANGED to relm4 (option B): SPEC-gui Tech Stack/Structure/Code Style rewritten; capability map updated.
- Skills installed via `npx skills add` (now in skills-lock.json): rust-best-practices (apollographql), rust-async-patterns (wshobson), z-rust-gtk4 (victorzhuk, relm4+libadwaita), gtk-ui-ux-engineer (gotar). Ran `npx skills update` (9 updated) + `npx agents-opencode --update` (harness backup in .opencode/.backups/).

*This file exists for agent compatibility. All state is in session-state.json.*
