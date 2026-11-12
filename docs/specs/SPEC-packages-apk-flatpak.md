# SPEC-packages-apk-flatpak

## Objective

Expose Alpine (`apk`) and Flatpak packages in the same thin-client control
center as the Nix store. Both backends are capability-gated and audited.

## API surface

| Action | Backend |
|---|---|
| `list_apk` | `apk info -v` + `apk info` (name/version pairing) |
| `search_apk` | `apk search -v <query>` |
| `refresh_apk` | `apk update` |
| `list_apk_repositories` | parse `/etc/apk/repositories` (`[@tag] url`) |
| `add_apk_repository` | upsert the line + `apk update` (atomic write + backup) |
| `remove_apk_repository` | drop the line + `apk update` (atomic write + backup) |
| `install_apk` | `apk add <package>` (progress via job log) |
| `remove_apk` | `apk del <package>` |
| `upgrade_apk` | `apk upgrade` (progress via job log) |
| `list_apk_updates` | `apk update` FIRST (the check refreshes the indexes — replaces the old maintenance "Refresh indexes" button) then `apk version -l <` → `ApkUpdate[]` |
| `apk_remove_regex` | match installed names against the regex daemon-side (the apk CLI has none) → dry-run returns `{dry_run, would_change}`, apply runs one `apk del <names…>` job (destructive in the CLI) |
| `apk_upgrade_regex` | same match → one `apk upgrade <names…>` job |
| `list_flatpak` | `flatpak list --columns=... --system/--user` |
| `list_flatpak_remotes` | `flatpak remotes --columns=name,url [--system/--user]` |
| `add_flatpak_remote` | `flatpak remote-add <scope> <name> <url>` |
| `remove_flatpak_remote` | `flatpak remote-delete <scope> <name>` |

- Both remote mutations rotate the installation's `repo/config` first
  (uniform slots); `apk add`/`del` rotate `/etc/apk/world` first.
| `list_flatpak_updates` | `flatpak remote-ls <scope> --app/--runtime --updates --columns=…` |
| `get_flatpak_permissions` | `flatpak override --show` + `flatpak info --show-permissions` |
| `set_flatpak_permissions` | `flatpak override --show` → diff → one `flatpak override` invocation with only the changed flags (foreign keys/groups untouched); the override file rotates first |
| `search_flatpak` | `flatpak search --columns=... --system/--user <query>` |
| `install_flatpak` | `flatpak install -y --noninteractive <scope> <remote> <app>` (remote default `flathub`, validated) |
| `get_flatpak_info` | `flatpak remote-info <scope> <remote> <app>` (name, version, sizes) |
| `remove_flatpak` | `flatpak uninstall -y --noninteractive <scope> <app>` |
| `flatpak_remove_regex` | match app id/name daemon-side → dry-run `{dry_run, would_change}` or one `flatpak uninstall … <refs…>` job |
| `flatpak_upgrade_regex` | same match → one `flatpak update … <refs…>` job |
| `update_flatpak` | `flatpak update -y --noninteractive <scope>` |

- APK is system scope: root only.
- Flatpak has system and user scope; user scope runs as the authenticated
  principal. Root may retarget user scope to any existing normal-login
  account through the wire `as_user` (`--as-user`); non-root principals
  may target only their own account (same hardened boundary as
  theming/crontab), and system-scope actions reject `as_user`.
- The GUI never shells out. It renders typed rows from `ursine-api`.

## Security

- Absolute trusted tool paths, argv only, no shell.
- Package IDs and search queries are validated before execution.
- Output is bounded by the shared exec helper.
- Destructive actions require GUI confirmation.
- Capability gates: `apk` binary; `flatpak` binary.
- Audit uses the normal module/action path; no secrets are logged.
- Override writes: ids pass the strict charset, paths are built from
  fixed prefixes, content is validated (tokens/filesystems/env), the
  override file rotates through the uniform `.ursine-backup[.N]`
  slots (`backup_configs.copies`) before the CLI rewrites it
  (max THREE copies), and every key
  outside the four edited lists is preserved (no `--reset`).

## GUI

Sidebar entries **APK packages** and **Flatpak applications**. Each panel has:

- source dropdown (APK/Flatpak);
- Flatpak system/user dropdown;
- Remotes section: add form (name + URL for the current scope) and a
  per-remote Remove behind a confirmation (flatpak refuses while
  packages still come from the remote);
- Flatpak Show dropdown (Apps | Runtimes) over rows tagged `kind`
  (list runs `flatpak list --app` and `--runtime` — the ref column has
  no kind prefix);
- search and direct-install fields (Flatpak: remote dropdown next to the
  install entry, sizes shown in the install confirmation);
- installed/search rows with install/remove actions;
- refresh/update controls with confirmations;
- Upgrade section shows only the update COUNT for both managers (the
  per-package lists are gone by user decision); per-package Upgrade
  buttons live in Installed and appear only when the check says the
  package is outdated;
- per-app Permissions editor (row button): sockets/devices/filesystems
  add-remove lists, environment name/value rows, read-only effective
  view from `flatpak info --show-permissions`, Apply writes the four
  lists.

The overall shell uses a GNOME Settings-style left sidebar for every logged-in
screen.

## Testing

- Unit: APK package/query validation, Flatpak id validation, scope flags;
  override diff: current vs wanted → exact flag list (foreign groups untouched)
  and permission-token validation in `modules/packages_flatpak/permissions.rs`.
- `cargo test --workspace --all-features`.
- VM smoke: get/set permissions on a real app, verify
  `flatpak override --show` reflects the edit and a Session Bus policy
  (when present) survives the write — the write path cannot run in CI.
- Manual: install/remove a harmless package on Alpine and Flatpak; verify
  capability-gated states when binaries are absent.

## Package progress (implemented 2026-10-27)

Mutations must stream the exact process output as terminal text, not a
percentage bar. The daemon owns a RAM job registry and writes stdout/stderr to
`/run/ursine/jobs/<id>.log`; frontends poll `job_output`. A **Close** button is
shown only after the job completes. This applies to APK, Flatpak and Nix.
