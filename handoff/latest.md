# Handoff

**Active state**: `state/session-state.json`
**Milestone log**: `AGENTS.md`

See `state/session-state.json` for current goals, decisions, risks, and next actions.
See `AGENTS.md` for project context and milestone history.

## Session 2026-11-09 — Uniform backup rotation: `[backup_configs]` (main, commit `2fa4d0e`)

**Status**: Pushed `13a6b4b..2fa4d0e`, CI `42599246428` watched (pending green at handoff). User request: ONE uniform backup mechanism for every config file, count from a settings file (`[backup_configs] copies = 3` in `/etc/ursine/config.toml`, clamp 1..=10, restart to apply), backups also for files changed via CLI (crontab named explicitly, theming confirmed).

- New `ursine_core::backup_configs`: slot naming `<file>.ursine-backup[.N]`, rotate BEFORE live replace, rotation failure aborts the write (shell exit 42); legacy schemes (timestamped restore/scanner copies, dot-prefixed atomic slots, btrbk `.conf.bak`) pruned to the same cap. `rotate`/`rotate_async`/`rotate_move`/`rotate_store` + owner chown for user-home files; NotFound-tolerant shift (concurrent writers).
- Direct writes covered: config.toml (governs its own backups), niri/apk/flatpak shell scripts (shared `ROTATE_FN`, behavioural sh test), theming `write_owned` chain (copies threaded end-to-end, owner-aware), COSMIC entries, dconf (rotated ONCE per apply), airscan.conf, btrbk.conf, snapshot restores (`rotate_move`; btrfs resolved before rotating; blocking-pool move; rollback errors surface the slot).
- CLI-changed files rotated before the CLI runs: crontab → `/var/backups/ursine/crontab.<user>` (0600, fail-closed), nix registry JSON, flatpak `repo/config`, `/etc/apk/world` (add/del), `/etc/firewalld`, NM `system-connections`, `/etc/cups/printers.conf`, account files (passwd/shadow/group/gshadow). Excluded with rationale in SPEC-daemon-core: nix profile generations, flatpak/apk DBs, systemd symlinks, audit JSONL.
- Reviews: 2 passes (2m+4l, then 1h+4m+5l) — all fixed; 1 scanners finding rejected (code already `symlink_metadata`). Gate: fmt/clippy/tests/static/docs/typos all PASS.
- Env fix: host `git credential.helper` (libsecret) hangs after server restart → repo-local `credential.helper = !gh auth git-credential`.
- VM smoke pending: rotate a config twice (verify slots), crontab set (check `/var/backups/ursine`), nix registry add/remove (check `~/.config/nix/registry.json.ursine-backup`), `[backup_configs] copies = 1` behavior.

## Session 2026-11-06 — Printers live job progress (main) — v2 CODE LIST CLOSED

**Status**: DONE — `3fddaf0` CI-green. Printers screen subscribes to a 3 s tick while visible; the silent load path avoids the busy flicker, never clears a persistent error before the reply, and may overlap an in-flight read so a finished mutation's reload is never dropped (newer reply wins). Review: 3 medium fixed. TASKS Printers line ticked.

- **This was the last required v2 code item.** Remaining v1.0 closure work is user/VM-side: APKBUILDs in `feralos-aports`, real FeralOS PAM/activation/module smoke, then the v1.0 tag.
- VM smokes pending for this week's features: flatpak override write path (get/set + Session Bus policy survives), firewall toggle + test service, disks loop-device format/partition with `disks_format=true`, bluetooth legacy-PIN + numeric pairing, printer live refresh.

## Session 2026-11-06 — Bluetooth pairing agent complete (main, 1 commit)

**Status**: DONE — `7e442e3` CI-green. `set_bluetooth_pairing_pin` keeps the PIN in daemon RAM only (flag-only on the wire, stdin on the CLI, audit logs action names only) and registers a zbus `org.bluez.Agent1` (KeyboardDisplay + SetDefaultAgent) that answers RequestPinCode/RequestPasskey; RequestConfirmation matches numerically (leading-zero PINs included) and DENIES when unset — the clear path runs UnregisterAgent + drops the export (race window closes with deny, not auto-accept). Registration is mutex-serialized with export rollback; PIN stored only after registration succeeds; without a PIN the legacy `--agent NoInputNoOutput` pair path stays. GUI: password input + status flag. Review: 1 high + 4 medium + 3 low fixed (1 false positive). TASKS Bluetooth line ticked.

- VM smoke: pair a legacy PIN device (alphanumeric ok) and a numeric-comparison device (numeric PIN required).
- Next: **Printers live job progress** — the last v2 code item; after that only APKBUILDs in feralos-aports, real FeralOS smoke and the v1.0 tag remain (user/VM work).

## Session 2026-11-06 — Repositories: apk + nix + flatpak (main, 4 commits)

**Status**: DONE — all four commits CI-green: `80c686d` APK add/remove/mask (`@tag url` per apk-repositories(5) = MASKED repo unless world pins `pkg@tag` — the `@edge` ask; upsert semantics: re-add updates the tag, empty tag/Unmask clears; atomic write + backup + `apk update` after each mutation), `f1c838a` test-thread stack fix (the growing `Action` enum pushed debug frames past libtest's 2 MiB default → `RUST_MIN_STACK=16777216` in CI Test/gui jobs + the canonical test command in build-and-test.md), `1724ca6` Nix flake repositories via the NATIVE registry (decision recorded: no ursine-side state file; `repo=` search prefixes bare installables; GUI dropdown + Flake repositories section; global entries read-only), `1605cfd` Flatpak remote-add/delete (scope captured when the confirm opens; URL charset = installable-atom so root/su guards agree). Protocol bumped to **v4** (`list_apk_repositories` rows are `{url, tag}`).

- Review closure: apk 3 high + 2 medium + 4 low (1 low rejected: 0600 backup = established niri/flatpak pattern), nix 3 medium + 5 low, flatpak 3 high + 3 medium + 1 low (1 low with recorded rationale: panel-invisible `job` flag, guard stays in the dispatcher, inputs survive). CHANGELOG's three Unreleased blocks merged into one.
- Also fixed: `Search` action gained `repo` (serde default, old clients ok); `Action::Search { repo: None }` literals updated in tests.
- VM smoke: apk add a repo with `@edge` tag then Unmask; nix add/remove a registry entry + `search repo=…`; flatpak add a remote + Remove (scope flip while confirm open must not hit the other installation).

## Session 2026-11-06 — Disks format/partition complete (main, 1 commit)

**Status**: DONE — `1ebcf8b` CI-green. Destructive trio (`format_disk`, `partition_create`, `partition_delete`) behind the triple gate recorded in session-state key_decisions: `disks_format = true` config key (default off, restart required), `confirm_device` typed exactly equal to `device`, CLI `--yes`. Safety rails: mounted target/children rejected via lsblk, `parted print` pre-flight turns the blank-disk case into a clear "create a partition table first" error, `DISK_FSTYPES` shared between daemon and GUI through ursine-api, GUI flows one-at-a-time with input preserved on errors and Mount disabled while a flow is open. Review: 10 findings (4 medium) all fixed; guide `--yes` list also backfilled with the firewall/network/flatpak names it had missed. TASKS Disks line ticked.

- VM smoke: attach a loop-backed image, format it, create/delete a partition with the flag on; verify the gate message with the flag off.
- Next: **Bluetooth pairing agent PIN/passkey** (TASKS v2), then printer live job progress.

## Session 2026-11-06 — Firewall extras complete (main, 1 commit)

**Status**: DONE — `72c3036` CI-green. Enable/disable = default-zone target toggle (firewalld has no global switch; decision recorded in session-state key_decisions: `enabled=false` sets ACCEPT, `enabled=true` REJECT, permanent+reload, DROP→REJECT round-trip documented, GUI confirms before disable, CLI treats it destructive). Custom service definitions create/delete with a single `ls /etc/firewalld/services` listing (no per-service `--path-service` N+1). Status now carries the active-rules summary: per-zone `target`/`interfaces`/`rich_rules` (from `--list-all` + `--list-rich-rules`), derived `enabled`, `custom_services`. Review: 9 findings (1 high — delete confirm, 2 medium — shared name input + N+1) all fixed. TASKS Firewall line ticked; zone create/delete stays out of scope (not in plan lines).

- VM smoke: toggle filtering twice (confirm path), create + delete a test service, check zone rows show target/interfaces.
- Next: **Disks format/partition** (TASKS v2: explicit destructive confirmation, exact device typed — SPEC-disks already sketches it), then Bluetooth pairing agent PIN/passkey, printer live progress.

## Session 2026-11-05 — Network v2 detail complete (main, 2 commits)

**Status**: DONE — two slices, each with gate + review + green CI: `b667a79` IPv4 DHCP/static per connection (`SetNetworkIpv4`, per-connection settings in `get_network`, GUI editor with reconnect confirm; blocker: the argv guard rejected the empty `""` values nmcli uses to clear properties — now shared `run_root_program_guarded` allows empty args and space-safe names via `run_root_program_value`, which also fixes connection names like `Home Wi-Fi`) and `530667c` forget + VPN (`ForgetNetwork` destructive, `SetNetworkConnection` up/down non-destructive per the DisconnectNetwork precedent + SPEC-confirm-list, saved profiles with kind/active in the view, VPN section for `vpn`/`wireguard`, destructive Forget confirmation). TASKS Network line ticked.

- Review totals: 11 findings (1 blocker) + 5 findings, all fixed except one accepted with rationale (forget confirm clears before dispatch — re-click on error, no input lost).
- Gotchas: nmcli settings fields only exist per connection name (no blanket listing → one call per connected device, documented); `:` is the terse column separator so connection names reject it; `connection down` errors on inactive profiles (surfaced verbatim).
- Open: VM smoke (set ipv4 on a test profile, forget a test Wi-Fi profile, connect/disconnect the `niuma` wireguard).
- Next: **Firewall extras** (TASKS v2: enable/disable, zone management, custom services, active rules summary — some already shipped, verify against SPEC first), then Disks format/partition, Bluetooth pairing agent, printer live progress.

## Session 2026-11-04 — Flatpak v2 detail complete (main, 5 commits)

**Status**: DONE — five slices, each with its own gate (422 tests at the end), review cycle, static/docs PASS, push and green CI: `26707ae` remote dropdown + install sizes (`get_flatpak_info`, two-step confirm with sizes), `7a57d81` app/runtime split (`kind` from `--app`/`--runtime` queries + Show dropdown), `a5bcf23` update-available list (`list_flatpak_updates` + per-app Update), `48cd194` permissions editor (override keyfile edited directly so foreign groups survive; GLib env escapes; 14 findings incl. 1 high), `f1b020e` `as_user` user-scope targeting (own-account rule for non-root, remotes None-scope rejects the target). TASKS Flatpak line ticked; plan v2 polish for Flatpak empty.

- Design notes worth remembering: `flatpak override --reset` deletes [Session Bus Policy] — never use it for partial edits; `ref` column has no app/runtime prefix (split needs two queries); `remote-ls --updates` is the update oracle; miette-style flatpak errors come from `remote-info` human layout.
- Dispatch `as_user` now covers packages_nix + theming + crontab + Flatpak(user scope); messages updated, contract tests still green.
- Open: VM smoke of the permissions write path (get/set on a real app, `flatpak override --show` reflects it, Session Bus policy survives); pre-existing epoch/scope race in the package panel recorded as risk (not introduced here).
- Next: **Network DHCP/static IPv4, forget, VPN** (TASKS v2 required), then Firewall extras, Disks format/partition, Bluetooth pairing agent, printer live progress.

## Session 2026-11-04 — niri compatibility range + field-level validate errors (main)

**Status**: DONE — commit `b01f0a2` (13 files, +643/-31); full gate green (401 tests, clippy `-D warnings`, fmt, `static-check all` PASS, docs PASS). Review cycle: 6 findings fixed before commit. Pushed with the harness commits.

- Plan line: features-v1-v2.md "schema/version check against `niri --version`" + "validation error mapped to the field where possible" (TASKS v1.0 closure item now `[x]`).
- Compatibility: `parse_niri_version` (CalVer `YY.MM[.patch]`, tolerates `niri 25.11 (unknown commit)`) vs `NIRI_MIN_SUPPORTED = 25.05` (const tuple, test ties string↔tuple); view exposes `version_parsed`/`version_supported`/`min_supported`; GUI Compatibility row warns below the floor — `niri validate` remains the hard gate (SPEC rule).
- Field errors: `validate_errors.rs` parses miette stderr (real fixtures captured on host niri 25.11: unknown node + unclosed brace) → `{message,line,column,section,field}`; underline row wins over header, column only kept when header line == row, spanless wrapper blocks emitted with cause chain (`├─▶`/`╰─▶` merged), `/* */`+`//`+strings stripped before depth/section mapping (`section_key` translates niri node → managed section).
- GUI: structured list via `niri_validate_detail`, raw stderr fallback when no entry carries a location. CLI: `response_exit_code` → 1 on `valid=false` (guide exit table + niri section + man page updated). Parity: no new action (response-shape only) so daemon+GUI+CLI shipped together.
- Gotchas: miette header coordinate pointed at the enclosing node (2:1 vs underline line 3) — never pair them across lines; `cargo fmt` before clippy caught nothing, typos flagged `unparseable`.
- Next: **Flatpak detail** (remote dropdown, app/runtime split, update-available list, permissions editor, size/progress, target user — TASKS v2 required), then Network static/forget/VPN, Firewall extras, Disks format/partition, Bluetooth pairing agent, printer live progress.

## Session 2026-11-04 — OpenCode V2 harness migration (main)

**Status**: DONE — commit `1e3dcc2` (43 files: pack + config + pre-commit allowlist), main ahead origin by 1. Rules plugin (`opencode-rules` 2.0.0) confirmed V2-native, loads clean.

- Root cause of "plugin error + agenti invisibili": opencode auto-updated to v2.2.1 — V1 plugin API no longer runs, and V2 honors `hidden: true` (16 agents had it since scaffold).
- `agents-opencode` ported to V2 `Plugin.define({id, setup})` with `tool.hook("execute.before")` (read guard verified blocking `.npmrc`), `session.hook("compaction")`, `shell.hook("create.before")`, `event.subscribe()`; `plugin list` shows it loaded.
- Full V2 conversion: `opencode.json` → `plugins`/`permissions`/`agents` (bash→shell, task→subagent, doom_loop dropped — not a V2 action, external_directory ask kept); 23 agent frontmatters → `permissions` arrays, `temperature` dropped; 16 commands `subtask`→`subagent`; redundant config permission copies dropped (frontmatter authoritative). Converter: `artifacts/opencode-v2-migration/convert.py` (idempotent).
- Pre-commit scan now reports `file:line` and honors `.githooks/secret-allowlist` (1 approved false positive: security-auditor.md:76 sample password). Never `--no-verify`.
- Agent invocation in V2 TUI: **Shift+Tab** = agent.cycle, **Ctrl+X a** = agent.list (leader ctrl+x), Ctrl+p = palette; subagents launch via natural language or pack slash commands (`subtask`→`subagent` = background child session). No Tab anymore.
- Gotchas: `opencode run --agent review` resolves but fails on `crouton-alpha-free` with "This model is not available in your country" (provider/geo, unrelated); `npx agents-opencode --update` will restore V1 frontmatter + `hidden: true` — re-run converter + `sed -i '/^hidden: true$/d' .opencode/agents/*.md`.
- Next: push `1e3dcc2`; user picks next v1/v2 feature item (see session-state next_actions).

## Session 2026-11-02 — cross-user admin via CLI (main, untagged)

**Status**: DONE — releases `v0.25.0`…`v0.25.4` (pins `27d4451`, `2f4a695`, `2052bda`, `d744a21`; every tag CI green incl. package/publish/verify-pin). **VM smoke PASSED: 0 failures, 1 expected SKIP** on 0.25.4 (artifacts/vm-smoke-0.25.1.sh — read-only battery + refusal guards + as-found assertion). Findings fixed across the loop: nix packaging never shipped ursinectl (flake+activate), five su builders rejected root targets (root-direct sweep), OpenRC tools missing the busybox /bin paths, crontab via `crontab -u` (no su/suid). Gotchas learned: nix caches github HEAD for 1h (`tarball-ttl`) and profile entries pinned to a `rev` never move on upgrade.

- Root principals may now target any EXISTING normal-login account for theming/crontab/nix (dispatch validates the foreign target BEFORE the capability gate: typed `NotFound` naming the user); the own-account wire rule still binds every non-root principal (review-hardened boundary unchanged).
- Existence errors name the user (`user X does not exist`) in both the su path and theming identity.
- Fixture-account integration test: plant a DMS source in another user's home → detect/get/apply theming (cross-user write proven: GTK `settings.ini` lands in their home); cross-user crontab read accepted with the CI busybox missing-suid limit documented — **VM smoke must confirm FeralOS ships a suid crontab** (else user crontabs fail system-wide).
- SPEC-crontab/SPEC-theming/USAGE updated; decision recorded in session-state key_decisions.
- Crontab reworked (user directive): `crontab -u <user>` root-direct everywhere — no su, no suid dependency (busybox fresh-user list wording mapped to "no crontab"). Impersonation kept only for nix/flatpak/niri/gsettings/scanimage (rationale in session-state key_decisions).
- Next: VM smoke on this machine — nix update (pin 0.25.0) or tarball install.sh, then `ursinectl theming theme_get --as-user <user>`, `ursinectl crontab crontab_get --as-user <user>` (works without suid now), caps, apk job stream.

## Session 2026-10-30 — ursinectl CLI released (v0.24.0)

**Status**: DONE — `v0.24.0` published (4 assets), nix auto-pin `f8d3b4c`, tag CI 41116788684 green (version/lint/test/package/publish/verify-pin), main CI 41116780409 green.

- **CLI decision (supersedes 2026-10-27 "thin client")**: `ursinectl` is an in-process host of `ursine-core` (same `Daemon::handle` as the GUI), hard root gate (`/proc/self/status` euid==0, no override, fails closed), chroot-capable (no daemon/socket/dinit; absolute paths resolve in the target). Auth: NoAuth rejects every `auth` request. Jobs under `/run/ursinectl/jobs/<pid>-<seq>` (per-process: the registry purges its dir at startup — fixed concurrent-run clobbering).
- **Surface**: generic serde-driven `<module> <action> key=value...` — every API action reachable by construction (parity auto); values JSON-or-string, `key=@file`, `key=-`; secrets only `key=-` (stdin); unknown keys rejected; numbers coerce into string fields (`port=8080`); `--json`/`--yes`/`--as-user`/`--config`; job mutations stream the log to stdout and exit with the job's code; exit codes 0/1/2. Crontab/theming of root stages per-process (`/root`, direct tool, no `su`).
- **Parity rule (binding)** updated in AGENTS/workflow/ursine: every feature ships daemon + GUI + CLI in the same task.
- **Tests: 381 workspace-wide (95 CLI)** — 21 unit (incl. drift guards: ALL_ACTIONS registry, USAGE coverage, exactly-one-sample, destructive table), 24 E2E (real binary: exit codes, job path with real apk job, crontab round-trip/boundaries/@file, unsafe configs, non-root gates), 46 in-process (19 per-module dispatch sweeps over the 100-action SAMPLES table, wrong-module rejection sweep, audit sweep, response-shape contract, protocol/auth boundaries, byte-exact log cursors, stream poll-boundary/UTF-8 reassembly), 4 samples; core 194 (staging guard). Test-support code lives only in `tests/` (proved absent from the product binary via `strings`).
- **Product bugs found by tests**: fixed staging/temp names raced across concurrent writers (ENOENT + wrong-payload install risk) → per-process names; three static-gate findings (typos/ec/taplo); rustdoc `argv[0]` broken link (first tag attempt failed lint; tag re-cut on `967fa11`).
- **Gates**: static-check ALL PASS, docs PASS, default+all-features matrices 381 green, install.sh staged smoke installs ursinectl to `/usr/local/bin`.
- Next: VM smoke of 0.24.0 (install tarball → `ursinectl system status`, caps, apk job stream, crontab; chroot install flow) + push the ursinectl guide in docs when requested.

## Session 2026-10-30 — log search + crontab released (v0.23.0)

**Status**: DONE — `v0.23.0` published (4 assets + checksums), nix auto-pin `1cf222b`, tag CI 41055001521 green incl. package/publish/verify-pin.

- Commits: `cf78b64` log search + crontab section; `caaa527` Tier-2 review fixes (rebased onto pin `a346e4f`); `05c3395` `chore(release): 0.23.0` (workspace was still 0.22.1 — first tag cut without the bump failed the CI version gate, package/publish skipped; tag re-cut on `05c3395`).
- Tier-2 review (security/code/requirements), all confirmed findings fixed: crontab cross-user `as_user` rejected in dispatch (+ test); bounded `crontab -l` (oversize = error, never silent truncation a save would propagate); trusted absolute tool path (user PATH cannot plant a fake `crontab`); search per-line bound + NUL rejection + unterminated-tail delivery; follow supersession (jump gated on `!busy`, stale Follow polls dropped); `log_read` audited for End/Before (only From-mode follow polls exempt); spawn_blocking staging; save/get race gates; shared `max_log_lines()`/`default_log_search_limit()`; truncated-scan note wins over "No matches"; SPEC-logs/SPEC-crontab updated.
- Niri resolution question answered: already implemented — Niri → Outputs (mode like `3440x1440@60.000`, position X/Y, scale, transform, VRR, "Add output section"); `niri validate` + atomic write; niri live-reloads the file. No new work.
- Gate at release: 285 tests (44 GUI / 42 API / 6 integration / 193 core + 1 ignored), clippy `-D warnings`, fmt, docs PASS.
- Next: VM smoke on 0.23.0 — install release tarball or nix update + `ursine-activate`; check Logs (search jump, tail -f, load-previous), Crontab on busybox `crontab` (wording of "no crontab" message), Niri Outputs mode edit.

## Session 2026-10-27 — uniform theming implemented + review-closed (uncommitted)

**Status**: gate green — 261 tests (core 179+1 probe, GUI 40, API 36, integration 6), clippy `-D warnings`, fmt, static-check PASS, docs PASS, live probe against the real DMS config on this VM PASS. Uncommitted; top bar specs pending from the user.

- New `theming` module (detect/get/apply) + GUI section: reads Wayle/DMS config (running Wayle of the session uid wins, else DMS; inactive without a shell) and propagates one theme to GTK3/4/2, qt5ct/qt6ct, KDE, Flatpak overrides, COSMIC cosmic-config (systemic), `environment.d`, Firefox, VSCodium, terminals (alacritty import fix, kitty, ghostty, foot), cava, Electron flags. Protocol additions: `ModuleId::Theming`, `Action::ThemeDetect/Get/Apply`, `Capability::Theming` (any login user), `ModulesConfig.theming`.
- Tier-2 review: 4 lenses, ~45 findings (3 blockers: Firefox profiles.ini escape → root write; give_tree symlink chown; missing apply confirmation) — ALL fixed; rejected with rationale: env first-apply convergence (impossible case), UX global items already covered.
- Security hardening: component-wise home path check + `O_NOFOLLOW` temp + stale-tmp cleanup (residual TOCTOU documented), symlink-skipping chown walker, label sanitization (`\n`/`=` blocked), as_user theming own-account only, marker-ownership everywhere (foreign files preserved → `skipped`), 8 MiB read cap, JSONC-tolerant VSCodium merge.
- Plan/docs: SPEC-theming (status, KDE/portal rows, capability note), intent uniformity amendment, TASKS, CHANGELOG; settings switch + home capability card for Theming.
- Next: commit (Tier-2 closed), then user decides push/tag (v0.20.1?) for VM smoke; top bar specs from the user.

## Session 2026-10-27 — dinit boot fix + CLI in plan

**Status**: post-0.20.0 fixes; uncommitted.

- dinit service aligned with the working feralos-aports unit (`src/ursine/ursined.dinit`): `depends-on = local.target`, `before = login.target` (logfile in the `/var/log` fstab subvol must open after that mount — missing targets were why boot start failed), `restart = false`; wrapper now sources the env-file for manual starts; installation guide updated. shellcheck/shfmt/docs PASS.
- CLI decision: thin `ursinectl` added to the v3 plan wave (commit `60ee2b2`).
- Next: commit dinit fix; decide whether to cut v0.20.1 or move the v0.20.0 tag before the VM smoke.

## Session 2026-10-27 — package progress implemented + review-closed (0.20.0 ready)

**Status**: DONE — Tier-2 review closed (4 lenses: requirements/code/security/ux), every confirmed finding fixed; gate green (210 tests: 135 core / 37 GUI / 32 API / 6 integration; clippy `-D warnings`; fmt; static PASS; docs PASS); version 0.20.0; commit `611f263`; tag `v0.20.0` (not pushed).

- Package progress (P0, binding rule) implemented backend + GUI: `ursine-api` job types + `Action::JobOutput` + protocol v3; `ursine-core` `jobs.rs` registry (RAM, `/run/ursine/jobs/<id>.log`, 1 MiB cap + truncated, TTL/lost sweep, audit on start/complete, poll not audited); APK/Flatpak/Nix mutations return `{"job_id":...}` and run via job log (apk `--no-progress` removed, `RefreshApk` stays sync, previews/dry-run/empty stay sync).
- GUI: `client::job_output` + poll chain (`JobPolled(epoch, job_id, …)`, 400 ms delay, snap-to-bottom) + terminal box `job_panel` (monospace 16 KiB tail, Close on done/detached, busy held until completion, reload on done, epoch/sign-out guard).
- Review fixes (all confirmed findings, 4 lenses): single tracked job across both panels (dispatch guards + `track_job` fallback + stale-poll drop by job id); drain full 64 KiB pages after done (tail loss); JoinHandle monitor closes panicking workers immediately; completion audit + sweep file IO moved outside the jobs lock; startup purge of stale job logs; prune job re-validates profile at execution; `JobOutput` principal binding (origin+user, Forbidden on mismatch); flatpak system jobs audit `as_user=None`; poll failure keeps the log box detached with Close; specs fixed (`--no-progress` rows, job-log design supersedes `progress.rs`); fail-closed registry documented.
- Rejected as enhancements (not defects): global cross-screen job indicator; keyboard-focus verification of Close; Close-on-owning-tab duplicate feedback (owning panel keeps the outcome message).
- Docs/state: AGENTS package-progress rule (implemented), SPECs, TASKS, CHANGELOG → `0.20.0 — 2026-10-27`, session-state key_decisions (incl. RefreshApk sync approval), memory saved.
- Next: final static + docs gates, commit, tag `v0.20.0`, then user VM smoke (daemon + GUI live output + Close).

## Session 2026-10-27 — package progress implemented (uncommitted)

**Status**: Tier-1 gate green (207 tests: 132 core / 37 GUI / 32 API / 6 integration; clippy `-D warnings`; fmt); static-check PASS in `ursine-static:1`; docs pending check; no commit (orchestrator Pre-Commit Review Gate next).

- Package progress (P0, binding rule) implemented backend + GUI: `ursine-api` job types + `Action::JobOutput` + protocol v3; `ursine-core` `jobs.rs` registry (RAM, `/run/ursine/jobs/<id>.log`, 1 MiB cap + truncated, TTL/lost sweep, audit on start/complete, poll not audited); APK/Flatpak/Nix mutations return `{"job_id":...}` and run via job log (apk `--no-progress` removed, `RefreshApk` stays sync, previews/dry-run/empty stay sync).
- GUI: `client::job_output` + poll chain (`JobPolled`, 400 ms delay) + terminal box `job_panel` (scrollable exact text, Close only when `done`, busy flag held until completion, reload on done, epoch/sign-out guard).
- Added dispatch test: invalid/unknown `job_id` rejected, polls absent from audit log.
- Docs updated: AGENTS package-progress rule (implemented), SPEC-packages-nix, TASKS, CHANGELOG; state/session-state.json refreshed.
- Next: Tier-2 review lenses on the frozen diff, fix findings, commit; then remaining v1/v2 details.

## Session 2026-10-27 — post-0.19.0 v1/v2 work

**Status**: CI green `40537405247`; changes on main, no new tag.

- Niri: output mode/position, animation slowdown + window-open/close duration/curve, version gate.
- APK: per-package upgrade, update-available list, repositories, install preview, protected warnings.
- Flatpak: per-app update, configured remotes list. Bluetooth battery. Disks UUID/usage/SMART. Printers CUPS options + job state. Network IP/gateway/DNS + Ethernet.
- Package progress decision recorded: job log + `job_output` polling, terminal text, no percentage, Close after completion. Not implemented.
- Next: implement package progress, then remaining v2 details, review/release.

## Session 2026-10-26 — Niri v1 outputs (in progress)

- Added Niri output `mode` and `position` (x/y) through API, KDL read/write, and GUI.
- Added `niri --version` to the config view and GUI; compatibility gate and field-level validation errors still pending.
- Plan traceability rule added to AGENTS/workflow: no scope outside intent/inventory/SPEC; scanner config-only; autologin excluded.
- Added APK per-package upgrade/update list/repositories/protected warnings, Flatpak remotes list, APK install preview, Flatpak per-app update, Bluetooth battery, disk UUID/usage/SMART, CUPS printer options, Niri animation timing/version gate, and network IP/gateway/DNS + Ethernet, and printer job state details.
- Next: Niri version gate/field errors; APK repos/size/progress; Flatpak remotes/runtimes/permissions; network/firewall; disks format; printer job progress.

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
