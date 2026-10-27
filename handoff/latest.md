# Handoff

**Active state**: `state/session-state.json`
**Milestone log**: `AGENTS.md`

See `state/session-state.json` for current goals, decisions, risks, and next actions.
See `AGENTS.md` for project context and milestone history.

## Session 2026-10-26 — Niri v1 outputs (in progress)

- Added Niri output `mode` and `position` (x/y) through API, KDL read/write, and GUI.
- Added `niri --version` to the config view and GUI; compatibility gate and field-level validation errors still pending.
- Plan traceability rule added to AGENTS/workflow: no scope outside intent/inventory/SPEC; scanner config-only; autologin excluded.
- Added APK per-package upgrade/update list, Flatpak per-app update, Bluetooth battery, disk UUID/usage/SMART, and CUPS printer options (media/Duplex/ColorModel).
- Next: Niri animations/curves; APK repos/size/progress; Flatpak remotes/runtimes/permissions; network/firewall; disks SMART/format; printer job progress.

## Session 2026-10-26 — dinit services, 0.19.0

**Status**: 0.19.0 review/validation pending; tag in this release.

- Services backend supports OpenRC and dinit. dinit wins when running; gate accepts `openrc` OR `dinit`.
- dinit: `dinitctl list/start/stop/restart/enable/disable`, `boot.d` enabled state, internal `boot`/`recovery` filtered, idempotent enable/disable.
- OpenRC capability now requires the runtime (`/run/openrc` or `openrc-init`), not just `rc-service` tooling.
- GUI heading follows active backend; Home/Services availability accepts either init.
- Tests: 123 core + 36 GUI; clippy `-D warnings`; fmt. Tag pending.

## Session 2026-10-26 — init hardening, 0.18.4

**Status**: 0.18.4 review-closed; release pending push/tag.

- Fixed FeralOS init detection: PID 1 (and `/proc/1/exe`) wins over installed OpenRC/dinit tooling; OpenRC+dinit hosts select dinit.
- Activation now copies `ursined` to `/usr/local/sbin/ursined` and PAM policy to `/etc/pam.d/ursine`; the user-writable Nix profile is never the live root path. Paths are validated, temp files use `mktemp`, and dinit dirs are restricted to `/lib/dinit.d` or `/etc/dinit.d`.
- dinit unit has no distribution-specific targets; activation/deactivation handle stale units and `boot.d` links, preserve an existing `ursined.conf`, and tolerate already-enabled services.
- install.sh defaults dinit to `/lib/dinit.d`, rejects empty/`/` destdir, restarts systemd/OpenRC, reloads dinit, and validates existing file ownership/mode.
- CI smoke now covers automatic PID-1 detection, stale-unit cleanup, root-owned copies, second-run idempotency, boot-link removal, `--root /` rejection, and dinit install defaults.
- Review: two cycles; previous critical/high/medium findings fixed. Validation: shellcheck, actionlint, staged activation/deactivation smoke, dinit-check.
- Next: Services dinit backend+GUI (P0), real FeralOS PAM/dinit smoke, v2 polish.

## Session 2026-10-23 — users + niri baseline, 0.7.0

**Status**: 0.7.0 released. Main CI 39981968110 green; tag CI 39986691808 green; Nix pin 86d90e2; local profile `ursined 0.7.0 (auth: pam)`.

- Users: accountsservice list/create/delete/set-password, `/etc/group` memberships, runtime-selected `usermod`/`addgroup`/`gpasswd` group replacement, SHA-512 crypt off the async worker; uid 1000..60000 and self-wheel removal guards. GUI: create (admin), password confirmation, groups, delete confirmation with home cleanup.
- Niri: KDL round-trip for layout gaps, animations, touchpad, cursor; unknown nodes/comments preserved; `niri validate` before write; atomic user-scoped temp+rename with backup (`.ursine-backup`); all niri I/O and validation run as the target user through fixed `su` scripts (root never opens the path); passwd lookup fails closed. GUI form + validate/save.
- Spec scope note: these are the first niri managed-section groups; outputs, keybinds, gestures, keyboard/pointer, environment/spawn lists, screenshot path and hotkey overlay remain (docs/TASKS.md).
- Security review: first pass found root-side niri TOCTOU/write issues; refactored all niri read/write/validate to run as the target user through fixed `su` scripts. Second pass: no security findings.
- Gate: 161 tests (GUI 35, API 26, integration 6, core 94); clippy `-D warnings`; fmt; static all PASS; docs PASS; version 0.7.0.
- Released v0.7.0; `nix/binary.nix` auto-pinned in 86d90e2.

**Next**:
1. Root activation/restart remains manual: `sudo "$HOME/.nix-profile/bin/ursine-activate"` then restart `ursined`.
2. Real accountsservice/niri smoke needs a running daemon as root.
3. CI optimization pushed in 88f3bdc: package starts in parallel, single workspace clippy, GUI build removed, Nix PR-only, caches restore-only off main, verify-pin after publish.

## Session 2026-10-23 — CI optimization

- Lint is now one `cargo clippy --workspace --all-targets --all-features`; GUI job runs only `cargo test -p ursine --all-targets` (no separate build, no duplicate clippy).
- `package` starts in parallel with test/lint/gui/analyzer/docs; `publish` waits for all quality gates.
- Nix flake moved off the tag/main critical path: PR-only and only when Nix inputs change; tag `verify-pin` runs `nix flake check` + `nix build` after publish.
- Cargo target/registry caches: restore-only on PR/tag, saved only on `main`; release package path uncached. Zizmor cache-poisoning clean.
- Optimized main CI 39988854754 green in ~11m with package skipped (v0.7.0 already tagged).
- Details: `docs/guides/build-and-test.md` §8.

## Session 2026-10-22 — snapshots (btrbk), uncommitted

**Status**: 0.6.0 released. Main CI 39941124119 green; tag CI 39944669834 green (package/publish/verify-pin); Nix pin b78c159; local profile upgraded to ursined 0.6.0 (auth: pam).

- Backend: raw parser for `btrbk --format raw` list/stats, `btrbk snapshot`, btrbk config read/init, restore preview/apply via `btrfs subvolume snapshot` (btrbk 0.32 has no restore; approved fallback). Capability gate Btrbk+Btrfs; audit through dispatch.
- GUI: Snapshots panel from Home, list, stats, create, dropdown config editor, restore source dropdown + in-place target default/backup, single/bulk removal with confirmation. Thin frontend; all decisions in daemon.
- Tests: 150 total (GUI 31, API 26, integration 6, core 87), clippy/fmt, static all, docs, nix linters green.
- Deferred explicitly in spec: `snapshots.setConfig` and pre-update hooks.

**Next**:
1. Root activation/restart on FeralOS: blocked by sudo policy; run `sudo "$HOME/.nix-profile/bin/ursine-activate"` manually.
2. Real btrfs snapshot/restore/config/delete smoke requires root (btrfs subvolume list is EPERM as user).
3. Next feature: users (accountsservice) or niri editor.

## Session 2026-10-22 — static analysis gate + v0.4.0 (uncommitted, gate-green)

**Status**: strict static/security/dependency gate implemented, version 0.4.0 pushed. GitHub CI run 39833951848 green: image, flake, docs, zizmor, analyzer, gui, test, lint, package, publish. App is not containerized; only tooling images are used.

- Rust lints: pinned `rust-toolchain.toml` (1.99.1), `rustfmt.toml`, `clippy.toml`, workspace `[lints]` with clippy all/pedantic/nursery/cargo + restriction set (`unwrap_used`, `expect_used`, `panic`, `indexing_slicing`, `string_slice`, ...), `unsafe_code = forbid`. Fixed all production findings.
- Repo lint: `typos`, `editorconfig-checker`, `shellcheck`, `shfmt`, `taplo`, `actionlint`, `zizmor`, `statix`/`deadnix`/`alejandra`; Markdown via `ursine-docs:1` + `scripts/docs-check.sh`.
- SAST: gitleaks history scan; cargo-geiger reports; code security via strict clippy (unsafe forbid, disallowed Command/temp_dir, panic/unwrap restrictions).
- Dependencies: `cargo-deny` (advisories, licenses, bans, sources; wildcard deny; pinned git allowlist), `cargo-audit` with warn <10 / block >=10 threshold (`scripts/audit-threshold.sh`), `cargo-machete`, per-crate CycloneDX SBOM. Duplicates: `cargo tree -d`, report-only.
- Tooling: three Alpine 3.24 images (`ursine-ci:1` build/test, `ursine-static:1` analyzers, `ursine-docs:1` Markdown); `scripts/dev.sh` is the single local entry point; CI builds `ursine-ci` once and pushes it to GHCR for all build/test jobs. Canonical commands: `docs/guides/build-and-test.md`; policy: `docs/STATIC-ANALYSIS.md`; exceptions: `docs/static-analysis-exceptions.md`.
- Layout: shipped activation/deactivation tools moved to `src/tools/`; web placeholder at `src/web/`; dev/CI tooling stays in `scripts/`.
- CI: GUI tests run; build/test/lint/gui/package use the GHCR `ursine-ci:main` image; `analyzer` and `docs` jobs build their own images and run the same scripts; zizmor job; Nix linters; actions/images pinned; release cache removed.
- Gate: `scripts/dev.sh check` PASS; `scripts/dev.sh static all` PASS; `scripts/dev.sh docs` PASS; cargo test 143 (GUI 31, API 26, integration 6, core 80) all features; PAM/default matrix; clippy `-D warnings`; fmt; rustdoc; nix linters; zizmor clean.
- Version: workspace 0.4.0 (minor bump). `nix/binary.nix` stays pinned to 0.3.1 until the next tag release.

**Next**:
1. Tag/release later per policy (updates `nix/binary.nix`).
2. Snapshots feature: btrbk backend first, GUI panel in the same feature.

## Session 2026-10-21 — v0.3 store parity hardening (uncommitted, all findings closed)

**Status**: every confirmed review finding — blocker, high, medium and low — is fixed. Gate green; no commit yet. Real daemon+GUI smoke still pending.

- Review closure: trust predicate now uses the shared `ursine-api` installable policy; explicit target-user installs and all upgrades prompt because that user's flake registry can redirect `nixpkgs`.
- Prune: resolves the actual `nix profile` path in the user's login env (XDG/NIX_STATE_HOME aware), runs `nix-env` as the profile owner (no root write into user state), rejects unknown active generation, and applies only an exact previewed `{current, would_delete}` plan. GUI requires a matching preview before apply.
- Regex actions: daemon computes matches once and applies them by exact package name, so preview and mutation use the same engine; dry-run lists matches. Quoted shell values allow regex syntax without weakening injection checks.
- Exec: streaming stdout/stderr with hard caps (search 32 MiB complete, other output 64 KiB), process-group kill on timeout/overflow, absolute trusted `su`/`nix` paths for root helpers.
- Auth/socket: rate limit charged before size checks, no fsynced audit per limited attempt, 30 s PAM timeout with a bounded 8-permit semaphore (timed-out tasks keep the permit), per-uid 8-connection cap, protocol busy reply on rejection.
- Config: read and write reject symlinks, group/world-writable files, non-root owners, oversized input; atomic writes keep `0640`.
- Client: deadlines derive from shared `ursine-api` timeouts + 30 s write + 10 s margin; bounded connect helper threads; legacy search/config payloads stay compatible; one-second redraw tick while a long action runs.
- Search caps now return `truncated` and the GUI says "first 200 of more"; unknown payloads no longer dump raw JSON.
- Gate in `ursine-ci:1`: build + test + clippy `-D warnings` + fmt → **143 tests** (GUI 31, API 26, integration 6, core 80); log `artifacts/final-gate-2/gate.log`.
- `steps:` is gone from `.opencode/agents/codebase.md` only; subagent max-steps still triggers (cap elsewhere). No open review findings.

**Next**:
1. Real daemon + GUI smoke (display + root activation), then commit with the conventional message.
2. Snapshots: btrbk daemon module first, GUI panel in the same feature (parity rule).
3. Optional distro policy: `pam_faillock` in `packaging/pam.d/ursine`; daemon rate limiting already covers failed logins.

## Session 2026-10-21 — v0.3 store parity (uncommitted)

**Status**: GUI covers every `packages_nix` daemon action. Backend untouched; no commit yet.

- `client.rs`: typed nixpkgs search, generic package call, daemon-matched timeouts (310 s search, 910 s mutations, 3610 s GC), `asUser` on every store call.
- `store.rs`: search + install, direct install, per-package upgrade/remove, upgrade all, regex upgrade/remove, generations list, rollback, diff closures, wipe history (preview + apply), prune generations (preview + apply), store GC.
- Destructive actions use an inline confirmation panel with exact consequences; dry-runs preview before apply; long output is clipped.
- `app.rs`: store action routing, numeric input validation, post-mutation profile reload, session expiry handling.
- Remaining backend actions wired: `system.status` on Home (version/protocol/auth) and `config.init` in Settings (create commented default config).
- Tests: 12 GUI, 24 API, 6 integration, 58 core = **100 green**; `cargo clippy --workspace --all-features -- -D warnings` + `cargo fmt --check` green in `ursine-ci:1`.
- Docs/state updated: AGENTS parity + UI reference rules, `.opencode/rules/{workflow,ursine}.md`, TASKS, CHANGELOG, session state.

**Next**:
1. Real daemon + GUI smoke for all store actions (display required); verify a real nix profile and long-action feedback.
2. Pre-Commit Review Gate, then commit.
3. Snapshots: implement the btrbk daemon module first, then the GUI panel in the same feature (parity rule).

## Session 2026-10-21 — workflow: frontend/backend parity

**Status**: binding workflow updated (docs only).

- Parity rule added to `AGENTS.md`, `.opencode/rules/workflow.md`, and `.opencode/rules/ursine.md` (invariant 15).
- UI reference rule added (invariant 16): cosmic-settings for COSMIC structure, Webmin for action coverage, Cockpit for capability gating/status/destructive confirmations.
- Backend first, frontend immediately, same feature. No new feature until daemon and frontend reach functional parity. Deferral only with explicit user approval recorded in state.
- Consequence: `packages_nix` store mutations (daemon already complete) are the next work; GUI controls ship before snapshots/users/niri.

**Next**: implement GUI store mutations (search/install/remove/upgrade/rollback/GC/prune) against existing daemon actions.

## Session 2026-10-21 — v0.3.1 RELEASED

**Status**: `v0.3.1` tagged and released. Tag CI run `39602272962` green; `nix/binary.nix` auto-pinned on `main` in `8dada79`; `verify-pin` green.

- Release assets: `ursine-v0.3.1-x86_64-musl.tar.gz`, checksum, `ursine-flake-v0.3.1.tar.gz`, checksum.
- Local Nix profile upgraded: `ursined 0.3.1 (auth: pam)`; activation/deactivation tools present.
- Release policy documented: feature releases bump minor, patches only when explicitly requested, major only when marked in the workflow.

**Next**:
1. Real daemon + GUI smoke for Settings persistence/live gating.
2. Continue v0.3 store mutations/search and snapshots.

## Session 2026-10-21 — settings + manual module modes (uncommitted)

**Status**: committed in `ce74ca0`; released as `v0.3.1` (CI `39602272962`).

- Added a dedicated Settings screen opened by a gear icon in Home's top-right.
- Settings shows detected capabilities, refreshes them, and edits module modes: Auto, Manual on, Manual off.
- Added protocol actions `config.get_config` and `config.set_module_switch`; both are authenticated and audited.
- Daemon persists module switches atomically with `toml_edit`, preserving unrelated TOML/comments/formatting/permissions, and applies the new switch to its live config.
- `manual on` does not install missing dependencies; missing capabilities still render unavailable with reason/fix.
- GUI remains thin; all config mutations go through the daemon.
- Review hardened: atomic config writes use unique `create_new` temp files, fsync, parent/target checks, permission preservation; `config.init` is serialized, refuses overwrite, and refreshes live modules; wheel/as_user/config checks are local; audit records `module=value`; failed-login password is cleared; Home honors manual-off and Settings has retry/empty states.
- Validation: fmt; 24 API tests + 6 protocol integration tests + 58 core tests; 6 GUI tests; clippy for daemon/workspace and GUI with `-D warnings` (`artifacts/logs/settings-gate-final.log`). GUI visual smoke still pending.

**Next**:
1. Run the real daemon + GUI and verify settings persistence plus live gating.
2. Release v0.3.1 when ready so the Nix package contains this feature.
3. Continue v0.3 store mutations/search and snapshots.

## Session 2026-10-20 — README + release CI gate (v0.3.0 RELEASED)

**Status**: v0.3.0 tagged and released. Tag CI run `39452993223` green; `nix/binary.nix` auto-pinned on `main` in `bdbf6ce`; `verify-pin` green.

- Added root `README.md`: intro, Nix install/input instructions, local dev flake, project overview.
- CI `version` job reads `[workspace.package].version`; package runs only for tags/new versions. PR package smoke runs only for source/packaging path changes; `workflow_dispatch` has `force_package` override.
- Added Cargo registry/target cache (`actions/cache`) to the release package job.
- Tag releases verify the published checksum, update `nix/binary.nix` on `main`, and attach `ursine-flake-<tag>.tar.gz` (`flake.nix`, `flake.lock`, `nix/`, `README.md`).
- Added `verify-pin` job: after a tag release, fetches `main` and runs `nix flake check` against the auto-updated pin.
- `getting-started` and packaging spec corrected: flake packages a pinned dynamic-musl release artifact, not a static/source build; `nix run` replaced with `nix build github:skea999/ursine#ursine`.
- Validation: workflow YAML parses; version gate simulation returns `release=false` for current v0.1.0; Nix files parse; README links resolve; pin `sed` substitution tested.
- GitHub Actions: first forced run (cache miss) green in ~18 min; second forced run (cache hit) green, cache `cargo-release-Linux-cfd210d5...` restored 578 MB. Tag `v0.3.0` run green: release assets `ursine-v0.3.0-x86_64-musl.tar.gz`, checksum, `ursine-flake-v0.3.0.tar.gz`, checksum; `nix/binary.nix` auto-pin `bdbf6ce`; `verify-pin` passed.
- `.agent-memory/2026/10/20.jsonl` updated and pushed.
- Implemented the package chain `ursine` (GUI) → `ursined` (daemon + packaging) → `ursine-activate` (tools). Tools detect systemd/OpenRC/dinit, symlink PAM to the stable profile, render the unit against the profile, restart/stop the service, and remove PAM/unit files. Generic profile discovery uses `SUDO_HOME`, then `SUDO_USER`, then the invoking profile, then the system default. Staged activation/deactivation, generic `SUDO_HOME`, package attrs, Nix checks, and shellcheck green.
- Installed `ursine` v0.3.0 into the user Nix profile; `ursined 0.3.0 (auth: pam)`. Host system service/PAM not activated yet (needs root).
- Simplified `README.md` to exactly 3 commands: registry add, default-profile install, and local `sudo "$HOME/.nix-profile/bin/ursine-activate"`. Activation/deactivation tools are packaged with the profile and versioned; tested isolated HOME install and local profile scripts.

**Next**:
1. Activate the host service as root: `sudo "$HOME/.nix-profile/bin/ursine-activate"` (profile upgraded to the versioned-tools revision).
2. v0.3 store mutations/search + snapshots panel.
3. Run PAM smoke on FeralOS and full OpenRC boot on a disposable VM.

## Session 2026-10-20 — packaging review hardening (uncommitted)

**Status**: packaging primitives are review-hardened and locally green; no commit/push yet.

- Fixed review blockers/major findings: restored systemd `[Install] WantedBy=multi-user.target`; split CI build (`contents: read`, checkout `persist-credentials: false`) from a separate publish job (`contents: write`, tags/main only); added `flake` to package `needs`; removed tag `--clobber`; CI now stages and executes systemd/OpenRC/dinit installs and asserts PAM mode `0644` + `auth: pam`.
- Installer hardening: auth gate (stub refused unless `--allow-stub`; cross-arch staging refused unless `--allow-unverified-auth`), `mktemp` temp files, strict path allowlist plus `.`/`..` rejection, PAM mode set on temp before rename (no final-path chmod/chown race), non-root staging ownership warning, config dir `0750`.
- Audit dir is enforced `0750` (rejects root, symlink and world-writable parent); audit file remains `0600` and rejects symlink/non-regular targets.
- Verification: real PAM smoke incl. expired account green (`auth_ok`, bad/expired `unauthorized`, 6 capabilities, no password in audit); installer matrix systemd/OpenRC/dinit green with PAM `0644`, `[Install]`, no temp leftovers; hostile/traversal path rejection and unverified-auth gate green; shellcheck green; Rust gate 81 tests + clippy `-D warnings` + fmt green (`artifacts/logs/rust-gate-final3.log`). Init validation: `systemd-analyze verify` and `dinitcheck` green; OpenRC reached service dependency/cgroup setup but the container has read-only cgroups.
- Release tarball rebuilt from current PAM daemon + GUI: `artifacts/release-final/ursine-test-x86_64-musl.tar.gz`, sha256 `e144a2039c3daaa503e476652d78750645be84ad93d02d1887aedee9ccc9c367`; packaged installer staged systemd/OpenRC/dinit, PAM/version/unit checks green.
- Docs updated: installation guide now covers auth gate, `--allow-stub`, strict paths, staging ownership, and musl/glibc caveat.

**Next**:
1. Boot-test OpenRC on a disposable VM/real target (`systemd-analyze verify` and `dinitcheck` are green).
2. Run first GitHub CI package/publish job and verify tarball + checksum asset.
3. Test PAM login on FeralOS with a real user before v1.0.
4. Complete Pre-Commit Review Gate, then commit/push/tag. No commit yet.

## Session 2026-10-20 — packaging primitives GREEN (uncommitted)

**Status**: cross-distro installer, PAM service and systemd/OpenRC/dinit units are implemented and staged-tested. Real PAM login is green in an Alpine container; FeralOS target login still pending.

- New `install.sh` (POSIX sh): absolute path overrides, `DESTDIR`, PAM `pam_unix.so` detection with distro hints, conflict refusal + `--force` backup, automatic enable/start. `--check` prints the plan.
- New `packaging/`: `pam.d/ursine`, `systemd/ursined.service`, `openrc/ursined`, `dinit/{ursined,ursined.wrapper,ursined.conf}`. dinit follows FeralOS ADR-008 wrapper/config and ADR-009 conventions.
- Release tarball now includes `install.sh` + `packaging/`; CI package job smoke-tests a staged systemd install and checks PAM/GUI/daemon/unit paths.
- Dev flake now stages `packaging/` and `install.sh` beside `bin/`; first `nix build` in `nixos/nix` container succeeded with dummy binaries. `scripts/build-gui-dev.sh` built real release binaries, staged them, and the staged daemon executed; the flake build was not rerun with those binaries.
- Tests: Alpine 3.24 container staging for systemd/OpenRC/dinit + rerun; tarball-layout install; binaries-only install; Debian PAM hint; shell syntax. Rust gate `cargo test --workspace --all-features` + clippy `-D warnings` + fmt green in `ursine-ci:1`.
- Real PAM smoke in `ursine-ci:1`: test user in `wheel`, `/etc/pam.d/ursine` from the asset, correct password → token, wrong password → `unauthorized`, token `capabilities.list` → 6 entries, audit has no password material.
- Docs: `docs/guides/installation.md` (new), getting-started, SPEC-packaging-infra, TASKS, CHANGELOG. State updated. Changes NOT committed.

**Next**:
1. Test the real PAM login path on FeralOS with a user and `/etc/pam.d/ursine`.
2. Run Pre-Commit Review Gate, then commit/push/tag; verify GitHub CI and release tarball.
3. Test installer in a non-Alpine container (Debian) and verify `systemd-analyze`/OpenRC/dinit runtime behavior.
4. Decide NixOS module (`security.pam.services.ursine`) vs installer-only for Nix users.

## Session 2026-10-20 — v0.3 GUI store view GREEN (uncommitted)

**Status**: read-only nix store screen wired to the real `packages_nix` module.

- `store.rs`: installed profile packages + generation history; Home ↔ Store navigation; Open disabled when the Nix capability is missing; busy/empty/error states.
- Client: per-call timeout; profile calls use 75 s (daemon allows 60 s); generations payload parsed and unit-tested.
- Session handling: expired/revoked token on any call signs out and returns to login with "Session expired"; home label corrected to "session lifetime".
- Gate: `cargo build --workspace`, 81 tests (3 GUI + 23 api + 6 integration + 49 core), `cargo clippy --workspace --all-features -- -D warnings`, `cargo fmt --check` — all green in `ursine-ci:1`.
- Docs corrected: module is `packages_nix`, not `packages`. Changes NOT committed.

**Next**:
1. Store mutations/search (`install`, `remove`, `upgrade`, `search`) with confirmation + progress; per-user selector.
2. Snapshots panel: daemon snapshots module still returns `NotImplemented` — implement the btrbk module first.
3. GUI smoke test on a display, then Pre-Commit Review Gate and commit.

## Session 2026-10-20 — v0.3 GUI login + home GREEN (uncommitted)

**Status**: the libcosmic GUI now drives the real protocol: PAM login and capability-gated home.

- New modules: `src/ursine/src/{client,app,login,home}.rs`; `main.rs` is a thin entrypoint. `client` wraps a blocking Unix-socket round-trip in `spawn_blocking` (5 s timeout, `URSINE_SOCKET` override); `app` owns `App`/`Session`/`Message`; login uses `text_input(...).password()`; home mirrors daemon `required_caps` and shows each capability's reason/fix.
- Session token RAM-only; password cleared on success; sign-out clears session and capability data.
- Gate: `cargo build --workspace`, `cargo test --workspace --all-features` (80 tests: 2 GUI + 23 api + 6 integration + 49 core), `cargo clippy --workspace --all-features -- -D warnings`, `cargo fmt --check` — all green in `ursine-ci:1`.
- Permissions fixed: `.opencode/agents/codebase.md` frontmatter is the effective channel (wins over `opencode.json`); minimal set keeps `bash` allow, `nix profile add*` ask, sudo/doas deny, `external_directory` ask. The current session still runs the old set until restart.
- Docs/state updated (TASKS, CHANGELOG, session-state, this handoff). Changes NOT committed.

**Next**:
1. Nix store view (`packages_nix.list_installed` / `packages_nix.generations`) + snapshots panel; wire home module rows to real views.
2. Show daemon version from `system.status`; handle token expiry with a re-login prompt.
3. Run a real GUI smoke test against `ursined` (stub auth) on a machine with a display, then Pre-Commit Review Gate and commit.

## Session 2026-10-19 — v0.1 SKELETON GREEN (code written, committed pending)

**Status**: workspace + daemon skeleton complete, all checks green in container.

- Workspace: `src/ursine-api`, `src/ursine-core`, `src/ursined`, `src/ursine` (GUI placeholder), `tests/ursine-api/` integration wired via `[[test]]`.
- 55 tests green (21 api unit + 6 api integration + 28 core unit); clippy `-D warnings` + fmt green incl. `--all-features` (pam).
- E2E smoke in podman: auth → 64-hex token (TTL 600s) → system.status → capabilities.list (6) → bad token → unauthorized → audit JSONL lines (auth/status/list) → SIGTERM shutdown clean. Binary dynamic musl (ldd).
- Auth: `pam` crate pinned **0.7** (pam-sys 0.5 pregenerated bindings — 0.8 pulls bindgen/clang, hostile to musl containers). Feature `pam`; dev builds use loud STUB auth (grants wheel, WARN per login).
- Build: `.cargo/config.toml` `target-dir=artifacts/target` + musl **dynamic** (`-crt-static` off; static musl cannot link libpam.so). Dev image `ci/Containerfile.dev` (`ursine-ci:1`); podman needs `--network=host` on this machine (pasta/tap broken).
- `flake.nix` (pkgsMusl, x86_64, feature pam) + `flake.lock`; `nix flake show` eval green. Full nix build NOT run locally (25 min ephemeral-container attempt timed out) — CI flake job is the gate.
- CI `.github/workflows/ci.yml` ACTIVE: matrix `default`/`pam` (alpine:3.22 = FeralOS target), clippy+fmt, nixos/nix flake check/build, release-on-tag musl tarball.
- Docs patched: SPEC-packaging-infra (pkgsMusl dynamic, CI matrix, dev image), architecture overview, getting-started (podman dev-image commands), AGENTS stack commands, TASKS.

- **Audit fix (CI-driven)**: `pam 0.7` pulled vulnerable/unmaintained `users 0.8` (RUSTSEC-2025-0040, no fix). Swapped to **`pam-client 0.2`** (maintained fork, pam-sys 0.5 pregenerated, no `users`, no bindgen). API: `Context::new(service, Some(user), Conversation::with_credentials(user, password))` + `authenticate(Flag::NONE)`.
- **CI hardening**: `NIX_CONFIG` enables flakes globally; vet job builds cargo-vet from source (taiki-e prebuilt is glibc → not executable in musl container); Semgrep findings annotated with justified `nosemgrep` (temp-dir in tests; args parsing strict). New jobs: vet (cargo-vet, 57 exemptions), audit (cargo-audit/RustSec), sast (Semgrep p/rust), publish (flake archive artifact).

- **Nix strategy (final)**: nixpkgs does NOT cache `pkgsMusl`/`pkgsStatic` (verified on stable nixos-26.05: rustc/linux-pam NOT in cache.nixos.org). `pkgsStatic` is also impossible for us: PAM modules are `dlopen`ed at runtime and static musl has no dlopen. Decision: **Alpine (rust:1-alpine) builds the dynamic-musl binaries; the flake packages the pinned release artifact** (`nix/binary.nix` URL+sha256, `stdenvNoCC`, builds in seconds). Release v0.1.0 created with the tarball; `nix flake check` + `nix build` green locally in nixos/nix container. Release checklist: bump `nix/binary.nix` on every tag.
- **CI jobs final**: test matrix, lint, vet, audit, sast, flake (pinned package), package (Alpine release build; tag release or rolling `latest` on main).

**v0.2 DONE (2026-10-19)**: nix module on the NEW CLI only (`nix profile ...`);
actions list/add/remove/upgrade/search/history/rollback/wipe-history/diff-closures;
exec `su -l -s /bin/sh <user> -c 'nix ...'` with strict validation, uid 1000..60000,
timeouts; parsers for profile-list/search JSON. 68 tests, clippy/fmt green.
Also: **storeGc** (root `nix store gc`, total) and **pruneGenerations** (keep last N,
dry-run plan from `nix profile history`, exec via `nix-env --delete-generations +N`).
**Verified against the VM's real nix 2.31.5** (read-only): `profile list --json`
schema matches our parser; `profile add` exists; `upgrade/remove` support
`--regex` and `remove --all` (now implemented); `history` emits ANSI escapes and
has no `(current)` marker -> current generation read from `profile-N-link`;
`--delete-generations +N` requires **root** (users cannot lock store profiles).
78 tests green. Remaining real-op e2e: FeralOS (daemon + non-root user).

**GUI stack decision (2026-10-19, same session)**: desktop shell stays **Wayle (GTK4)**; the app moves to **libcosmic** (COSMIC toolkit, iced-based) — security over visual coherence (no GTK/gdk-pixbuf/librsvg in the GUI process). Pinned `libcosmic` rev `430f3d48a97d8779f9c1985473912e594ccca1ae`. Distro already mixes COSMIC apps (cosmic-edit/player) on niri, so mixed design languages accepted. **Web GUI (v3) must also be memory-safe** — stack undecided; Rust→WASM preferred (see `docs/specs/SPEC-web.md`). UX principles remain toolkit-independent.

**v0.3 SPIKE DONE (2026-10-19)**: libcosmic pinned rev **builds on musl** in the dev container (`cargo build -p ursine`, deps: cmake/fontconfig/freetype/xkbcommon/expat/pkgconf). Minimal window shows daemon status via a real socket round-trip (`capabilities.list` unauthenticated → expected `unauthorized`; `URSINE_SOCKET` env override). `cargo clippy -p ursine -D warnings` + fmt green. CI: new `gui` job (build+clippy); daemon jobs use `--exclude ursine` so they stay fast. Vet re-generated (596 exemptions from the libcosmic tree).

**Next (v0.3 screens)**: login (PAM prompt) → home cards → nix store (list/search/install/remove/GC/generations) → snapshots. Also: dinit service + `/etc/pam.d/ursine` files for packaging.

## Session 2026-10-16 — INTENT CONFIRMED (NO CODE YET)

**Confirmed Statement of Intent**: `docs/intent/ursine.md` — explicit yes from skea999. Version plan:

- **v1**: backend skeleton + GUI libcosmic (mandatory; amended 2026-10-19 from GTK4): nix packages, users, snapshots, niri editor
- **v2**: apk + flatpak; printers, scanners, bluetooth, network, firewall, disks (all D-Bus/CLI, capability-gated) — no OIDC
- **v3**: OIDC + web (web = LAST frontend)
- **v4**: Incus UI
- **v5**: ZFS + Pirateus
- **v6**: other distros (pkg/init abstraction, glibc revisited)

**UI principles (binding)**: all UIs behave as one product (same layout/navigation/terminology per CONTEXT.md); user-friendly (COSMIC conventions, capability-gated panels never broken-looking, confirm destructive actions, plain language). Visual theming follows each toolkit; pixel-identical across toolkits not required (amended 2026-10-19).

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
