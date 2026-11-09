# ursinectl — root CLI reference

`ursinectl` is the Ursine root command-line interface: every daemon action
from a terminal, running **in-process** (no daemon, no socket, no dinit
required — it works inside a FeralOS chroot during installation) and
therefore **root-only**.

> **Binding: keep this guide in parity with the CLI.** Any change to the
> `ursinectl` surface — a new action, a new flag, an exit-code change, a
> changed default — MUST update this guide in the same task that ships the
> code. A CLI change merged without the matching guide update is incomplete
> work (same rule as the daemon/GUI/CLI parity rule in `AGENTS.md`).

## Root gate

The process refuses to run when the effective uid is not 0, **before any
side effect** (before config, audit or job directories are opened):

```console
$ ursinectl system status
error: ursinectl must run as root (use sudo)
$ echo $?
2
```

There is no override flag and no environment escape: `ursinectl` is always
invoked through `sudo` (or from a root shell / a chroot as root).

## Synopsis

```text
ursinectl [--json] [--yes] [--as-user USER] [--config PATH] \
          <module> <action> [key=value ...]

ursinectl --help [module]
ursinectl --version
```

## Global options

| Option | Meaning |
|--------|---------|
| `--json` | Print the result as one compact JSON line (scripting). Without it the result is pretty-printed JSON (the contract; per-module tables are not required). |
| `--yes`, `-y` | Confirm a destructive action (see below). Without it the CLI refuses and exits 2. |
| `--as-user USER` | Target another account for the user-scoped modules (`packages_nix`, `theming`, `crontab`, Flatpak with `system=false`). The user must exist; see [Target users](#target-users). |
| `--config PATH` | Configuration file (default `/etc/ursine/config.toml`). The same guards apply as in the daemon: regular file, not group/world-writable. |
| `--help`, `-h` | Full usage, or the action list of one module (`ursinectl --help nix`). Works without root. |
| `--version` | Print `ursinectl <version>` and exit. Works without root. |

Options may appear before or after the module/action; `--as-user=U` and
`--config=P` (equals form) are accepted too.

## Value syntax (`key=value`)

- **JSON when possible**: `dry_run=true`, `keep=3`, `offset=0`,
  `value={"layout":{"gaps":8}}` — a value that parses as JSON keeps its
  type; anything else is a string (`query=niri`).
- Numbers are coerced into string fields where that is the only fit
  (`port=8080` → `"8080"`); real type errors still fail
  (`keep=abc` → usage error).
- **`key=@PATH`** reads the file content as the value
  (`content=@/tmp/my.cron`).
- **`key=-`** reads the value from stdin (whole input).
- **Unknown keys are rejected** (`unknown field …`, exit 2): a typo never
  silently does nothing.
- **Secrets** (`password`, `new_password`) must be `key=-`. Passing them
  on the command line is refused (argv is visible in `/proc`):

```console
# the password is read from stdin — never from argv
$ printf 'hunter2\n' | sudo ursinectl users set_password uid=1000 new_password=- --yes
```

## Modules and actions

Action names are the stable wire names (identical to the API tag and the
audit log). Run `ursinectl --help` for the grouped list.

### system

- `status` — version, protocol version, auth backend
- `job_output job_id=… offset=…` — poll a background job log (the CLI
  does this automatically for mutations)

### config

- `init` — write the commented `/etc/ursine/config.toml` template (only
  when absent; conflicts if it exists)
- `get_config` — current module switches, path, existence
- `set_module_switch module=<id> value=auto|true|false` — persist and
  apply one switch

### capabilities (`caps`)

- `list` — every capability with `available`, `reason`, `fix`
- `refresh` — re-run detection

### packages_nix (`nix`)

`list_installed`, `search query=… [repo=…]`, `install package=…`,
`list_nix_repositories`, `add_nix_repository name=… url=…`,
`remove_nix_repository name=…`,
`remove package=…`, `remove_all`, `remove_regex pattern=… [dry_run=true]`,
`upgrade [package=… | all=true]`, `upgrade_regex pattern=… [dry_run=true]`,
`generations`, `rollback [generation=…]`,
`wipe_history [older_than=…] dry_run=…`, `diff_closures [generation=…]`,
`store_gc`, `prune_generations keep=… dry_run=…`

`list_nix_repositories` shows the flake registry of the target user
(`user` entries editable, `global` read-only). `search repo=…` runs
against any registry flake and returns bare installables prefixed with
that flake, so Install resolves against the searched repository;
`repo` defaults to `nixpkgs`. `remove_nix_repository` is destructive
(`--yes`).

### packages_apk (`apk`)

`list_apk`, `search_apk query=…`, `refresh_apk`,
`list_apk_repositories`, `add_apk_repository url=… [tag=…]`,
`remove_apk_repository url=…`,
`install_apk package=…`,
`preview_apk_install package=…`, `remove_apk package=…`, `upgrade_apk`,
`list_apk_updates`, `upgrade_apk_package package=…`

`add_apk_repository` writes an `apk-repositories(5)` line and runs
`apk update`. A `tag` masks the repository (`@edge <url>`): apk
ignores it unless a world entry pins `pkg@tag`. Re-adding an existing
URL updates its tag; `tag=` empty unmasks it (the GUI "Unmask" button
does the same). `remove_apk_repository` deletes the line and is
destructive (`--yes`).

### packages_flatpak (`flatpak`)

`list_flatpak [system=true]`, `list_flatpak_remotes`,
`list_flatpak_updates [system=true]`, `search_flatpak query=… [system=true]`,
`add_flatpak_remote name=… url=… system=true|false`,
`remove_flatpak_remote name=… system=true|false`,
`install_flatpak app_id=… [system=true] [remote=…]`,
`get_flatpak_info app_id=… [remote=…] system=true|false`,
`remove_flatpak app_id=… [system=true]`, `update_flatpak [system=true]`,
`update_flatpak_app app_id=… [system=true]`,
`remove_flatpak_orphans [system=true]`,

`add_flatpak_remote` registers a location on the selected installation
(errors when the name exists); `remove_flatpak_remote` is destructive
(`--yes`) and flatpak itself refuses while packages still come from
that remote.
`get_flatpak_permissions app_id=… system=true|false`,
`set_flatpak_permissions app_id=… system=true|false sockets=[…]
devices=[…] filesystems=[…] env=[…]`

`list_flatpak` rows carry `kind` (`app` or `runtime`): the daemon queries
`flatpak list --app` and `--runtime` separately because the `ref` column
has no kind prefix. `list_flatpak_updates` answers the same row shape for
refs whose remote has a newer commit (`flatpak remote-ls --updates`).
`remote` defaults to `flathub` and must be a configured remote name.
`get_flatpak_permissions` returns the four editable override lists
(`sockets`, `devices`, `filesystems`, `env`) plus the raw effective view
from `flatpak info --show-permissions`. `set_flatpak_permissions`
replaces exactly those four lists and preserves every other key/group of
the override file (Session Bus policies are never dropped); it requires
`--yes`. `env` entries are `{name,value}` objects; an empty value unsets
the variable in the sandbox.
`get_flatpak_info` answers `name`, `version`, `download_size`,
`installed_size` and `runtime` for the install confirmation; a failed
lookup still allows the install (the job reports its own errors).
`list_flatpak_remotes` without `system` keeps the legacy both-installations
listing.

### printers

`list_printers`, `printer_jobs [printer=…]`,
`add_printer name=… uri=… description=… location=…`,
`add_printer_manual name=… uri=… driver=… ppd=… description=… location=… options=[…]`,
`remove_printer name=…`, `set_default_printer name=…`,
`set_printer_options name=… options=[…]`, `pause_printer name=… pause=true`,
`cancel_printer_jobs [printer=…]`

### scanners

`list_scanners`, `get_scanner_config`,
`set_scanner_device name=… address=… protocol=…`,
`remove_scanner_device name=…`, `set_scanner_discovery enabled=true|false`

### network

`get_network`, `set_wifi_enabled enabled=true|false`,
`connect_wifi ssid=… password=-`, `connect_network device=…`,
`disconnect_network device=…`, `forget_network connection=…`,
`set_network_connection connection=… up=true|false`,
`set_network_ipv4 connection=… method=dhcp|static addresses=[…]
[gateway=…] dns=[…]`

`forget_network` deletes a saved profile (Wi-Fi passwords included)
and `set_network_connection up=false` deactivates one — both are
destructive and require `--yes`; `up=true` activates (VPN connect).
`get_network` also lists every saved profile (`profiles`) and the GUI
derives its VPN section from `vpn`/`wireguard` kinds.

`set_network_ipv4` rewrites the profile's IPv4 settings (`dhcp` clears
addresses/gateway/DNS and takes none of them; `static` needs at least
one CIDR address) and re-activates the connection when it is active.
Connection names may contain spaces (`connection="Home Wi-Fi"`). The
action is destructive: it requires `--yes`.

### bluetooth

`get_bluetooth`, `set_bluetooth_power on=true|false`, `scan_bluetooth`,
`bluetooth_action address=… op=pair|connect|disconnect|trust|untrust|forget`

Pairing runs through `bluetoothctl --agent NoInputNoOutput pair` —
no custom D-Bus agent (invariant 3); Just Works devices pair without
further input.

### disks

`list_disks`, `mount_disk device=…`, `unmount_disk device=…`,
`format_disk device=… fstype=… confirm_device=…`,
`partition_create device=… start_mib=… end_mib=… confirm_device=…`,
`partition_delete device=… number=… confirm_device=…`

`list_disks` reports `format_enabled` (the `disks_format` daemon
config key). The three destructive actions require `disks_format =
true` in `/etc/ursine/config.toml` **plus a daemon restart**, a
`confirm_device` that equals `device` typed exactly, and `--yes`.
`fstype` ∈ `ext4, xfs, btrfs, vfat, exfat, f2fs, ntfs`; sizes are MiB.
`partition_create` needs an existing partition table (create one with
`parted mklabel gpt` first).

### services

`list_services`,
`service_action name=… op=start|stop|restart|enable|disable`

### firewall

`get_firewall`, `set_default_firewall_zone zone=…`,
`add_firewall_service zone=… service=…`,
`remove_firewall_service zone=… service=…`,
`add_firewall_port zone=… port=…`,
`remove_firewall_port zone=… port=…`, `reload_firewall`,
`set_firewall_enabled enabled=true|false`,
`create_firewall_service service=… [ports=[…]]`,
`delete_firewall_service service=…`

`get_firewall` also reports the per-zone `target`, `interfaces` and
`rich_rules` (the active-rules summary), the derived `enabled` flag
(default-zone target `ACCEPT` = filtering off) and the
`custom_services` definitions under `/etc/firewalld`.
`set_firewall_enabled` has no firewalld global switch: it sets the
default zone target to `REJECT` (on) or `ACCEPT` (off), permanent +
reload — `enabled=false` is destructive and needs `--yes`.
`create_firewall_service` defines a named service with optional ports;
add it to zones with `add_firewall_service`;
`delete_firewall_service` removes the definition (`--yes`).

### snapshots

`list_snapshots`, `stats`, `create_snapshot [section=…]`,
`restore_snapshot snapshot=… target=… dry_run=…`,
`get_btrbk_config`, `init_btrbk_config`, `set_btrbk_config settings={…}`,
`delete_snapshot snapshot=…`, `delete_snapshots sources=[…]`

### users

`list_users`, `create_user name=… fullname=… admin=true|false`,
`delete_user uid=… remove_files=true|false`,
`set_password uid=… new_password=-`, `list_groups`,
`set_groups uid=… groups=[…]`

### niri

`get_niri_config`, `validate_niri_config [proposed=…]`,
`set_niri_section section=… value={…}`

`get_niri_config` reports `version` (raw `niri --version`),
`version_parsed` (`major.minor[.patch]`), `version_supported` (`false`
below the tested minimum) and `min_supported`.
`validate_niri_config` returns `valid`, the raw `stdout`/`stderr`, and
`errors`: field-level entries `{message, line, column, section, field}`
mapped from niri's diagnostics when possible. `valid=false` exits with
code 1 even though the call itself succeeded.

### theming

`theme_detect`, `theme_get`, `theme_apply [show_minimize=false]`

### logs

`log_list`, `log_read file=… [lines=…] cursor={"mode":"end"|"before"|"from"}`,
`log_search file=… query=… [limit=…]`

Paths are relative to `/var/log`; escapes and binary files are rejected.

### crontab

`crontab_get`, `crontab_set content=…` (empty content clears it;
`content=@file` or `content=-` from stdin are the practical forms).

Operations run as root through the portable `crontab -u <user>` form —
no `su`, no suid requirement on the tool.

## Target users

The `--as-user` option:

- Accepted only by the user-scoped modules: `packages_nix`, `theming`,
  `crontab` and Flatpak actions on the user installation
  (`system=false`; system-scope Flatpak rejects it). Anywhere else
  dispatch rejects it (exit 1).
- **Root may target any existing normal-login account** (admin flow):
  `sudo ursinectl theming theme_get --as-user gab`.
- A foreign target is validated **before** the capability gate: unknown
  users fail with `user <name> does not exist` (typed NotFound);
  system accounts are rejected.
- Non-root principals (the GUI) may target only their own account.
- `theming` has no root fallback (root is not a login user): always pass
  `--as-user`.

Flatpak example (another account's user installation):

```bash
sudo ursinectl flatpak list_flatpak system=false --as-user gab
sudo ursinectl flatpak get_flatpak_info app_id=org.mozilla.firefox system=false --as-user gab
```

## Destructive actions (`--yes`)

Removing, deleting, rolling back, garbage collecting, applying
restores/crontab/theme changes require `--yes` (the CLI equivalent of the
GUI confirmation). Dry-run/preview variants never do. Without the flag the
CLI prints the refused action and exits 2. The current set includes:

`remove`, `remove_all`, `remove_regex` (apply), `upgrade_regex` (apply),
`wipe_history` (apply), `rollback`, `store_gc`, `prune_generations`
(apply), `remove_apk`, `upgrade_apk`, `upgrade_apk_package`,
`remove_flatpak`, `update_flatpak`, `update_flatpak_app`,
`remove_flatpak_orphans`, `restore_snapshot` (apply), `delete_snapshot`,
`delete_snapshots`, `delete_user`, `set_password`, `crontab_set`,
`theme_apply`, `remove_printer`, `cancel_printer_jobs`,
`remove_scanner_device`, `remove_firewall_service`, `remove_firewall_port`,
`set_firewall_enabled` (off), `delete_firewall_service`, `forget_network`,
`set_network_ipv4`, `set_flatpak_permissions`, `unmount_disk`,
`format_disk`, `partition_create`, `partition_delete`.

## Background jobs (package progress)

Mutations that answer `{"job_id": …}` are followed automatically: the CLI
streams the job log to **stdout as terminal text** (never a percentage
bar, per the binding package-progress rule) and exits with the job's exit
code.

```console
$ sudo ursinectl apk remove_apk package=oldpkg --yes
Removing oldpkg...
OK: 12 MiB in 3 packages
$ echo $?
0
```

CLI jobs live under `/run/ursine/cli-jobs` (never the daemon's
`/run/ursine/jobs`).

## Exit codes

| Code | Meaning |
|------|---------|
| `0` | Success (a background mutation finished with exit code 0). |
| `1` | Action/API error (capability unavailable, validation, job failure) or `validate_niri_config` answering `valid=false`. The message goes to stderr, with a `fix:` hint when the daemon provides one. |
| `2` | Usage error: bad arguments, unknown module/action/field, missing `--yes`, or the root gate. |

## Chroot usage

Because everything is in-process, `ursinectl` is the tool for an installed
system that has no running services yet:

```console
# inside the FeralOS install, as root
ursinectl caps list          # health probe of the target system
ursinectl system status
ursinectl logs log_list
```

Absolute paths (`/etc/ursine/config.toml`, `/var/log/ursine/audit.jsonl`,
`/run/ursine/…`) resolve inside the chroot automatically.

## Examples

```console
# status and capabilities
sudo ursinectl system status --json
sudo ursinectl caps list

# switch a module off, persistently
sudo ursinectl config set_module_switch module=crontab value=false

# nix: search, install (streams the job log, exits with its code)
sudo ursinectl nix search query=niri
sudo ursinectl nix install package=niri --yes

# manage another user's crontab and theme
sudo ursinectl crontab crontab_get --as-user gab
printf '* * * * * /bin/true\n' | sudo ursinectl crontab crontab_set content=- --as-user gab --yes
sudo ursinectl theming theme_detect --as-user gab

# logs: tail, previous chunk, follow offsets, search
sudo ursinectl logs log_read file=ursine/ursined.log lines=200
sudo ursinectl logs log_search file=ursine/ursined.log query="listen" limit=20
```

## Troubleshooting

- `unknown field 'x' for action 'y'` — a typo'd key; exit 2, nothing ran.
- `unknown variant 'x', expected one of …` — wrong action name (the list
  in the message is authoritative).
- `module disabled by configuration` — the module switch is off
  (`config get_config`, `config set_module_switch`).
- `reason: …` / `fix: …` on stderr — capability gate; the `fix:` line is
  the actionable hint (`apk add …`, `boot …`).
- `user 'x' does not exist` — `--as-user` typo (typed before any gate).
- `refusing destructive action … without --yes` — pass `--yes` after
  reviewing the action.

## Files

| Path | Role |
|------|------|
| `/etc/ursine/config.toml` | Module switches and daemon paths (shared with `ursined`). |
| `/var/log/ursine/audit.jsonl` | Append-only audit log: every CLI action is recorded with principal `root`. |
| `/run/ursine/cli-jobs/` | CLI background-job logs (tmpfs, per-process subdirectories). |
| `/usr/local/bin/ursinectl` | The binary (install.sh, or the nix profile via `ursine-activate`). |

## See also

- `man ursinectl` (shipped with the package)
- `docs/guides/installation.md` — installer, PAM, service units
- `docs/specs/SPEC-ursinectl.md` — design and test contract
