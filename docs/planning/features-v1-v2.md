# Feature inventory — v1 and v2

**Status:** planning reference.
**Sources:** `docs/intent/ursine.md`, `docs/specs/`, `docs/TASKS.md`.
**Versions:** `v1`/`v2` are product milestones, not the app semver
(`0.x` releases happen during construction).

Rules applied to every feature:

- backend first, GUI in the same feature;
- capability-gated, never a broken panel;
- destructive actions confirmed;
- audited, no secrets in logs;
- absolute tools/argv, bounded output, timeout;
- tests for validation, gating, failure paths.

## v1 — complete control center foundation

### Foundation

- Unix socket daemon, JSON-lines protocol, request IDs.
- PAM handshake; dev stub behind feature flag.
- RAM-only session token, expiry, sliding TTL.
- Failed-login rate limiting and bounded PAM concurrency.
- Audit JSONL: principal, module, action, target user, outcome; no passwords.
- Capability detection live, never persisted.
- Per-module tri-state config: `auto`, on, off.
- `asUser` for per-profile operations only.
- Client timeouts and bounded socket replies.

### GUI shell

- Login screen with user prefilled.
- GNOME Settings-style sidebar.
- Overview with module cards and capability status.
- Settings: capabilities refresh, module modes, config file status.
- Capability reasons and exact fix commands.
- Busy, empty and error states.
- Confirmation dialogs for destructive actions.
- Session expiry returns to login.
- Thin client: every decision in the daemon.

### packages.nix

- List installed packages in the target profile.
- Search nixpkgs.
- Install direct flake/attr and search result.
- Remove one, remove all, regex remove with preview.
- Upgrade one, upgrade all, regex upgrade with preview.
- Profile generations and history.
- Rollback to a generation.
- Closure diff between generations.
- Wipe history with dry-run preview.
- Prune generations with exact preview binding.
- Store garbage collection.
- `asUser` profile selector.
- Search result truncation indicator.

### snapshots (btrbk/btrfs)

- List snapshots.
- Stats per source subvolume.
- Create snapshot for all or one config section.
- Read btrbk config.
- Initialize default btrbk config.
- Edit managed global btrbk options.
- Restore preview.
- Restore with timestamped backup of existing target.
- Delete one snapshot.
- Delete all snapshots for selected sources.
- Capability gate: btrbk + btrfs.

### users (accountsservice)

- List human users with UID, name, full name, shell, home, groups.
- Create locked user.
- Administrator toggle (`wheel`).
- Set password with confirmation.
- Replace supplementary groups through runtime-selected tool
  (`usermod` / `addgroup`+`delgroup` / `gpasswd`).
- Delete user with home cleanup confirmation.
- UID policy: reject system and nobody-style users.
- Block removing the invoking principal’s own `wheel` membership.

### niri config editor

Already shipped baseline:

- layout gaps;
- animations off;
- touchpad tap/natural-scroll/dwt;
- cursor size/theme;
- KDL round-trip, comments/unknown nodes preserved;
- `niri validate` before write;
- atomic user-scoped write + backup;
- validate action and GUI result.

Still required for v1:

- keyboard layouts and repeat settings;
- pointer/touchpad full options;
- outputs: mode, scale, position, transform, VRR;
- layout: struts, border/focus-ring, preset widths/heights;
- animations: durations and curves where supported;
- keybinds with capture widget;
- gestures;
- environment variables add/remove;
- spawn-at-startup add/remove;
- screenshot path;
- hotkey overlay;
- schema/version check against `niri --version`;
- validation error mapped to the field where possible.

### Packaging and release

- `/etc/pam.d/ursine`.
- systemd, OpenRC, dinit units.
- install.sh with `DESTDIR`, path overrides, auth gate.
- activation/deactivation tools.
- release tarball and checksum.
- Nix package and pinned release artifact.
- CI: tests, lint, analyzer, docs, GUI, package, tag publish, Nix pin.
- FeralOS real PAM smoke before v1.0.

## v2 — full system coverage

### packages.apk

Implemented baseline:

- list installed;
- search repositories;
- refresh indexes;
- install;
- remove;
- upgrade all;
- capability `apk` and module switch.

v2 polish:

- show package descriptions/versions consistently;
- per-package upgrade;
- dependency/install size preview;
- repository list/dropdown;
- update-available list;
- operation progress/log;
- protected-package warnings.

### packages.flatpak

Implemented baseline:

- system/user list;
- search;
- install from `flathub`;
- remove;
- update all;
- system/user scope dropdown;
- capability `flatpak` and module switch.

v2 polish:

- remote dropdown, not hardcoded `flathub`;
- runtimes and apps separation;
- update-available list;
- per-app update;
- Flatpak permissions editor (filesystem, devices, sockets, env);
- install size/progress;
- user scope for another target user if required later.

### printers (CUPS)

- Capability: CUPS daemon/lpstat available.
- List printers, state, default printer.
- List queued jobs.
- Pause/resume printer.
- Cancel one/all jobs.
- Set default printer.
- Add network printer (URI/PPD or driverless IPP).
- Remove printer.
- Set common options: paper size, duplex, color mode.
- Confirmation for remove and cancel-all.
- GUI panel with printer list and job table.
- Backend: CUPS/IPP via `lpstat`, `lpadmin`, `cancel`, or CUPS D-Bus/CLI.

### scanners (SANE)

Configuration only; image acquisition is intentionally out of scope
(`docs/specs/SPEC-scanners.md`).

- Capability: `scanimage`/SANE available.
- List discovered devices.
- Read/write `/etc/sane.d/airscan.conf`.
- Add/update/remove manual airscan devices (name, address, protocol).
- Toggle airscan discovery.
- GUI panel with device list, form, discovery toggle, confirmation for remove.
- Backend: `scanimage -L` and fixed root-owned airscan config; no scan command.

### bluetooth (BlueZ)

- Capability: BlueZ D-Bus name present.
- Adapter power on/off.
- List known devices.
- Start/stop discovery.
- Pair, connect, disconnect, trust, remove.
- Pairing agent for PIN/passkey.
- Show battery/connected state where available.
- Confirm remove/forget.
- GUI: device list, scan button, dropdowns for adapter and actions.
- Secret/PIN handling ephemeral, never logged.

### network (NetworkManager)

- Capability: NetworkManager D-Bus name present.
- List devices and active connections.
- Ethernet connect/disconnect.
- Wi-Fi scan, list SSIDs, connect with password.
- Show signal, security, IP, gateway, DNS.
- DHCP and static IPv4 configuration.
- VPN connections where NetworkManager exposes them.
- Connection enable/disable, forget Wi-Fi.
- Secret agent integration; passwords never logged.
- GUI: device list, Wi-Fi dropdown, connect dialog, IP details.
- Confirmation for forget/remove connection.

### firewall (firewalld)

- Capability: firewalld D-Bus name present.
- Show default zone and zones.
- Enable/disable firewall.
- List/open/close services and ports per zone.
- Add/remove custom services.
- Reload firewalld.
- Show active rules summary.
- Confirm close/remove/reload.
- GUI: zone dropdown, service/port table, toggle.
- Backend: `org.fedoraproject.FirewallD1` D-Bus; no shell where possible.

### disks (UDisks2)

- Capability: UDisks2 D-Bus name present.
- List block devices and partitions.
- Show size, filesystem, label, UUID, mount point.
- Mount/unmount.
- SMART health where available.
- Format partition (explicit destructive confirmation, exact device typed).
- Partition create/delete only if explicitly enabled; otherwise v2 read-only.
- Show usage/free space.
- GUI: device table with detail pane, mount/unmount actions.
- No write to raw devices without confirmation; no silent format.

### Log viewer (SPEC-logs)

- [ ] `logs` module: dynamic `/var/log` discovery, tail with configurable
  line count (default 200), load-previous chunks, follow (`tail -f`
  polling), path-escape hardened; GUI sidebar section.
- [x] Search inside a log file (substring, click-to-jump).
- [x] Crontab section (SPEC-crontab): view/replace the session user's
  crontab via the portable `crontab <file>` form (busybox/cronie/vixie).

### v2 cross-cutting

- Every module opens on the CURRENT configuration if it exists on disk
  (read-only works with no display/compositor; live-only bits show an
  explicit unavailable note) — binding, see `docs/intent/ursine.md`.
- Capability checks and fixes for every backend.
- Module switches for every new module.
- `asUser` only where a per-user backend exists.
- Audit every mutation.
- Exit codes/signals normalized into actionable errors.
- GUI uses one sidebar, same header, confirmations, empty states.
- Tests: parsing, validation, capability gating, failure paths, timeout.
- Manual smoke on Alpine/FeralOS with real CUPS, SANE, BlueZ,
  NetworkManager, firewalld and UDisks2.
- Packaging: dependencies documented per module; missing backend disables panel.

### theming (uniform desktop)

User decision 2026-10-27; spec: `docs/specs/SPEC-theming.md`.

- Goal: every application looks identical regardless of toolkit (GTK3/4/2,
  Qt5/6 qtct, KDE, Flatpak, libcosmic/COSMIC, session env) **and per-app
  themes the shells do not cover** (Firefox userChrome, VSCodium color
  customizations, alacritty/kitty/ghostty/foot terminals, cava, Electron
  flags); source = Wayle or DMS shell config, running shell of the session
  user wins; inactive when neither is present. DMS/Wayle own configs are
  read-only sources.
- `theming` module (detect/get/apply) + GUI section, capability-gated,
  parity in the same task.
- Application title bar: planned as the separate app `ursine-titlebar`
  (per-window layer-shell buttons; v1 niri-only; later at most
  COSMIC/KDE/GNOME) — Ursine only configures it (native-button removal +
  its TOML from the shell snapshot). Full analysis in SPEC-theming §top bar.

### Optional v2 candidates

Not required by the intent table but compatible:

- users avatars (autologin is excluded: not feasible for this product);
- snapshot hook before package updates;
- system audit-log viewer;
- services panel for dinit/OpenRC;
- Flatpak permission editor (listed above as polish);
- dashboard for CPU/RAM/disk.

## Explicitly out of v1/v2

- OIDC, web interface → v3. (`ursinectl` CLI: was v3-thin-client; user
  decision 2026-10-30 — in-process root-only host of `ursine-core`, chroot
  capable, `docs/specs/SPEC-ursinectl.md`; can land any time after the
  current milestone.)
- Incus UI → v4.
- ZFS/Pirateus → v5.
- Multi-distro abstraction/glibc → v6.
- AppImage.
- glibc release builds.

## Acceptance for v1/v2

- Every daemon action has a GUI control; no backend-only feature.
- No panel appears broken when the backend is missing.
- Every destructive action confirmed.
- No password/secret in audit, logs, argv or environment.
- Tests and analyzer gates green.
- FeralOS PAM and package operations smoke-tested as root.
- v1.0 backend + GUI complete; packaging lives in the external `feralos-aports` repository.
