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

ursinectl actions [--json]
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
- `set_update_auto manager=apk|flatpak|nix enabled=true|false` —
  persist the "check updates automatically" preference for one package
  manager (defaults: apk/flatpak on, nix off — the nix check re-fetches
  flakes and only runs on demand).
- `set_module_switch module=<id> value=auto|true|false` — persist and
  apply one switch

### capabilities (`caps`)

- `list` — every capability with `available`, `reason`, `fix`
- `refresh` — re-run detection

### packages_nix (`nix`)

`list_installed_nix`, `search_nix query=… [repo=…]`, `install_nix package=…`,
`list_nix_repositories [system=true]`,
`add_nix_repository name=… url=… [system=true]`,
`remove_nix_repository name=… [system=true] [source=registry|channel]`,
`remove_nix package=…`, `remove_all_nix`,
`remove_regex_nix pattern=… [dry_run=true]`,
`upgrade_nix [package=… | all=true]`,
`upgrade_regex_nix pattern=… [dry_run=true]`,
`generations_nix`, `rollback_nix [generation=…]`,
`list_nix_updates` (on-demand update check: compares each profile
element's pinned lock with the current `nix flake metadata` lock of its
flake reference; prints the count + names; network),
`wipe_history_nix [older_than=…] dry_run=…`,
`diff_closures_nix [generation=…]`, `store_gc_nix`,
`prune_generations_nix keep=… dry_run=…`

The nix actions name their module in the wire tag, like `install_apk` and
`install_flatpak` do, so an audit line or a client log says which package
manager ran. The bare names of earlier versions (`list_installed`, `search`,
`install`, `remove`, `remove_all`, `remove_regex`, `upgrade`,
`upgrade_regex`, `generations`, `rollback`, `wipe_history`,
`diff_closures`, `store_gc`, `prune_generations`) stay accepted as aliases,
so existing scripts keep working.

`list_nix_repositories` shows the configured repositories of one scope:
`system=true` is the system registry (`/etc/nix/registry.json`) plus root's
channels, the default is the target user's registry and channels. Builtin
registry defaults are never listed — they have no file to edit. A channel
fills a name the registry does not define, so a FeralOS install shows its
pinned `nixpkgs` once. `search repo=…` runs against any registry flake and
returns bare installables prefixed with that flake, so Install resolves
against the searched repository; `repo` defaults to `nixpkgs`.
`remove_nix_repository` is destructive (`--yes`): `source=registry` (default)
runs `nix registry remove` for the user, or edits the system registry file
for `system=true`; `source=channel` runs `nix-channel --remove`. A user-scope
call may only target the caller's own account.

### packages_apk (`apk`)

`list_apk`, `search_apk query=…`,
`list_apk_repositories`, `add_apk_repository url=… [tag=…]`,
`remove_apk_repository url=…`,
`install_apk package=…`,
`preview_apk_install package=…`, `remove_apk package=…`, `upgrade_apk`,
`list_apk_updates [refresh=true]`, `upgrade_apk_package package=…`

`list_apk_updates` answers from the cached index by default: it writes
nothing and needs no network, so a client may poll it. `refresh=true`
runs `apk update` first — that request rewrites the index cache and is
reported as mutating by the catalogue. `upgrade_apk_package` runs
`apk upgrade <package>` and answers `not_found` (with a `fix`) when the
package is not installed.

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
`list_flatpak` rows carry `kind` (`app` or `runtime`): the daemon queries
`flatpak list --app` and `--runtime` separately because the `ref` column
has no kind prefix. `list_flatpak_updates` answers the same row shape for
refs whose remote has a newer commit (`flatpak remote-ls --updates`).
`remote` defaults to `flathub` and must be a configured remote name.
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

`init_btrbk_config` reads the layout from the machine: the top-level
subvolumes `btrfs subvolume list -a /` reports (nested ones and the
snapshot-store subvolume are skipped) become the `subvolume` lines of the
written file, and the `snapshot_dir` comes from the machine too — an
existing `/btrbk/snapshots` (FeralOS) wins, then `/.snapshots`, which is
created when neither exists. Each subvolume is probed with
`btrfs subvolume show`, so a layout that cannot be snapshotted fails with
an error instead of a written config. When the listing cannot be read the
file falls back to the historical single name (`home` when `/home` exists,
else `@`).

`set_btrbk_config` only replaces the options it is given: an option an
earlier call wrote stays in the managed block, so a client changing one
option need not resend the rest. A managed option that also appears below
the first `volume`/`subvolume`/`target` line is refused with a `conflict`
error naming the key and line — btrbk reads such a line as that section's
value, which silently wins over the managed global one.

`create_snapshot` is destructive: `btrbk snapshot` applies the retention
rules afterwards and deletes the snapshots they no longer keep, so it needs
`--yes`.

`restore_snapshot` refuses a target that is a mount point (a mounted
subvolume cannot be renamed); unmount it or restore from a recovery boot.
The live target is kept in the same capped backup chain as every other
file (`backup_configs.copies`, 3 by default), so a restore beyond the cap
deletes the oldest saved tree of the target; the preview and the apply
answer name it in `would_delete`.

### users

`list_users`, `create_user name=… fullname=… admin=true|false`,
`delete_user uid=…|name=… [remove_files=true|false]`,
`set_password uid=…|name=… new_password=-`, `list_groups`,
`set_groups uid=…|name=… groups=[…]`

`uid` and `name` are alternatives: exactly one of the two, so a client
that knows the login name does not need a `list_users` round trip.

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

### apk regex (pattern actions)

- `apk_remove_regex pattern=… [dry_run=true]`, `apk_upgrade_regex
  pattern=… [dry_run=true]` — the daemon matches installed names (the
  apk CLI has no regex); dry-run previews `would_change`, the apply runs
  one job over the matched set (`--yes` required).
- `flatpak_remove_regex pattern=… [dry_run=true] [system=true|false]`,
  `flatpak_upgrade_regex pattern=… [dry_run=true] [system=true|false]` —
  same contract against Flatpak ref ids/names. The pattern matches
  applications AND runtimes (`Platform` selects the freedesktop runtime),
  and the dry-run adds `matches`: one row per ref with `app_id`, `name`,
  `branch` and `kind`, so a runtime and an id installed on several branches
  stay distinguishable. `would_change` still carries the bare ids.

### booster

`booster_config_get`, `booster_config_set keys={…}` and
`booster_regenerate [--yes]` (SPEC-booster).

- Config: surgical edit of the known `/etc/booster.yaml` keys — comments,
  unknown keys and the installer marker stay byte-identical; the file
  rotates through the `backup_configs` slots before the atomic write.
  `keys` is a JSON object of the known keys (`clear: [...]` removes
  them): `keys={"compression":"zstd","network.dhcp":"off"}`.
- Regenerate runs the distribution's own kernel-update process:
  `apk fix kernel-hooks` when `/etc/kernel-hooks.d` has hooks (FeralOS
  rebuilds the booster image + signed UKI there), otherwise
  `regenerate_images` / `booster build` per installed kernel. The output
  is the merged hook/process log; requires `booster`
  (`apk add booster`).

### apparmor

`apparmor_status`, `apparmor_set_mode path=… mode=enforce|complain|disable|enable`,
`apparmor_load path=…`, `apparmor_check path=…`, `apparmor_remove path=…`,
`apparmor_reload_all`, `apparmor_unconfined`, `apparmor_remove_unknown`,
`apparmor_profile_get path=…`, `apparmor_profile_set path=… content=…`
(`content=@file` reads the profile from a file), `apparmor_notifications_enable`,
`apparmor_cleanprof name=…`, `apparmor_autodep path=…`,
`apparmor_features_abi`, `apparmor_decode hex=…`,
`apparmor_easyprof binary=… [template=…] [policy_groups=…]
[template_vars=[…]]`, `apparmor_easyprof_list what=templates|policy_groups`
(SPEC-apparmor).

- `apparmor_easyprof` only PRINTS the generated profile (aa-easyprof's
  `--output-directory` is never passed — no profile file is written);
  copy the output into a profile file yourself to persist it.
- `template_vars` declares policy variables as `@{NAME}=VALUE` (each passed
  as `--template-var`), e.g. `template_vars=["@{APPNAME}=scanner-app"]`:
  policy groups that write `@{APPNAME}` (`opt-application`) fail
  verification without the declaration; malformed entries answer `invalid`.
- Profile backups live outside `/etc/apparmor.d` (under
  `/var/backups/ursine/apparmor`) because every AppArmor tool parses that
  whole directory. `apparmor_cleanprof` runs `aa-cleanprof -s --no-reload`,
  so it never waits on stdin.

- All profile paths must live under `/etc/apparmor.d` (canonicalized —
  symlinks cannot escape).
- `apparmor_profile_set` refuses content that fails
  `apparmor_parser -d` BEFORE writing; the file rotates through the
  `backup_configs` slots first. Applying is a separate `apparmor_load`.
- `apparmor_notifications_enable` writes `show_notifications="yes"` into
  `/etc/apparmor/notify.conf` and installs the `aa-notify` autostart
  entry for the session user — notifications start at the next login.
- `enable` mode = remove the `disable/` symlink + reload (there is no
  aa-enable command).

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

Niri example (the account whose session the config belongs to):

```bash
sudo ursinectl niri get_niri_config --as-user gab
sudo ursinectl niri set_niri_section section=layout value=@layout.json --as-user gab
```

## Action catalogue (`ursinectl actions`)

`ursinectl actions` prints what the daemon accepts without running
anything: no root, no configuration, no audit line. Every action appears
with its module, its **effect** (`read-only` or `mutating`), its `--yes`
gate (`no`, `always`, `by request`) and its parameters — a trailing `?`
means the request may omit the field.

```console
$ ursinectl actions
ACTION                         MODULE            EFFECT     DESTRUCTIVE PARAMS
status                         system            read-only  no
…
install_apk                    packages_apk      mutating   no          package
list_apk_updates               packages_apk      read-only  no          refresh?
remove_regex_nix               packages_nix      mutating   by request  pattern, dry_run?
format_disk                    disks             mutating   always      device, fstype, confirm_device
```

`--json` prints the same data as one compact document, schema
`ursine.actions/1` — the form clients should consume (the table is for
humans and may gain columns):

```json
{"schema":"ursine.actions/1","count":143,"actions":[{"name":"install_apk","module":"packages_apk","effect":"mutating","destructive":"never","previewable":false,"params":[{"name":"package","required":true}]}]}
```

- `effect` is `read_only` when the request changes nothing on the machine
  (reads, previews, `dry_run` runs) and `mutating` otherwise. A dry-run
  capable action is listed as `mutating` with `previewable: true`: the
  request-level effect is read-only only while `dry_run` is true.
- `destructive` is the `--yes` verdict of the *variant*: `never`,
  `always` or `request` (the payload decides). `list_apk_updates` is
  `read-only` because the cached-index diff writes nothing; the request
  carrying `refresh=true` runs `apk update` and reports `mutating`, just
  like a `dry_run=true` request reports `read-only`.
- `params[].required` is `true` when the wire rejects a request that omits
  the field. There is no full JSON Schema yet: the catalogue carries names
  and the required flag only, never a type or a value constraint.

The classification itself lives in `ursine-api` (`Action::spec`,
`Action::is_read_only`, `Action::is_destructive`), so the CLI, the GUI and
any other client read one list.

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
`set_network_ipv4`, `unmount_disk`,
`format_disk`, `partition_create`, `partition_delete`,
`booster_regenerate`, `apparmor_remove_unknown`, `apparmor_cleanprof`,
`apparmor_set_mode` with `mode=disable`.

This list is documentation, not the implementation: the authoritative
classification is the `destructive` column of
[`ursinectl actions --json`](#action-catalogue-ursinectl-actions)
(`always` = flag required, `request` = only for the real run). A new action
is classified in `ursine-api/src/catalogue.rs`, so the CLI, the GUI and the
catalogue cannot disagree.

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
sudo ursinectl nix search_nix query=niri
sudo ursinectl nix install_nix package=niri --yes

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
- `docs/guides/installation.md` — installer, service units
- `docs/specs/SPEC-ursinectl.md` — design and test contract
