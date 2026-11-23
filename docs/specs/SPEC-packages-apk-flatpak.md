# SPEC-packages-apk-flatpak

## Objective

Expose Alpine (`apk`) and Flatpak packages in the same thin-client control
center as the Nix store. Both backends are capability-gated and audited.

## API surface

| Action | Backend |
|---|---|
| `list_apk` | `apk info -v` + `apk info` (name/version pairing) + `/lib/apk/db/installed` (`T:` descriptions) + ONE `apk policy <names…>` (origin URL) + repositories list = `/etc/apk/repositories` **plus every file in `/etc/apk/repositories.d/`** (mask tag; row `label` built daemon-side) → `description` / `origin` / `origin_label` |
| `search_apk` | `apk search -v <query>` |
| `list_apk_repositories` | parse `/etc/apk/repositories` PLUS every regular file in `/etc/apk/repositories.d/` (`[@tag] url`); the row `label` is built daemon-side (`@tag`, else the path identity) |
| `add_apk_repository` | upsert the line + `apk update` (atomic write + backup) |
| `remove_apk_repository` | drop the line + `apk update` (atomic write + backup) |
| `install_apk` | `apk add <package>` (progress via job log) |
| `remove_apk` | `apk del <package>` |
| `upgrade_apk` | `apk upgrade` (progress via job log) |
| `list_apk_updates` | `apk update` FIRST (the check refreshes the indexes — replaces the old maintenance "Refresh indexes" button) then `apk version -l <` → `ApkUpdate[]` |
| `apk_remove_regex` | match installed names against the regex daemon-side (the apk CLI has none) → dry-run returns `{dry_run, would_change}`, apply runs one `apk del <names…>` job (destructive in the CLI) |
| `apk_upgrade_regex` | same match → one `apk upgrade <names…>` job |
| `list_flatpak` | `flatpak list --columns=application,name,version,branch,origin,description --system/--user` + best-effort orphan count (`flatpak uninstall --unused --system/--user` answered `n` — read-only plan parse; `orphans` field may be null) |
| `list_flatpak_remotes` | `flatpak remotes --columns=name,url [--system/--user]` |
| `add_flatpak_remote` | `flatpak remote-add <scope> <name> <url>` |
| `remove_flatpak_remote` | `flatpak remote-delete <scope> <name>` |

- Both remote mutations rotate the installation's `repo/config` first
  (uniform slots); `apk add`/`del` rotate `/etc/apk/world` first.
| `list_flatpak_updates` | `flatpak remote-ls <scope> --app/--runtime --updates --columns=…` |
| `search_flatpak` | `flatpak search --columns=... --system/--user <query>` |
| `install_flatpak` | `flatpak install -y --noninteractive <scope> <remote> <app>` (remote default `flathub`, validated) |
| `get_flatpak_info` | `flatpak remote-info <scope> <remote> <app>` (name, version, sizes) |
| `remove_flatpak` | `flatpak uninstall -y --noninteractive <scope> <app>` |
| `flatpak_remove_regex` | match app id/name daemon-side → dry-run `{dry_run, would_change}` or one `flatpak uninstall … <refs…>` job |
| `flatpak_upgrade_regex` | same match → one `flatpak update … <refs…>` job |
| `update_flatpak` | `flatpak update -y --noninteractive <scope>` |

- APK is system scope: root only.
- `list_apk_updates` is the one APK read that writes: the leading
  `apk update` rewrites the cached package index (there is no separate
  `refresh_apk` action since v0.47.0). It never touches `/etc/apk/world` or
  an installed package, and `list_apk`/`search_apk` stay purely read-only. A
  client that gates writes must treat this call as a write.
- Flatpak has system and user scope; user scope runs as the authenticated
  principal. Root may retarget user scope to any existing normal-login
  account through the wire `as_user` (`--as-user`); non-root principals
  may target only their own account (same hardened boundary as
  theming/crontab and niri), and system-scope actions reject `as_user`.
  The rule is uniform: list, search, install, info, remove, update, orphans,
  the two regex actions and the remote add/remove actions all honour the
  target when `system=false` and reject it when `system=true`.
- The GUI never shells out. It renders typed rows from `ursine-api`.

## Security

- Absolute trusted tool paths, argv only, no shell.
- Package IDs and search queries are validated before execution.
- Output is bounded by the shared exec helper.
- Destructive actions require GUI confirmation.
- Capability gates: `apk` binary; `flatpak` binary.
- Audit uses the normal module/action path; no secrets are logged.

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

The overall shell uses a GNOME Settings-style left sidebar for every logged-in
screen.

## Testing

- Unit: APK package/query validation, Flatpak id validation, scope flags.
- `cargo test --workspace --all-features`.
- Manual: install/remove a harmless package on Alpine and Flatpak; verify
  capability-gated states when binaries are absent.

## Package progress (implemented 2026-10-27)

Mutations must stream the exact process output as terminal text, not a
percentage bar. The daemon owns a RAM job registry and writes stdout/stderr to
`/run/ursine/jobs/<id>.log`; frontends poll `job_output`. A **Close** button is
shown only after the job completes. This applies to APK, Flatpak and Nix.
