# Handoff

**Active state**: `state/session-state.json`
**Milestone log**: `AGENTS.md`

See `state/session-state.json` for current goals, decisions, risks, and next actions.
See `AGENTS.md` for project context and milestone history.

## Release 0.49.0 — open-issue backlog + action catalogue (2026-11-23)

**Status**: tag `v0.49.0` at `bc7f8ce`, tag CI `45230005789` green, release assets published (musl tarball + flake source), nix pin `86bc581`. Host update pending: user runs `scripts/test-host.sh` after the tag.

- 17 issues fixed: niri KDL v1 + `xcursor-*`; `as_user` for niri and flatpak remote/regex; firewalld stand-alone `--set-default-zone` + typed refusals; OpenRC/dinit idempotency; missing-profile prune; AppArmor backups outside `/etc/apparmor.d`, `aa-cleanprof -s --no-reload`, easyprof `template_vars`; btrbk one managed block + runnable init probe; restore refuses a mount-point target; users actions accept `name=`; `url` 2.5.9 (RUSTSEC-2026-0592).
- PR #19 (same backlog, independent) closed by its author; its action-catalogue commit `bb3df93` was cherry-picked with author credit: `ursinectl actions [--json]` (`ursine.actions/1`), `Action::spec` exhaustive, `is_read_only`/`is_destructive` in `ursine-api`.
- Gate: local `scripts/dev.sh verify` green (static all, docs, release + installer/activation smoke); tag CI green.
- Issue #10 was already closed in 0.48.1; all #1-#18 are now closed.

## Open item — Wayle + matugen theming (deferred 2026-11-23)

- On this host (DMS/quickshell) Theming works end to end: `theme_get` returns the matugen palette, `theme_apply` reports every target written, and the daemon audit has no theming errors.
- The user reports a palette error on **Wayle**, to be investigated later (owner: skea999). Start from the exact GUI message plus a real `~/.config/wayle/config.toml`: `theming/wayle.rs` expects `[styling]` and `[styling.palette]` keys, while matugen may write a different shape (the fixture uses `theme-provider = "matugen"`).

## Release 0.48.0 — search results in their own window (2026-11-20)

**Status**: tag `v0.48.0` at `16281de`, CI `44732730677` green (pin `c7a56a9`). Host on 0.48.0.

- All three managers open a half-page **search dialog** on Search: title `Search results (n)`, pagination (`search_page` per manager), per-row Install, Close. The installed list stays on the page underneath.
- A busy state shows `Searching…` inside the dialog; empty results show `No matches`; errors surface as a status row (and on the page).
- APK/flatpak keep search results in `search_apk`/`search_flatpak` instead of overwriting the installed lists (`searching` flag deleted); nix keeps `search_results` with `search_open`.
- Switching package tab or installation closes the dialogs; no wire/protocol change.
- Browser slot order unchanged: confirmations → job progress → list/generations → search.

## Release 0.47.0 — audit of the package-manager surface (2026-11-20)

**Status**: tag `v0.47.0` at `47c8925`, CI `44653055406` green (pin `8091752`). Host on 0.47.0, protocol **7**.

- **Auto-check on for nix** (`UpdatesAuto` default `nix: true`; explicit `[updates] nix = false` still wins). Verified on the host: `config get_config` → `{"apk":true,"flatpak":true,"nix":true}`.
- **`refresh_apk` removed** (protocol v7): no GUI control since the 0.31 decision (`list_apk_updates` refreshes indexes first) and the only package action outside the job path. Verified: the action is an unknown variant now; `list_nix_updates --as-user gab` still returns 14 updates.
- **Audit result**: every remaining action of `packages_nix` (18), `packages_apk` (13) and `packages_flatpak` (14) is referenced by the GUI, mutations go through the job registry, reads answer synchronously. CLI parity is the generic `ursinectl <module> <action>` surface; destructive classification covers the risky set. Nix `upgrade` stays non-destructive by design (profile history/rollback).
- Search Repository dropdown always offers `nixpkgs` plus the current scope's entries (system-only entries appear in the System scope).

## Release 0.46.0 — fast nix update check (2026-11-20)

**Status**: tag `v0.46.0` at `4adef55`, CI `44647630161` green (pin `cb46c1e`). Host on 0.46.0.

- **What the button did**: `nix profile list --json`, then one `nix flake metadata --json` per unique `originalUrl`, **sequentially**. `nix flake metadata` fetches the flake and evaluates it, so channel tarballs (and github repos on a cold cache) dominate the time; `flake:nixpkgs` can re-download a ~40 MB tree once its tarball TTL expires.
- **Now**: unique refs are fetched **in parallel (bounded 4)**; `flake:` aliases resolve through `nix registry list` (user > system > global); a target parsing as `github:owner/repo[/ref]` whose elements all pin a rev uses `git ls-remote` (optional `git`, ~0.45 s, no fetch/eval); everything else keeps `nix flake metadata`.
- **Comparison unchanged**: revision match first, narHash fallback, update reported only when a comparable lock proves it. Count and per-package Upgrade buttons still come from the single response.
- **Also**: `list_nix_updates` joined the user-scoped allowlist, so `ursinectl … --as-user` works like the GUI.
- **Measured on the host** (`ursinectl nix list_nix_updates --as-user gab`): 1.39 s cold-ish, 0.95 s warm, 14 updates; two github refs go through ls-remote, `flake:nixpkgs` (channel alias here) through metadata.

## Release 0.45.0 — password focus on open (2026-11-20)

**Status**: tag `v0.45.0` at `a88c058`, CI `44641893826` green (pin `c261d6b`). Host on 0.45.0.

- `init` returned `text_input::focus` before the first layout, so the op was dropped and the cursor stayed out of the password box.
- `login_focus_pending` latch: the first window `Opened`/`Resized`/`Focused` retries the focus once (subscription only active while pending); sign-out arms it again.
- No wire/config change. Release 0.44.0's dialog fix still needs one **Apply** in Theming to rewrite the stored cosmic theme.

## Release 0.44.0 — dialog colours, sidebar icons, hidden modules (2026-11-20)

**Status**: tag `v0.44.0` at `ca692e1`, CI `44635552607` green (pin `93a456f`). Host on 0.44.0.

- **Green dialogs root cause**: COSMIC paints every dialog with `primary.component.base`; the theming target mapped `primary` to the shell accent. `primary`/`secondary` are neutral containers again (`mix(bg, fg, 0.10/0.14)`), the accent stays in `accent*`, and a unit test pins both. The stored cosmic theme keeps the old green bases until the user presses **Apply** in Theming.
- Browser dialog no longer repeats the title (page heading only).
- Sidebar icons: Disks `drive-harddisk-system-symbolic`, Niri `display-symbolic`, Theming `preferences-appearance-symbolic`, Logs `text-x-generic-symbolic` (all present in the Cosmic theme).
- Sidebar hides entries whose module is `Off` in Settings or whose capability is missing (`home::module_mode` + `screen_available`); Home keeps the fix hints.

## Release 0.43.0 — launcher entry (2026-11-19)

**Status**: tag `v0.43.0` at `98b694c`, CI `44586459732` green (pin `0a4e429`). Host on 0.43.0.

- `packaging/org.feralos.Ursine.desktop` + `packaging/icons/org.feralos.Ursine.svg` (bear mark, dark tile + green).
- `install.sh --datadir` (default `PREFIX/share`) installs them under `applications/` and `icons/hicolor/scalable/apps/`, for release tarballs and any package-manager staging.
- Flake GUI package (`guiCore`) carries the same files, so `nix profile install` surfaces the launcher entry through `XDG_DATA_DIRS`.
- Installer smoke asserts both files for systemd/OpenRC/dinit staging; the CI tarball contract lists them; nix lint/eval green.
- **Host verified**: `~/.nix-profile/share/applications/org.feralos.Ursine.desktop` → `ursine-0.43.0`, icon present, path already in `XDG_DATA_DIRS`.

## Release 0.42.0 — responsive layout (2026-11-19)

**Status**: tag `v0.42.0` at `8255b1b`, CI `44566281029` green (publish + pin `2aeb573`). Host on 0.42.0.

- Width classes via `Application::on_window_resize` → `window_width`: compact `< 640`, wide `>= 1080`; pure helpers `compact_width`/`wide_width` unit-tested.
- Compact: sidebar forced icon-only, package/repository rows stack actions under the text, content padding 8, dialogs `min(720, width - 32)`.
- Wide: content capped at 1200 px; medium keeps the manual sidebar toggle.
- Threaded `compact` through `store::vm`/`packages::vm` and `package_ui` row builders (`RowVm.compact`); no wire/config change.
- Still open: the green package-list background — waiting on the user's screenshot and whether cosmic-settings shows the same.

## Release 0.41.0 — collapsible icon sidebar (2026-11-19)

**Status**: tag `v0.41.0` at `75d1aa3`, CI `44539840596` green (`verify` published `draft=false`, `nix` pinned `a6bb188`). Host on 0.41.0.

- Sidebar (software-center rail): every screen has a symbolic icon; a header toggle collapses the rail to a 64 px icon strip (right-side tooltips name each row), the expanded header carries the FeralOS subtitle, and the footer shows the signed-in user and the version. Sign-out has the log-out icon in both modes.
- `nav_collapsed` is in-app state only (no config/wire change).
- Layout: `button::custom(row![icon, label])` + `.selected(...)`; icon-only rows wrapped in `widget::tooltip`.

## Releases 0.40.1 / 0.40.2 — nix scope UI (2026-11-19)

**Status**: `v0.40.1` (tag CI `44497425808` green, pin `6cc4359`) and `v0.40.2` (tag CI `44502816747` green, pin `d3d8c52`). `scripts/test-host.sh` upgraded the host twice; it now runs 0.40.2 / protocol 6.

- **0.40.1 (`c1b030f`)**: `System` scope hides the target user — the system registry has no target, while the installed list stays on the signed-in account (a non-root user cannot read root's profile). The user scope offers a dropdown of normal login accounts (the plain input stays until the list loads). The installed list names its profile owner (`Installed (n) — <user>`); the screen opens on the user scope.
- **0.40.2 (`ee2e71e`)**: the dropdown keeps desktop accounts only (uid 1000..60000, home under `/home`, real shell) — the daemon's `ListUsers` includes `greeter` (uid 1002, `/var/lib/greetd`). Rule extracted to `desktop_accounts()` with a unit test.
- **Host data verified**: `users list_users` returns `gab` (1000, fish, /home/gab) and `greeter` (1002, /var/lib/greetd); the dropdown now shows only `gab`.

## Release 0.40.0 — nix repository scopes (FeralOS model), protocol v6 (2026-11-18)

**Status**: tag `v0.40.0` at `99b3a2c`, CI `44363193354` green (`verify` published `draft=false`, `nix` pinned `baff114`). `scripts/test-host.sh` upgraded the host to 0.40.0 (`protocol_version: 6`). Feature commit `d2e7b2b`.

- **Scope dropdown** `System | User` (same control as Flatpak) drives the nix Repositories section; the target-user field still picks the profile.
- **List** shows configured entries only: the scope's flake registry plus its `nix-channel` channels (a channel fills a name the registry does not define). Builtin `global` defaults are never returned — no file to edit, no restore path — and the old `scope=global` removal code is gone.
- **Rows** keep the apk shape (name, URL, Remove); `Remove` routes `source=registry` (user CLI or the system file) vs `source=channel` (`nix-channel --remove`). System adds merge an `indirect` entry into `/etc/nix/registry.json` (`github:owner/repo[/ref]`, backup + atomic write).
- **Wire protocol v6**; `ursinectl`, guide, man page, SPEC-packages-nix, samples and tests updated (parser scopes, channel parsing, add/remove routing).
- **Verified on the host** through the daemon (as `gab`): user scope = `ursine` only; add `probe` → present; remove → gone; `nix registry list | grep user` clean. System scope = root's channel `nixpkgs https://nixos.org/channels/nixos-26.05`.
- **Install finding**: this FeralOS machine has the channel but **no `/etc/nix/registry.json`** (the installer's stable pin is missing), so `nixpkgs#…` still resolves through the builtin unstable mapping. The System scope Add form can write the pin (`nixpkgs` `github:NixOS/nixpkgs/nixos-26.05`).

## Release 0.39.3 — nix repositories in the GUI (2026-11-18)

**Status**: tag `v0.39.3` at `cf6bd97`, tag CI `44320374782` green (`verify` published `draft=false`, `nix` pinned `53eade8` + attached the flake). Host upgraded by `scripts/test-host.sh` to 0.39.3 and the daemon now answers the registry with the user entry.

- **Symptom**: nix → Repositories showed "only the default repository (nothing extra)"; `flake:ursine` invisible; rows absent while apk/flatpak worked.
- **Root cause (audit-log evidence)**: the GUI sends `as_user` with `list_nix_repositories`; dispatch only honors `as_user` for actions in `uses_profile`, and the three registry actions were missing, so every call returned `invalid: as_user is only valid for user-scoped …`. The GUI cleared the section and rendered the empty state.
- **Fix (`637f121`)**: `ListNixRepositories`/`AddNixRepository`/`RemoveNixRepository` are user-scoped (the registry lives in the target's home); a non-root caller may only target its own account for add/remove (same boundary as theming/crontab/Flatpak user scope); empty-state text is now "No flake registry entries.". Two dispatch tests cover own-target acceptance and the foreign-target rejection. Rows keep the apk shape: name, URL, Remove.
- **Verified on the host**: `ursinectl --json nix list_nix_repositories --as-user gab` returns `{"name":"ursine","url":"github:skea999/ursine","scope":"user"}` first, then the global entries.
- **Also validated in this release**: the nix job's anonymous tarball smoke and forced `--draft=false` (introduced after the 0.39.2 draft incident) — 0.39.3 was public with zero manual steps.

## Session 2026-11-18 — binding gates at max severity, artifacts layout, host acceptance (uncommitted)

**Status**: released **0.39.2** (tag `v0.39.2` at `45fcd0f`); tag CI `44298587484` green (`verify` published the tarball+checksum, `nix` smoke+pinned+attached the flake source). Pin `b5a03bc` on `main` (0.39.2). Earlier gate work pushed as `3889791` (`34efb66..3889791`). `scripts/dev.sh check` green (fmt + 515 tests, single clippy pass), `scripts/dev.sh verify` green (static all + docs + release + installer smoke), Nix lint/eval green (`0.39.1`), jscpd **0 clones**. No tag yet (needs a version bump).

- **All gates binding**: `scripts/static-check.sh` fails when a tool is missing (no skips); jscpd 50 tokens threshold 0 on Rust + shell; advisories zero-tolerance via `cargo deny` + `cargo audit` (ignores only in `deny.toml` + documented exceptions). `ci/duplicates-baseline.txt` deleted — generated state is never committed; `cargo tree -d` writes a report into the run dir (90 duplicate crate versions, all from the pinned libcosmic/iced graph).
- **Artifacts layout**: `scripts/run-dir.sh` creates and prunes `artifacts/container-dev/<UTC timestamp>/` (newest 10); static, duplicates, release, package and smoke trees all write there; `URSINE_RUN_DIR` is forwarded so nested gates share one directory; CI sets it per run. No `.gitkeep`, no `container dev` spaces.
- **One image, two jobs**: `ci/Containerfile` → `ursine-ci:1`; `verify` runs every check on push/PR and builds/publishes only on a tag; `nix` runs lint+eval on PR/main and the tarball integrity smoke + pin on a tag. A push never builds artifacts; a tag never re-runs checks.
- **Dedupe 42 → 0 clones with real shared code**: `tooling::blocking`, `logs::tail_chunk`, `config_meta`, `session::live`, `jobs::run_worker`, `exec::{regex_noop,compile_regex,validate_search_query}`, apk/flatpak `start_regex_job` + `list_kind`, `booster_struct!`, `network_block`, `config_dir`, `write_failure`/`push_block`/`push_section`, `modules::prelude`, `Daemon::assemble`, the theme fixture and e2e helpers (`ok`, `json_ok`, `assert_root_gate`, `set_crontab_switch`, `assert_module_disabled`). No threshold change, no detector gaming.
- **Host acceptance replaces the dev path**: `scripts/build-gui-dev.sh` and `nix/dev-flake.nix` deleted — they ran `nix run` on the host and violated the container-only rule. `scripts/test-host.sh` is the single allowed host interaction and runs **after the tag** without touching the repository: ensure the `flake:ursine` registry entry, `nix flake metadata --refresh` + `nix eval --refresh` for the published version, install/upgrade the published GitHub flake (reinstall when the version differs), `sudo ursine-activate`, then check `auth: shadow`, the live service and `ursinectl system status`.
- **Fixes found by the gate**: GUI compile set (`pub(crate) use crate::ui`, `logs` container import, Bluetooth Scan button via `ui::header_with`), `elidable_lifetime_names` in `ui::header`, `redundant_pub_crate` allows on the private tooling/atomic/prelude modules, `Daemon::assemble` wiring for both binaries.

Host acceptance **ran for real**: `scripts/test-host.sh` upgraded the host from 0.39.1 to 0.39.2 through the published flake, OpenRC restarted `ursined`, and the checks returned `ursined 0.39.2 (auth: shadow)` + `{"version":"0.39.2","protocol_version":5,"auth":"shadow"}`. It first failed because the 0.39.2 release was left as a draft (asset 404 for anonymous users); the release is now published and CI fixed: the nix smoke downloads the asset anonymously and the publish step forces `--draft=false`. A tag run that fails in the nix job must be fixed forward: the first attempt died on libgit2 ownership of the bind mount; `safe.directory /work` is now set inside the container.

## Session 2026-11-17 — CI pipeline split + single-pass gate, release 0.39.1 (main, tag `v0.39.1`)

**Status**: Pushed through `52777bd` + tag `v0.39.1`; push CI `44095649999`/`44100041869` success (no `package` job — verified); tag CI `44104078121` success (checks skipped, `package`+`publish` green — verified); auto-pin `6519964` pulled, HEAD `6519964`.

- **nix repo rows (`463981c`)**: exactly the apk shape — identity (`flake:ursine`) title, full URL single detail, Remove right; `· scope` suffix gone from the row (confirm still says "system repository" for global).
- **CI restructure (`d1443ad`, user decision)**: branch push NEVER builds artifacts (only a release tag may); a tag never re-runs checks (the push of the same commit already ran them). `test/lint/gui/analyzer/docs/zizmor` → `if: github.ref_type != 'tag'`; `package` tag-only (+ forced dispatch / PR smoke) with `!cancelled()`; `publish` tag-only with `!cancelled()`. Verified live both ways: push run has no package, tag run skips checks.
- **Single-pass gate (`fe73cf5`, user decision "falli uguali, non perdere nessun test")**: `RUSTC_WORKSPACE_WRAPPER=clippy-driver CLIPPY_ARGS="-Dwarnings" cargo test` = one compilation does clippy lints + ALL tests, everywhere (CI test matrix + gui job, `dev.sh check/fast`). Separate clippy build deleted from CI lint job (fmt + rustdoc remain). **Bug found by probe**: clippy-driver takes `CLIPPY_ARGS` as ONE argv — `-D warnings` reaches rustc as unknown lint ` warnings` and denies nothing; must be `-Dwarnings`. Wrapper proved stricter than plain clippy (caught `derivable_impls` on ApparmorState/CrontabState → `#[derive(Default)]`). Full workspace single pass green (13 suites, rc=0).
- **`dev.sh fast`**: iteration gate — fmt + clippy/test scoped to changed crates + pure lints, no rust recompilation; time policy in `build-and-test.md` §2. `run_ci` now exports `RUST_MIN_STACK=16777216` (inprocess SIGABRT caught by fast on workspace scope, `37c0df1`).
- Release `52777bd` (0.39.1: nix rows CHANGELOG entry); gates green each commit (fmt, single-pass tests, docs, typos, static all).

## Session 2026-11-16 (d) — perf + modals everywhere, release 0.39.0 (main, tag `v0.39.0`)

**Status**: Pushed through `126d630` (keyring hung again mid-session — user fixed auth, GIT_TRACE pinned `git-credential-libsecret get`); feature CI `44060327227` + tag CI `44063478538`/`44063478566` success; auto-pin `ae99888` pulled, HEAD `ae99888`.

- **Perf (`ec5499a`)**: apk `parse_installed` O(n²)→linear (HashSet + `split_id`, no per-candidate `format!` — the UI freeze); zero-allocation filtering (`filter_keys` raw titles + `contains_ci`, NO lowercase anywhere — user asked to remove it; keys rebuilt on every source swap incl. the load_store blocker); pagination `PAGE_SIZE=50` + `page_bounds` for installed lists ×3 and nix generations (page-only string building); browser open button always visible (0.38.0 bug); nix rows show `name version` from the store path; `cargo clean` reclaimed133 GB (ENOSPC on link).
- **Modals (`54a9b9b` + `ffc5312`)**: confirmations AND job output out of the page into `Application::dialog` — priority: session gate → package confirms → other-module confirms → job progress (same slot as the confirm: confirm → output in one window; Close only when done) → half-page browsers. `ConfirmVm.rows` became generic (label → widget) so the disks format/create/delete flows carry dropdowns/inputs/type-to-confirm INTO the dialog; inline blocks deleted in ALL12 remaining modules (apparmor, booster, crontab, disks, firewall, network forget+IPv4, printers, scanners, services, snapshots, theming, users delete); `confirm_row`/`pending_text` gone; zero `title("Confirm…")` in views.
- Review closed (stale comments, CHANGELOG wording); gates green each step; rustdoc lesson (`cargo doc -D warnings` locally) applied pre-push.
- User's earlier "actions blocked until Close" attributed to the old inline/modal-less flow (resolved by the modal work — user confirmed).

## Session 2026-11-16 (c) — half-page browsers, release 0.38.0 (main, tag `v0.38.0`)

**Status**: Pushed through `f29d9d9`; feature CI `43932204543` + session-docs CI `43912341137` success; tag CI `43936042826`/`43936043200` success; auto-pin `e73433f` pulled, HEAD `e73433f`.

- **Half-page browsers (`93e3112`)**: nix Generations + the Installed list of ALL THREE managers open in a centered modal (`Application::dialog` override, cosmic `widget::dialog`: scrollable control, 900×≤600px, Close) from compact page rows ("List / Open list…", "History / Browse generations…"). Flags: `PackagesState.list_browser`, `StoreState.{list_browser,generations_browser}` — reset on kind switch, target-user change, every tab switch. Acting from inside a browser closes it first (row install/remove/upgrade ×3, every store action, Rollback/Diff which live inside the generations dialog).
- Review closed: blocker — Rollback/Diff didn't close the generations dialog; belt-and-braces reset in `open_package_tab`. VM-smoke note: scrollable `Fill` inside the dialog control must be clamped by `max_height(600)` for scrolling to engage (visual check pending).
- Gate green each step; rustdoc `cargo doc -D warnings` lesson applied pre-push.

## Session 2026-11-16 (b) — unlimited lists + search, Generations order, release 0.37.0 (main, tag `v0.37.0`)

**Status**: Pushed through `b5426f3`; tag CI `43906543519`/`43906543937` success, feature CI `43903607405` success (one rustdoc failure on `d796101` — intra-doc link to pub(crate) const — fixed in `167105f`, `cargo doc -D warnings` now run locally before push). Auto-pin `6d199d0` pulled, HEAD `6d199d0`.

- **Unlimited installed lists (`7409a4a`)**: 300-row caps removed (apk installed+updates, flatpak list+merges+updates), list reads use complete-output runners (new `run_root_program_complete`, flatpak user via `run_as_user_argv_bounded`, nix profile list via `run_as_user_json`) instead of silent 64 KiB truncation; `MAX_ROWS` documented search-only; Generations moved ABOVE Installed in `SECTIONS` + alignment test; 301-row fixture tests for apk/flatpak parsers.
- **Unlimited search results (`d796101`)**: apk inline search loop + flatpak `parse_search` caps removed, `MAX_ROWS` const deleted entirely; nix keeps documented 200 + "first N of more" (slow eval by design). flatpak 301-row search test.
- User confirmed the installed Filter row now searches ALL packages (it filters the unlimited rows).
- Gates green each commit; docs/typos green; one one-off SIGABRT in the suite under load (not reproduced, 2 greens after).

## Session 2026-11-16 — GUI fusion cleanup, alignment, filter + inline preview, release 0.36.0 (main, tag `v0.36.0`)

**Status**: Pushed through `9d07385`; auto-pin `f1d69d8` pulled, local HEAD `f1d69d8`. Tag `v0.36.0` = `e8a1653`, tag CI `43859083527` success, HEAD CI `43859782552` success (intermediate main runs cancelled by concurrency = normal). Transient push/keyring hang recurred once; retry with GIT_TRACE succeeded.

- **0.35.0 (Nov13)**: fusion follow-ups — `package_row.rs` merged into `package_ui.rs`; flatpak orphan count (`uninstall --unused` answered `n`, read-only plan parse, `orphans` field, label `Remove orphans… (N)`, disabled at 0); flatpak Preview button (get_flatpak_info → copyable top message); section naming unified (Repositories/Add repository/repository wording); nix header subtitle removed (header/status/busy shape identical).
- **Alignment pass (`4fce2a0`)**: list-row buttons guarded everywhere, Direct-install/Search arming + Enter-submit + `Searching…` label (new `search_busy` on PackagesState with full lifecycle), Upgrade-all/Add-form guards, `No matches` + `Result / No packages installed.` notes unified, repository rows = identity title + URL detail (apk daemon `ApkRepository.label`, nix name + `url · scope`), **apk reads `/etc/apk/repositories.d/`** (list + Remove rewrites owning fragment; symlinks skipped), placeholders `e.g. …`.
- **Filter + preview (`cbb9f1d`)**: Filter row in the installed list of all three (client-side title match before count/notes, `No matches` when emptied); pattern preview result INLINE under the buttons (`PatternSection.result`, nix regex dry-runs write it instead of Last-action); blocked pattern clicks now raise copyable errors (armed = non-empty pattern only); superseded preview reply clears `action_busy` before the epoch guard; regex unit tests (pure `matching_names`/`matching_app_ids` extracted, `regex_preview_summary`, extra validator cases — validator has NO backslash by design, tests use `[.]`).
- **0.36.0 release**: CHANGELOG initially left notes under `[0.35.0]` (no Unreleased heading existed) + blank-line lint miss — fixed in `a1972d6`/`9d07385` (both pushed; docs+typos green).
- Review closure on every batch (fusion7 nits, permissions7, alignment4, filter2). Gates green each commit.

## Session 2026-11-13 — 3-line installed rows + nix registry Remove everywhere, release 0.34.0 (main, tag `v0.34.0`)

**Status**: Pushed `1b99076..032fe41` (`33b839c` session + `5ca43d5` + `79141ef` + `88c557e` + `032fe41` chore(release)); CI green for `88c557e` (`43435927270`) and both release runs (`43438763702`/`43438764210` success); auto-pin `74b383d` pulled, local HEAD `74b383d`.

- **nix Upgrade section**: upgradable-names list removed (user decision) — count/toggler/Refresh stay; per-package Upgrade buttons still keyed on `NixUpdates.packages` (kept in API for that + ursinectl). Stale dry-run comments fixed (store.rs, Action doc).
- **3-line installed rows in all three managers** (user decision, hand-checked layout): title / description / repository line. apk: `@tag v3.24 main · <url>` (`ApkPackage.origin_label` — last two path segments with query stripped, tag from the existing `/etc/apk/repositories` parser); flatpak: new `description` column (verified on `remote-ls --updates` too — shared LIST_COLUMNS), line 3 = `app_id · branch · origin · <remote url>`, search description moved `origin` → `description` field; nix: `meta.description` best-effort (ONE `nix eval --impure --json --expr` per unique flake ref, names/attr segments validated before embedding, tryEval), line 3 = `attr original_url · https link`. List + evals share ONE LIST_TIMEOUT deadline (client profile timeout can never be blown). API fields serde-default for mixed versions.
- **nix registry Remove on EVERY row** (user decision incl. default/global): `RemoveNixRepository` + optional `scope` (`user`/`default` → `nix registry remove`; `global` → direct `/etc/nix/registry.json` edit — read (metadata cap first), `drop_global_flake` (indirect same-id only, wrong-typed `flakes` = error, idempotent), uniform backup rotation, atomic write). GUI passes row scope; confirm says "system registry entry" for global; guide + man updated (parity). **atomic writer now preserves the original file mode** (stat/chmod around the rename — also fixes `/etc/apk/repositories` staying 0600 after edits); behavioral test runs the real script.
- **Review closed** on both batches (5 nits →4 fixed: flakes diagnosis, metadata-before-read, executed mode test, CLI `scope=global` sample; blocking-read nit accepted per reviewer). Gates green each cycle; ec padding needed two fixes (string continuation, test JSON indent).
- **Push incident (Nov13)**: keyring session hung — `git-credential-libsecret get` never returns and gh (token in keyring, not hosts.yml) hangs too; GIT_TRACE pinpointed it; user fixed auth; push then `1b99076..88c557e` OK. Symptom→first suspect: credential helpers when `git push` times out silently.
- Release `032fe41` + tag `v0.34.0`; pin `74b383d`.

## Session 2026-11-12 (c) — nix update check fixed, unified details + shared GUI builders, release 0.33.0 (main, tag `v0.33.0`)

**Status**: Pushed `b02992d..b2d497b` (`7c3d71c` feat + `b2d497b` chore(release)); CI green for `7c3d71c` (`43273451156`) and both release runs (`43276561476`/`43276561501` success); auto-pin `1b99076` pulled, local HEAD `1b99076`.

- **nix "Check for updates" rewritten** (user bug report, hand-tested on this VM): nix 2.31.5 has NO `profile upgrade --dry-run` (help verified) → new path = `nix profile list --json` + one `nix flake metadata <ref> --json` per unique `originalUrl`; element outdated iff its pinned narHash (percent-decoded) / rev differs from the metadata's current lock of the SAME kind; missing data = never guess. Ref rides a single-quoted VALUE after `--` (verified `nix flake metadata --json -- flake:nixpkgs` works; `&`/`?` survive, quotes/newlines skipped). Old classifier/parser deleted. Python prototype on the real profile:14/15 outdated (ursine at current pin correctly excluded).
- **Errors copyable** everywhere: every `text(error)` in the GUI → `selectable_text` (login + all modules) — user can copy an error and paste it back.
- **Unified details (flatpak = reference)**: info line first, repository https link under it. apk: description + FULL `apk policy` URL (tail removed backend+GUI); nix: `attr · flake:nixpkgs` + https from the registry (lookup normalizes the `flake:` prefix the registry parser strips — review finding) or element url via `flake_to_https` (tested); flatpak unchanged.
- **nix registry rows** = apk style: title = flake URL, detail = `name · scope`, Remove/built-in unchanged.
- **Pattern schema unified** (user: same as nix): shared `pattern_section` (hint + input + Preview/Upgrade/Remove one row) used by ALL THREE; preview result → shared `status_items` "Last action" line with nix's exact wording; destructive apply → shared `confirm_section` at top (red for apk/flatpak, nix keeps suggested — its pending block covers installs); `pattern_preview` state deleted; packages inline confirm/list removed.
- **Shared GUI code** (user's core demand): `package_ui.rs` now owns status_items / pattern_section / confirm_section / repository_row / update_control_items — the three managers feed data, layout lives in one place.
- Review closed (5 findings): stale SPEC-daemon-core dry-run line, dead registry lookup (`flake:` prefix), `&`-query false negative (quoted value after `--`), stale apk "tail" doc, confirm styling per manager. Gate green each cycle; static needed2 fixes (typos `unparseable`, ec padding) + one rerun after a log-interleave incident (orphaned podman from a timed-out run — kill containers before rerunning).
- Release `b2d497b` + tag `v0.33.0`; pin `1b99076`.

## Session 2026-11-12 (b) — package-UI parity + regex + login focus, release 0.32.0 (main, tag `v0.32.0`)

**Status**: Pushed `075f43f..0315a59`; CI green for `ae065d2` (`43230992841`) and both release runs (`43233852074`/`43233852955` success); auto-pin `b02992d` pulled, local HEAD `b02992d`.

- **UI parity** (decision PACKAGE-MANAGER UI PARITY): identical section order Scope → Maintenance → Upgrade → Search → Remotes/Repositories → Pattern → Installed; counts on Installed titles; apk description = real (apk db `T:`) under `from <repo>` (one `apk policy` call); flatpak detail + remote URL; nix detail + `repository: <original_url>`; shared `package_ui.rs` (repository_row — flatpak remotes now item-per-remote like apk/nix — + update_control_items).
- **Regex Pattern actions for apk+flatpak** (decision PATTERN-REGEX):4 new actions, daemon-side match (CLIs have none), dry_run preview + ONE job over the matched set behind confirm; destructive = dry_run:false; samples apk=Ok/flatpak=Gate; help/guide/man.
- **Login password focus** at entry, after sign-out, after failed attempt.
- **libcosmic bump reverted** (decision recorded): no menu-position fix in ecc5fe4 range + zbus6/theme-rename risk; right-click menu position stays OPEN (user: leave it for now).
- Review closed (man parity blocker, confirm guards-before-take, doc wording, session-state). Gates green each step. Release `0315a59` + tag `v0.32.0`.

## Session 2026-11-12 — VM-smoke fixes + hand-verified commands, release 0.31.0 (main, tag `v0.31.0`)

**Status**: Pushed `0461357..417fb39` (push hit a transient DNS outage mid-way, recovered on retry); CI green for `52f6b82` (`43197471698`) and both release runs (`43199990883`/`43199991246` success); auto-pin `075f43f` pulled, local HEAD `075f43f`.

- **Hand-tested on the reference VM (user demanded real tests over hypotheses)**: `nix --version` = 2.31.5; `nix registry list` real output captured verbatim as a test fixture (scope column `user`/`global`, ~50 global rows, slash names, query-string URLs); `nix profile upgrade --help` has NO dry-run; `nix profile upgrade --all --dry-run` → `error: unrecognised flag '--dry-run'` (BRITISH spelling — classifier now matches both spellings); `apk version -l` alone errors — correct form `apk version -l '<'`.
- **Real bugs found by tests**: (1) `apk version -l '<'` was rejected by the strict argv guard (`<` outside atom charset) → the apk check ALWAYS failed with "invalid command argument" (the true cause of "non funziona") — fixed via quoted-value guard; the check also runs `apk update` first (indexes), maintenance "Refresh indexes" row removed; (2) nix dry-run Unavailable mapping; (3) registry scopeless rows were dropped (now `default`, shown+removable; global shows override hint; hint waits for load); (4) no-description text gone; (5) package-list text selectable (nix/apk/flatpak). New e2e `apk_update_check_refreshes_indexes_and_lists` runs the real chain (with apk-db lock retry).
- Gates green; release `417fb39` + tag `v0.31.0`. User retesting the VM next — findings to follow.

## Session 2026-11-11/12 — update-check UX + flatpak runtimes, release 0.30.0 (main, tag `v0.30.0`)

**Status**: Pushed `e385eb0..cfd8902` (3 feature commits + release); CI green for `9030034` (`43116011460`) and both release runs (`43118141564`/`43118141595` success); auto-pin `0461357` pulled, local HEAD `0461357`.

- **nix generations newest-first** (`8a4c722`): `Action::Generations` reorders groups (header+details kept together) before returning — CLI + GUI both benefit.
- **Update-check UX** (`9030034`, session-state UPDATE-CHECK UX): every manager's Upgrade section = persisted toggler (`[updates]` in config.toml via new `config.set_update_auto`; apk/flatpak default on, nix off) + prominent count ("—" until first check, never fake zero) + manual "Check for updates" Refresh. New `nix list_nix_updates` = `upgrade --all --dry-run` (300 s, parses `upgrading '<name>'` → {count, packages}). apk/flatpak fetch updates on open ONLY when the toggle is on. Flatpak per-package update LIST removed (count only; user correction: apk's list also existed but hid behind `!is_empty` + needing `apk update` — removed too). Installed lists: per-package **Upgrade button only when the check says outdated** (nix via dry-run names, apk/flatpak via update lists). Race guards: updates_seq generation counter + StoreScope on nix replies.
- **Flatpak runtimes** (same commit): "Show Apps/Runtimes" row gone — apps AND runtimes in one list, runtimes labelled **protected** (apk-style, no buttons). Nix: no app/runtime split → unchanged (explained to user).
- Review closed:2 stale-scope/kind blockers (seq+scope guards), double-fetch nit, seeding consistent with set_module_switch, ec-indent in test string, empty tests mod, guide indent. Gates green each amend; static-check all PASS (final).
- Release `cfd8902` + tag `v0.30.0`; VM smoke still pending (0.27–0.30 backlog: backup slots, no-D-Bus flows, booster/apparmor/easyprof, update-check UX + runtimes/conditional buttons).

## Session 2026-11-11 — easyprof + GUI fixes, release 0.29.0 (main, tag `v0.29.0`)

**Status**: Pushed `03a3e4a..749db23` (credential hiccup mid-session — global libsecret helper hung the push until the user fixed auth); CI green for `7db9d85` (`43045873637`) and both release runs (`43048578226`/`43048578318` success); auto-pin `e385eb0` pulled, local HEAD `e385eb0`.

- **aa-easyprof included** (`7d1e17b`, supersedes the deferral): man + upstream `easyprof.py` verified — default output is stdout (`--output-directory` opt-in, file-write branch gated on it); `apparmor_easyprof {binary, template?, policy_groups?}` uses TWO argv elements per short option (optparse: `-t=VALUE` keeps the `=`) + `apparmor_easyprof_list {what}`; binary rides the quoted-value guard (`*` legal); stdout-only → no profile file written (transient /tmp for apparmor_parser only).
- **GUI**: scrollable Home + Settings + Login (last screen), Settings module mode = segmented buttons `Auto|On|Off` instead of the dropdown; nix **Pattern actions** input invisible (single row: buttons squeezed the text input to zero width) → three stacked rows (title / full-width input / buttons); Generations history rows now labelled BY THE GENERATION NUMBER (header `Version N…` loses its prefix, change lines append to their generation, no more per-row "Profile history").
- Gates per commit: fmt/clippy/tests, static-check all, docs, typos — green.
- Release `749db23` + tag `v0.29.0`; VM smoke still pending (booster, apparmor incl. notifications + easyprof, plus the GUI fixes eyeball pass).

## Session 2026-11-10 — booster + apparmor modules, release 0.28.0 (main, tag `v0.28.0`)

**Status**: Pushed `4ad349a..1b8a2aa` (+ user commit `1882d5c` already remote); CI green for `65df199` (via later runs), `49ed08b` (`42864154585`), release `1b8a2aa` (both runs `42867007902`/`42867008016` success); auto-pin `03a3e4a` pulled, local HEAD `03a3e4a`.

- **booster module** (`65df199`, SPEC-booster): surgical `/etc/booster.yaml` editor (comments + installer marker byte-identical, backup → YAML validation → atomic + SET_LOCK), regenerate chain `apk fix kernel-hooks` → `regenerate_images` → per-kernel `booster build` (`/boot/booster-<flavor>`, never guessed), `select_step()` single source of truth for confirm preview; capability `Booster`; GUI + `ursinectl booster …` + guide/man. Review:16 findings + HIGH client timeout (5s → daemon_budget(120)).
- **apparmor module** (`49ed08b`, SPEC-apparmor): full headless CLI surface — tolerant aa-status runner (exit1-3=data), modes (enforce/complain/disable + enable = remove every disable/ marker + reload), profile SOURCE editor (syntax check on stdin BEFORE write, backup, SET_LOCK), notifications button (notify.conf + aa-notify autostart, next login), cleanprof/autodep with whole-dir backup, unconfined/remove_unknown/features_abi/decode; capability `Apparmor` (same fixed dirs as module); GUI + `ursinectl apparmor …` (15 actions) + guide/man. Review:10 findings (2 HIGH name/basename normalization) + ec indent. Deferred with user visibility: aa-easyprof (it is a policy GENERATOR, not a report — pending user verification).
- Naming decisions: module `booster` not `initramfs`; notifications activate from NEXT LOGIN (no same-session spawn).
- Gates: fmt/clippy/tests ×N, static-check all PASS (final trees), docs 0, typos 0 — for both features.
- VM smoke still pending for BOTH: booster (edit key → Save → Regenerate hook output, UKI rebuilt), apparmor (status round-trip, check-before-load, disable/enable cycle, Save refuses bad syntax, notifications after login).

## Session 2026-11-09 — CLI minimization + release 0.27.0 (main, tag `v0.27.0`)

**Status**: Pushed `ff92296..05aeb6b` + tag `v0.27.0`; both tag CI runs green (`42687910852`, `42687910997`); auto-pin `4ad349a` pulled. Local HEAD `4ad349a`.

- **Slice CLI-min** (user decision, inventory-first): flatpak overrides via `flatpak override --show` → diff → single invocation (`--unset-env` etc. verified against `override --help`); keyfile render deleted. Passwords via `chpasswd -e` (stdin WITHOUT trailing newline — the quoted-stdin runner appends one; verified in ursine-ci: single `\n` → exit 0, double `\n` → "missing new password"); direct shadow writer + SHADOW_LOCK deleted. Disks via `mount`/`umount` + sysfs/`/proc/mounts`/blkid/df listing; lsblk/udisksctl structs deleted; capability `Udisks2`→`Parted` (`apk add parted`). PartitionCreate/Delete arms + flatpak `validate_*` restored byte-identical from git.
- **Review fixes**: HIGH chpasswd double-newline stdin; MEDIUM `fstab_names` resolves `UUID=`/`LABEL=` specs via `resolve_spec` (blkid reverse map built in `mount_device`); LOW `/media/ursine/<device>` explicit 0755; `resolve_spec` canonicalizes `/dev/…` symlinks; `is_partition_of` replaces bare prefix match (sda vs sdaa); `mount_lookup` canonical fallback for dm paths.
- **Test flake ROOT-CAUSED + fixed**: `dispatch_per_module` macro called `call()` BEFORE matching `Dispatch::Skip` — Skip only skipped the assertion, so `create_user name=t` really ran and rewrote `/etc/passwd` in parallel with `root_manages…` account reads → intermittent `NotFound: user ursine-fix does not exist`. Fix: Skip now `continue`s before the call. ~13 runs since: 0 failures. Diagnostics (fixture_state snapshot) left at ThemeDetect/crontab sites.
- **Release 0.27.0** (procedure = `c81137c`): CHANGELOG `## [Unreleased]`→`## [0.27.0] — 2026-11-09`, Cargo.toml 3 version spots + Cargo.lock (diff byte-size matches 0.26.0 release), `chore(release): 0.27.0` `05aeb6b`, tag, push, CI green, workflow auto-pinned `chore(nix): pin release v0.27.0 [skip ci]`.
- Gate before tag: fmt + clippy `-D warnings` + full tests (`RUST_MIN_STACK`) ×multiple, static-check all PASS, docs-check 0 issues, typos 0. VM smoke still pending: backup slot rotation (`copies` clamp, `/var/backups/ursine`), no-D-Bus flows (spaced fullname, shadow `!`→hash, Just-Works pairing, Home `apk add parted` fix row), CLI-min (flatpak override round-trip with foreign keys, `chpasswd -e` password set, `mount`/`umount` + `/media/ursine` perms).

## Session 2026-11-09 — No D-Bus (Webmin study + full removal) (main, commit `31a9087`)

**Status**: Pushed `1dde913..31a9087`, CI `42616422224` watched (pending green at handoff). User decision after a Webmin study (source: `useradmin/user-lib.pl` — direct passwd/shadow/group edits, no bus, no polkit): **D-Bus is never used**; fewest components possible; capabilities name the exact CLI + install fix; Home shows that fix; delete dead code/files.

- New invariant 3 in `rules/ursine.md` + AGENTS.md "Minimal components" workflow principle + overview Auth model. Protocol **v5** (breaking removals).
- Users rewritten: `/etc/passwd` parse, `useradd -U`/BusyBox `adduser -D` + explicit `!` lock step, `userdel`/`deluser -r`, direct `/etc/shadow` write (unique tmp + rename + parent fsync, SHADOW_LOCK across the concurrent accept loop, quoted-value runner for spaced fullnames, spawn_blocking). Capability `accountsservice`→`shadow` (exact candidate-path probe).
- Bluetooth: `agent.rs` deleted, `set_bluetooth_pairing_pin` + `pairing_pin_set` + GUI PIN section removed; pairing = `bluetoothctl --agent NoInputNoOutput pair`.
- Theming: `ThemingTarget::Eog` removed (21→20), gsettings/dconf writers + dconf rotation deleted; settings.ini is the source of truth; `user` param dropped from run_blocking/write_target/gtk_settings.
- Capabilities CLI-first: Disks also requires `parted` (`apk add util-linux udisks2 parted`); Bluez/NM binary-only probes (socket probes dropped — trade-off recorded); Home prints `reason - fix` on activated features.
- zbus gone from workspace + ursine-core Cargo (libcosmic transitive dep documented). Docs: SPEC-users rewritten, 8 other SPECs + ROADMAP/TASKS/intent/PROJECT/capability-map/overview + CHANGELOG.
- Review: 1 potential blocker rejected (is_safe_atom accepts `/`; spaced fullnames → `run_root_program_value`) + 2 high (shadow tmp atomicity, SHADOW_LOCK) + mediums/lows all fixed; trade-offs in session-state.
- Gate: fmt/clippy/tests (RUST_MIN_STACK)/static/docs/typos all PASS.
- VM smoke pending: create user with spaced fullname + admin wheel; set password (check shadow `!`→hash, mode/owner preserved); delete with home cleanup; pair a Just Works bluetooth device; theming apply writes settings.ini only (no dconf); Home shows `apk add parted`-style fixes on activated-but-missing features.

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

- **This was the last required v2 code item.** Remaining v1.0 closure work is user/VM-side: APKBUILDs in `feralos-aports`, real FeralOS activation/module smoke, then the v1.0 tag.
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
- ~~Open: VM smoke of the permissions write path~~ — void: the permissions feature was removed entirely (user decision 2026-11-13); pre-existing epoch/scope race in the package panel recorded as risk (not introduced here).
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
