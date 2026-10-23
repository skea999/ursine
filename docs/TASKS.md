# Tasks — Ursine

**Milestone**: v0.4 — users + niri baseline (0.7.0)
**Updated**: 2026-10-21
**Workflow rule (binding)**: backend first, frontend immediately, same feature. No next feature until daemon + frontend reach parity.
**UI rule (binding)**: patterns from cosmic-settings (structure), Webmin (action coverage), Cockpit (gating/status/confirmations).

## Current Sprint

### Done — settings + config modes (2026-10-21)

- [x] Separate Settings screen with gear navigation, capability refresh, and module switches (`auto` / `manual on` / `manual off`); daemon `config.get_config` and `config.set_module_switch`; TOML edits preserve unrelated content, write durably with hardened temp files, and apply live; `config.init` refuses overwrite | P0

### Done — v0.3 GUI store parity (2026-10-21)

- [x] Store UI covers every `packages_nix` daemon action: search + install, direct install, per-package upgrade/remove, upgrade all, regex upgrade/remove, generation rollback, closure diff, history wipe (preview + apply), generation prune (preview + apply), store GC, and `asUser` target user. Destructive actions confirm; timeouts match the daemon; busy/error/output states. Remaining backend actions wired: `system.status` (Home) and `config.init` (Settings). 100 tests green, clippy `-D warnings`, fmt clean | P0

### Done — snapshots (btrbk) (2026-10-22)

- [x] Backend: raw parser for `btrbk --format raw` list/stats, snapshot creation, config read/init/set, delete, restore preview/apply via `btrfs subvolume snapshot` with in-place backup (btrbk 0.32 has no restore) | P0
- [x] GUI: Snapshots panel with list, stats, create, config init + dropdowns, restore preview + in-place backup, single/bulk removal | P0
- [x] Tests: raw parser, config init/read, restore path validation, section validation; 150 total. CI run 39861294723 green; released v0.5.0 (tag CI 39871807747, auto-pin 2250abc) | P0

### Done — static analysis + security gate, v0.4.0 (2026-10-22)

- [x] Strict lints: pinned toolchain, workspace clippy all/pedantic/nursery/cargo + restrictions, `unsafe_code = forbid`, rustdoc warnings | P0
- [x] Repo/SAST/deps: typos, editorconfig, shellcheck/shfmt, taplo, actionlint, zizmor, gitleaks, cargo-deny/audit/machete/geiger, SBOM; duplicates report-only | P0
- [x] Alpine 3.24 images `ursine-ci`/`ursine-static`/`ursine-docs`; `scripts/dev.sh` + `scripts/static-check.sh` + `scripts/docs-check.sh`; CI builds `ursine-ci` once and uses GHCR; static/docs/zizmor jobs; actions/images pinned. CI run 39833951848 green (image/static/docs/test/lint/gui/package/publish) | P0
- [x] Layout: `src/tools/` shipped scripts, `src/web/` placeholder; version 0.4.0 | P1

### Done — v0.3 store parity hardening (2026-10-21)

- [x] Review closure (three cycles, every finding incl. low): socket caps + per-uid connection cap + busy reply; PAM `spawn_blocking` with 30 s timeout, bounded 8-permit semaphore and audit; auth rate limit charged before size checks; streaming command output caps (32 MiB complete search / 64 KiB normal) with process-group kill; absolute trusted `su`/`nix`; XDG-aware prune as the profile owner with exact `{current, would_delete}` binding; regex preview + apply by exact names; config read/write symlink/mode/owner/size guards; shared timeout budgets; search truncation flag; bounded client connect threads; stale search/scope fences; one-second long-action redraw | P0
- [x] Gate green: **143 tests** (GUI 31, API 26, integration 6, core 80), clippy `-D warnings`, fmt clean (`artifacts/final-gate-2/gate.log`). No open review findings | P0
- [ ] Real daemon + GUI smoke (display + root activation), then commit | P0 | agent

### Done — v0.4 users + niri baseline (0.7.0)

- [x] Users backend: accountsservice list/create/delete/set-password, `/etc/group` memberships, `usermod` group replacement, uid policy, self-wheel lockout guard, SHA-512 crypt hashing off the async worker | P0
- [x] Users GUI: create (admin toggle), select, password + confirmation, supplementary groups, delete confirmation with home cleanup | P0
- [x] Niri backend: KDL round-trip for layout gaps, animations, touchpad, cursor; `niri validate` gate; atomic user-scoped write + backup; fail-closed passwd lookup; validate action | P0
- [x] Niri GUI: managed-section forms, validation result, save | P0
- [x] Gate: 161 tests (GUI 35, API 26, integration 6, core 94), clippy/fmt, static all, docs | P0

### Done — sidebar + APK/Flatpak (2026-10-23)

- [x] GNOME Settings-style sidebar shell for logged-in screens; Settings module modes are dropdowns; Niri gap/cursor presets and store keep/age presets added | P0
- [x] APK backend: list/search/refresh/install/remove/upgrade with trusted `apk`, validation, capability gate | P0
- [x] Flatpak backend: list/search/install/remove/update for system/user scope, trusted `flatpak`, capability gate | P0
- [x] GUI APK/Flatpak panels with source and scope dropdowns, confirmations, search/install/remove | P0
- [x] Tests: APK parsing/validation, Flatpak parsing/scope, action tags/capability inventory; 172 total | P0

### Todo

- [ ] Niri v1 form groups: outputs, keybinds, gestures, keyboard/pointer, environment/spawn lists, screenshot path, hotkey overlay | P1 | agent
- [ ] v3 (deferred): choose web GUI stack — **must be memory-safe** (Rust→WASM preferred: Leptos/Yew/Dioxus; TypeScript acceptable fallback). See `docs/specs/SPEC-web.md` | P2 | skea999

### In Progress

- (empty)

### Done — packaging primitives (2026-10-20)

- [x] Cross-distro `install.sh`: absolute-path overrides + `DESTDIR` staging, PAM `pam_unix.so` detection with distro-specific install hints, conflict refusal + backup, automatic enable/start; **P1** | P1
- [x] Installer hardening: `ursined --version` auth gate (stub refused unless `--allow-stub`; cross-arch staging refused unless `--allow-unverified-auth`), `mktemp` temps, strict path allowlist + `.`/`..` rejection, PAM `0644` via temp + rename, `0750` config dir, staging ownership warning | P0
- [x] PAM service `packaging/pam.d/ursine` (auth + account, no session); release tarball installs it as root:root 0644 | P1
- [x] Init units: `packaging/systemd/ursined.service`, `packaging/openrc/ursined`, `packaging/dinit/{ursined,ursined.wrapper,ursined.conf}` (FeralOS wrapper/config convention, `/lib/dinit.d` example) | P1
- [x] Release tarball includes `install.sh` + `packaging/`; CI smoke-tests systemd/OpenRC/dinit staging, asserts PAM mode and `auth: pam` | P0
- [x] Staging tests for systemd/OpenRC/dinit, idempotent rerun, conflict + `--force` backup; shell syntax + shellcheck green | P1
- [x] Real PAM smoke in Alpine CI container: correct password → token, wrong password → `unauthorized`, expired account → `unauthorized`, token call works, audit contains no password material | P1
- [x] CI publish split from build: package job read-only, publish job write-only/tag/main, flake added to package needs, checksum verified, idempotent tag asset publish without overwrite | P0
- [x] CI release gate: package skipped when workspace version is already tagged; PR packaging smoke only on build-input changes; Cargo registry/target cache; tag releases auto-pin `nix/binary.nix` and publish `ursine-flake-<tag>.tar.gz` | P0
- [x] `v0.3.0` release: GitHub tag pipeline green; tarball + checksum + flake source archive published; `nix/binary.nix` auto-pinned in `bdbf6ce`; `verify-pin` green | P0
- [x] `v0.3.1` release: Settings/manual module modes; tag pipeline green; tarball + checksum + flake archive published; `nix/binary.nix` auto-pinned in `8dada79`; `verify-pin` green | P0
- [x] `README.md` documents Nix build/input usage, release pin behavior, local dev flake, and project overview | P1
- [x] Nix package chain `ursine` -> `ursined` -> `ursine-activate`: daemon owns versioned activation tools and packaging assets; detect systemd/OpenRC/dinit, install PAM symlink + unit, restart/deactivate; staged tests for all inits, idempotency, dry-run, and `nix run` apps | P0
- [x] systemd unit `[Install] WantedBy=multi-user.target` restored; audit dir enforced `0750` with parent/target checks; review findings triaged | P0

### Done — v0.3 GUI store view (2026-10-20)

- [x] `store.rs`: installed profile packages + generation history via `packages_nix.list_installed` / `packages_nix.generations`; Home ↔ Store navigation; Open disabled when the Nix capability is missing; busy/empty/error states; 75 s profile timeout; expired token returns to login; 81 tests green, clippy `-D warnings`, fmt clean | P0

### Done — v0.3 GUI login + home (2026-10-20)

- [x] GUI split into `app.rs` / `client.rs` / `login.rs` / `home.rs`; thin Unix-socket client (`auth` + `call`, 5 s timeout, `spawn_blocking`); login screen (username default from `USER`, password field, friendly auth errors); capability-gated home mirroring the daemon gate, with missing-capability reasons/fixes, refresh and sign out; session token RAM-only; 80 tests green, clippy `-D warnings`, fmt clean in dev container | P0

### Done — v0.3 libcosmic spike (2026-10-19)

- [x] libcosmic pinned rev builds on musl; minimal window + real socket round-trip (`capabilities.list`, unauthenticated → expected `unauthorized`); env `URSINE_SOCKET` override; build deps added to `ci/Containerfile.dev`; CI `gui` job (build + clippy) added; daemon jobs exclude the GUI for speed | P0

### Done — v0.2 packages.nix (2026-10-19)

- [x] New-CLI-only nix module: list/add/remove/removeAll/removeRegex/upgrade/upgradeRegex/search/history/rollback/wipe-history/diff-closures + **storeGc** (root, total) + **pruneGenerations** (root exec, keep last N, dry-run with current from symlink); `su -l -s /bin/sh` as target user; strict validation; uid 1000..60000; timeouts; 78 tests green. **Verified against real nix 2.31.5** (list JSON schema, history ANSI/no-current, --regex flags, prune lock semantics) | P0

### Done — v0.1 skeleton (2026-10-19)

- [x] Cargo workspace `src/{ursine-api,ursine-core,ursined,ursine}`; template `src/` subdirs removed | P0
- [x] `ursine-api`: envelope with Bearer token, ModuleId, full Action surface, Principal/AsUser, SessionToken (redacted Debug), Capability::ALL, TriState serde, UrsineConfig | P0
- [x] `ursine-core`: config (load/render/write atomic), capability Registry + `ensure_available`, auth trait + stub + PAM (feature), TokenStore (10 min sliding TTL), RateLimiter, JSONL audit, unix socket (std peer creds, 0666), dispatch (auth → wheel check → token → modules) | P0
- [x] `ursined`: arg parsing, tracing, graceful SIGTERM/SIGINT, stub/PAM authenticator switch | P0
- [x] Tests: **55 green** (21 api unit + 6 api integration via root `tests/ursine-api/` + 28 core unit) | P0
- [x] Clippy `-D warnings` + `cargo fmt --check` green, including `--all-features` (pam) | P0
- [x] E2E smoke in container: auth → 64-char token → system.status → capabilities.list (6) → bad token rejected → audit JSONL (3 lines) → clean shutdown; binary dynamic musl (`ldd`) | P0
- [x] `flake.nix` (pkgsMusl, dynamic musl, x86_64, feature pam) + `flake.lock`; `nix flake show` evaluates green (full build runs in CI) | P0
- [x] `.github/workflows/ci.yml` ACTIVE: matrix default/pam in alpine:3.22, clippy+fmt gate, nixos/nix flake check/build, release-on-tag | P0
- [x] `ci/Containerfile.dev` dev image (Alpine 3.24 + rustup 1.99.1 + clippy/rustfmt + GUI deps); all local builds containerized (podman, `--network=host`) | P0
- [x] Dynamic musl decision: `.cargo/config.toml` `-crt-static` off (static musl cannot link libpam.so; Alpine-style) | P0

### Done — planning (2026-10-16)

- [x] Naming, architecture, intent v1-v6, capability map, 9 module specs
- [x] Harness aligned (agents hidden, models removed, workflow Pattern 8, host isolation rules)

## Backlog

- packages.nix (v0.2) → apk + flatpak (v2)
- snapshots: btrbk read-write config (subvolumes + scheduler), restore flow
- web + OIDC (v3), Incus UI (v4), ZFS+Pirateus (v5), multi-distro (v6)
- APKBUILDs in feralos-aports (v1.0)

## Known Issues

| Issue | Status | Workaround |
|-------|--------|------------|
| Nix flake full build never run locally (eval only) | open | first CI run validates; CI flake job is the gate |
| PAM real auth path not yet tested on a FeralOS target | open | Alpine container PAM smoke green; run the PAM-feature build on FeralOS with a real user before v1.0 |
| Full init activation not tested | open | `systemd-analyze verify` green, `dinitcheck` green; OpenRC start blocked by container read-only cgroups. Run on a disposable VM/real target. |
| Tag release publish/auto-pin/verify-pin | resolved | `v0.3.0` run `39452993223` green; release assets + flake archive published; `nix/binary.nix` pinned in `bdbf6ce`; `verify-pin` green |
| `.opencode/package.json` + lock tracked while `.opencode/.gitignore` exists | resolved | ignore lines dropped; files tracked intentionally |
| Agent fixes (hidden/quote/no-model) diverge from skea-store template | open | promote upstream when skea999 decides |

---

*Update start/end of each session.*
