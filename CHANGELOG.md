# Changelog

## [0.3.1] — 2026-10-21

### Added
- Settings screen: capabilities refresh plus manual module modes (`auto`, `manual on`, `manual off`) backed by new `config.get_config` and `config.set_module_switch` daemon actions. Module switches persist atomically and durably, preserve unrelated TOML, and apply live. `config.init` no longer overwrites an existing config.

## [0.3.0] — 2026-10-20

### Added
- `README.md`: Nix flake install/input instructions and project overview.
- Nix package chain: `ursine` depends on `ursined`; the daemon package owns `ursine-activate`/`ursine-deactivate` (`bin/ursine-activate`, `bin/ursine-deactivate`) and the packaging assets. Activation detects systemd/OpenRC/dinit, installs PAM as a profile symlink, renders the init unit against the stable profile, and restarts the daemon. Deactivation stops/disables it and removes PAM/init files. Both are idempotent.
- CI release gate reads the workspace version; the expensive `package` job runs only for a tag or a new version, while PR packaging smoke runs only when build inputs change. Cargo registry and `artifacts/target` use GitHub Actions cache.
- Tag releases update `nix/binary.nix` on `main` and attach `ursine-flake-<tag>.tar.gz`; `nix flake update` follows the release pin.
- Cross-distro release installer (`install.sh`): absolute-path overrides plus `DESTDIR` staging, automatic `/etc/pam.d/ursine` install with `pam_unix.so` detection and distro-specific hints, conflict refusal with `--force` backup, and automatic enable/start. Init units for **systemd, OpenRC and dinit** (FeralOS wrapper/config convention); `--init` forces the target when packaging.
- Release tarballs now include `install.sh` and `packaging/`; CI stages systemd, OpenRC and dinit installs, asserts PAM `0644`, and runs the staged daemon to require `auth: pam`.
- Installer safety: refuses stub-auth daemons unless `--allow-stub`, requires `--allow-unverified-auth` for cross-architecture staging, uses `mktemp` temp files, restricts paths to `[A-Za-z0-9/._+-]` and rejects `.`/`..` components, sets PAM mode on the temp file before rename, and warns on non-root staged ownership.
- PAM service asset `packaging/pam.d/ursine`: auth + account only (`pam_unix.so`), no session, installed root:root 0644.
- v0.3 GUI store view: installed nix profile packages and generation history (`packages_nix.list_installed` / `packages_nix.generations`), Home ↔ Store navigation, capability-gated Open action, busy/empty/error states, 75 s profile-call timeout; expired sessions return to login.
- v0.3 GUI driven by the protocol: login screen (PAM handshake, friendly auth errors) and capability-gated home (module rows with missing-capability reasons/fixes, refresh, sign out). Client is a thin Unix-socket transport with 5 s timeout; session token stays in RAM. 80 tests green; clippy/fmt clean.
- v0.3 libcosmic spike: pinned rev builds on musl; minimal window + real socket round-trip (`capabilities.list`); `URSINE_SOCKET` override; dev image and CI `gui` job updated with libcosmic build deps.

### Changed
- **GUI toolkit: GTK4/relm4 → libcosmic** (COSMIC toolkit, iced-based Rust UI), pinned rev `430f3d48a97d8779f9c1985473912e594ccca1ae`. Reason: security — remove GTK/gdk-pixbuf/librsvg C parsers from the GUI process; visual coherence with the GTK Wayle shell is best-effort (distro already mixes COSMIC apps on niri). UX principles remain toolkit-independent.
- Web frontend (v3): stack still undecided, but **memory-safe is now a binding constraint** (Rust→WASM preferred; TypeScript acceptable fallback) — `docs/specs/SPEC-web.md`.
- CI release publishing split from the build job: build keeps `contents: read`; a separate publish job holds `contents: write` for tags/main, requires the flake job, verifies checksums, and publishes only missing tag assets without overwriting existing ones.
- Audit directory is enforced `0750` (rejects root, symlink and world-writable parent); audit file remains `0600` and rejects symlink/non-regular targets.
- Docs aligned: SPEC-gui, SPEC-users, PROJECT, intent (dated amendment), architecture overview, getting-started, ROADMAP, TASKS, handoff.

## [0.1.0] — 2026-10-19

### Added
- v0.2 packages.nix: new-CLI-only nix profiles (`nix profile`), as-user execution
  with strict validation, 9 operations (list/add/remove/upgrade/search/history/
  rollback/wipe-history/diff-closures), total store GC and keep-last-N
  generation pruning (dry-run plan, root exec, current from symlink), JSON
  parsers, timeouts. remove/removeAll/removeRegex, upgradeRegex. 78 tests.
  Behavior verified against real nix 2.31.5.
- v0.1 skeleton (green): Rust workspace `src/{ursine-api,ursine-core,ursined,ursine}`; protocol types (JSON-lines + Bearer session token), PAM handshake (feature `pam`, dev stub), RAM token store with 10-min sliding TTL, per-uid rate limiting, JSONL audit, unix socket server (0666, peer creds), module dispatch with capability gating; 55 tests; e2e smoke verified.
- `flake.nix` (pkgsMusl dynamic musl, x86_64, `pam` feature) + lock; eval validated in nixos/nix container.
- `.github/workflows/ci.yml` ACTIVE: containerized test matrix (default/pam), clippy/fmt gate, nixos/nix flake check/build, release-on-tag.
- `ci/Containerfile.dev` dev image; dynamic musl build config (`-crt-static` off — static musl cannot link libpam).
- CI gates: cargo-vet (supply chain), cargo-audit (RustSec), Semgrep SAST.
- Nix flake packages the pinned musl release artifact (`nix/binary.nix`): nixpkgs does not cache musl toolchains, and static musl cannot `dlopen` PAM modules — Alpine builds, Nix publishes reproducibly. Release v0.1.0 published.
- PAM via `pam-client 0.2` (replaces `pam 0.7`): fixes RUSTSEC-2025-0040 (`users 0.8` vulnerable/unmaintained) and avoids bindgen.

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
