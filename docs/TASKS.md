# Tasks — Ursine

**Milestone**: v0.1 — Skeleton
**Updated**: 2026-10-16

## Current Sprint

### Todo
- [ ] AWAITING SPEC APPROVAL — no development until skea999 says so | P0 | skea999
- [ ] Init cargo workspace: `src/ursine-api`, `src/ursine-core`, `src/ursined`, `src/ursine`, `web/`; remove template `src/` subdirs | P0 | agent
- [ ] ursine-api: serde types (JSON-lines envelope, ModuleId, Capability, TriState, Error, Principal, asUser) | P0 | agent
- [ ] ursined skeleton: config load, capabilities scan, unix socket + peer creds, PAM handshake (feature `pam`, stub on dev), JSONL audit, dispatch NotImplemented | P0 | agent
- [x] CI `.github/workflows/ci.yml` written — **fully commented out until v0.1 skeleton** (test matrix per cargo feature in alpine container, clippy+fmt gate, `nixos/nix` container: flake check/build + Release publish on tag) | P0 | agent — 2026-10-16
- [ ] Activate ci.yml (uncomment) together with workspace + flake.nix | P0 | agent
- [ ] flake.nix: build ursined + ursine statically on musl (pkgsMusl/pkgsStatic), x86_64 only | P0 | agent
- [ ] Tests: serde roundtrip, tri-state config defaults, capabilities scan units | P1 | agent
- [ ] Clean template leftovers: template `src/` subdirs, `skills-lock.json` decision | P2 | agent

### In Progress
- (empty)

### Done
- [x] Naming locked: ursined / ursine / ursine-api / ursine-core / web | P0 | skea999 — 2026-10-16
- [x] Architecture agreed (auth no-polkit/PAM/token, stateless no-DB, capabilities, asUser) | P0 | skea999 — 2026-10-16, see docs/architecture/overview.md
- [x] PROJECT.md, CONTEXT.md, ROADMAP.md, handoff, session-state compiled | P0 | agent — 2026-10-16
- [x] Harness fully aligned: hidden subagents (16/23), no model overrides, valid frontmatters, orchestrator can dispatch all agents, podman runtime, host isolation + temp-file + retention policies, .opencode-agents-version gitignored | P0 | agent — 2026-10-16
- [x] Workflow updated to the new chains (Pattern 8 review gate with lens briefs, skill-chain with decision-grounding/code-change-impact/adr; 18/18 referenced skills verified present) | P0 | agent — 2026-10-16

## Backlog
- packages.nix module (v0.2 flagship) | P0
- packages.apk, packages.flatpak | P0/P1
- snapshots module (manual + pre-update auto) | P1
- users module (accountsservice D-Bus) | P1
- niri config.kdl editor + `niri validate` | P2
- web static frontend + PAM login | P2
- OIDC token issuance path | P2
- APKBUILDs in feralos-aports | P2 (v1.0)

## Known Issues
| Issue | Status | Workaround |
|-------|--------|------------|
| Dev machine: no rustc/cargo | open | Docker `rust:alpine` for builds (see getting-started) |
| Dev machine: no libpam headers | open | `pam` behind cargo feature; stub auth in dev builds |
| `.agents/`, `.opencode/`, `.githooks/` were missing from initial copy | resolved | harness re-aligned from `/home/gab/projects/skea-store` (2026-10-16) |

---

*Update start/end of each session.*
