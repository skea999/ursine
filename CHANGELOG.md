# Changelog

## [0.1.0] — 2026-10-19

### Added
- v0.1 skeleton (green): Rust workspace `src/{ursine-api,ursine-core,ursined,ursine}`; protocol types (JSON-lines + Bearer session token), PAM handshake (feature `pam`, dev stub), RAM token store with 10-min sliding TTL, per-uid rate limiting, JSONL audit, unix socket server (0666, peer creds), module dispatch with capability gating; 55 tests; e2e smoke verified.
- `flake.nix` (pkgsMusl dynamic musl, x86_64, `pam` feature) + lock; eval validated in nixos/nix container.
- `.github/workflows/ci.yml` ACTIVE: containerized test matrix (default/pam), clippy/fmt gate, nixos/nix flake check/build, release-on-tag.
- `ci/Containerfile.dev` dev image; dynamic musl build config (`-crt-static` off — static musl cannot link libpam).

All notable changes to Ursine. Format: Keep a Changelog, semver.

## [Unreleased] — 2026-10-16

### Added
- Workflow updated to the new harness chains: Pattern 8 Pre-Commit Review Gate (Tier 1 cargo build/test/clippy/fmt → Tier 2 `@review` lens briefs requirements|code|security|ux-responsive, ≤2 cycles) and skill-chain with decision-grounding, code-change-impact, adr steps — 18/18 referenced skills verified present.
- Agent hygiene: 16/23 agents hidden from Tab (subagents dispatch-only), model overrides removed (default model everywhere), all frontmatters valid YAML, orchestrator can dispatch the 3 new agents (compliance-auditor, legacy-modernizer, smart-contract-auditor).
- Harness re-aligned after template update: new agents (compliance-auditor, legacy-modernizer, smart-contract-auditor — 23 total), updated researcher agent + workflow rules (security lens, decision-grounding, adr), refreshed agents-guide + global-setup docs. opencode.json merged (template bash permissions + our instructions).
- `docs/intent/ursine.md` — **confirmed Statement of Intent** (interview-me protocol, explicit yes): version plan v1-v6, UI principles (UIs must look alike, user-friendly), phasing ≠ exclusion.
- ROADMAP rewritten to the confirmed v1-v6 plan; PROJECT.md vision/scope updated.
- Agents wired for the project: `opencode.json` `instructions` load project docs (CONTEXT, architecture, PROJECT, handoff, TASKS) into every agent; `.opencode/rules/ursine.md` binding invariants (opencode-rules plugin).
- Build & CI policy documented (`docs/architecture/overview.md`): musl everywhere (nix included, pkgsMusl/pkgsStatic), local builds via Docker + Alpine images (`rust:alpine`), GitHub CI test matrix per cargo feature, `nix flake check`/`nix build` per push, Release artifacts on tag.
- Project planning complete: architecture, naming, roadmap.
  - `docs/architecture/overview.md` — all decisions + rationale (auth: no polkit, PAM handshake + RAM session tokens; stateless no-DB; runtime capabilities + tri-state config; asUser for per-user ops; two feature axes).
  - `docs/PROJECT.md` — vision, users, scope (build vs already-packaged), risks.
  - `CONTEXT.md` — Rust stack + domain vocabulary.
  - `ROADMAP.md` — milestones v0.1 skeleton → v1.0.
  - `handoff/latest.md` + `state/session-state.json` — session memory.
  - File-based session protocol in `AGENTS.md` (template's PowerShell session-memory skill not available on this machine).
- `.githooks/pre-commit` — secrets scan + `cargo fmt --check`.

### Decided
- Name: Ursine. Binaries `ursined` (root daemon, dinit), `ursine` (GUI GTK4/libadwaita). Crates `ursine-api`, `ursine-core`. Web assets `web/`. Packaging in `skea999/feralos-aports`.
- Stack: Rust everywhere (single workspace). GUI must be gtk4-rs anyway; shared serde types make a second backend language a liability.
- Modules v1: packages (nix first — no GUI exists on any distro), snapshots, users, niri config editor. NOT building printers/scanners/bluetooth/network/firewall/disks (already packaged on Alpine).
