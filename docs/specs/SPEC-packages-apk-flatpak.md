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
| `upgrade_apk_package` | `apk info -e <package>` first: absent answers `not_found` with a `fix`; installed runs `apk upgrade <package>` (never `apk add`, which re-solves the world file and can drop packages it does not name) |
| `list_apk_updates` | `apk version -l <` → `ApkUpdate[]`; runs `apk update` FIRST only when `refresh=true` (that request rewrites the index cache and is mutating, the default read answers from the cache) |
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
| `flatpak_remove_regex` | match ref id/name daemon-side over applications **and** runtimes → dry-run `{dry_run, would_change, matches}` or one `flatpak uninstall … <refs…>` job |
| `flatpak_upgrade_regex` | same match → one `flatpak update … <refs…>` job |
| `update_flatpak` | `flatpak update -y --noninteractive <scope>` |

- `flatpak_remove_regex` / `flatpak_upgrade_regex` preview shape:
  `would_change` keeps the matched ids as bare strings (unchanged for
  existing clients); `matches` adds one row per matched ref with `app_id`,
  `name`, `branch` and `kind` (`app` | `runtime`). Both fields are needed:
  the id alone neither says whether the ref is an application or a runtime
  nor which branch it is on, and the same id appears once per installed
  branch. The apply sends one argv entry per distinct id — flatpak acts on
  every ref of an id it is given.
- Pattern matching stays unfiltered by kind: runtimes match unless the
  pattern excludes them, which is the pre-existing behaviour; the preview is
  what makes a runtime match visible.

- APK is system scope: root only.
- Flatpak has system and user scope; user scope runs as the authenticated
  principal. Root may retarget user scope to any existing normal-login
  account through the wire `as_user` (`--as-user`); non-root principals
  may target only their own account (same hardened boundary as
  theming/crontab), and system-scope actions reject `as_user`.
- `system` is optional on the wire for every flatpak action and defaults to
  `false` (the target user's installation, the same scope an explicit
  `--user` request asks for); the catalogue reports it as `required: false`.
  `list_flatpak_remotes` is the exception: its default is unset, which keeps
  the legacy both-installations listing.
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
- refresh/update controls with confirmations; the manual "Check for
  updates" sends `list_apk_updates refresh=true` (fresh index), the
  automatic check lists the cached index and writes nothing;
- refresh/update controls with confirmations;
- pattern previews (remove/upgrade matching) render one entry per matched
  ref as `id (kind, branch)` — the same id can sit on several branches and
  the pattern may have selected a runtime;
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
