# PROJECT — Ursine

**Status**: Planning (architecture agreed, no code yet)
**Stack**: Rust (workspace) — see `AGENTS.md#Stack` and `CONTEXT.md`

## Vision

The control center of FeralOS — Webmin-style, complete: one root daemon (`ursined`) with a unified API over every system area (packages: nix/apk/flatpak, users, snapshots, niri, printers, scanners, bluetooth, network, firewall, disks, later Incus/ZFS/Pirateus, later other distros), with interchangeable frontends: native GTK4 GUI first, web + OIDC in v3, Incus UI v4, ZFS+Pirateus v5, multi-distro v6. Modules self-disable via runtime capability detection. Confirmed intent: `docs/intent/ursine.md` (source of truth).

## Users

- Primary: FeralOS desktop users (niri / niriWayle) who want a GUI for system management.
- Secondary: FeralOS server/hypervisor admins managing the machine via the web frontend.
- Not for: non-FeralOS distros (Alpine-specific assumptions: apk, dinit, musl are fine).

## Problem

FeralOS ships a hardened musl-based desktop (niri + Wayle shell) but has no control center. What exists on other distros is missing here:

- No GUI for Nix (none exists on any distro) or for a unified apk+flatpak+nix store.
- No lightweight users GUI (only heavy gnome-control-center).
- No btrfs snapshot/rollback GUI (snapper/timeshift not packaged on Alpine; btrbk is CLI-only).
- No niri config GUI (config.kdl has no editor anywhere).

Standalone tools exist (system-config-printer, blueman, nm-applet, firewalld-gui, simple-scan, gnome-disk-utility) — they remain usable fallbacks, but Ursine will cover all these areas natively via the daemons' own D-Bus/CLI interfaces, capability-gated, so the user has ONE product instead of a pile of apps.

## Solution

Single backend, thin frontends:

- `ursined` — root daemon, dinit service. Owns ALL logic: modules for packages (apk/flatpak/nix), users (accountsservice), snapshots (btrfs/btrbk), niri config, capabilities detection. Stateless: the system itself is the database.
- `ursine` — native GUI (GTK4/libadwaita, same stack as the Wayle shell), thin API client.
- `ursine-web` — static assets served by the daemon itself; talks the same API. PAM login today, OIDC later.
- `ursine-api` / `ursine-core` — shared crates: API types + module implementations.

Key choices (full rationale in `docs/architecture/overview.md`):

- Rust for everything (GUI must be Rust for GTK4/libadwaita; shared serde types make a second backend language a liability).
- No polkit. Auth = PAM handshake once per session (desktop GUI and web login), then Bearer session token. Web will move to OIDC (JWT validated against IdP) without touching the API.
- No database. Audit log = JSONL + logrotate. Config = `/etc/ursine/config.toml`.
- Features auto-detect the machine at runtime (`"auto" | true | false` per module); compile-time cargo features trim the binary (web/OIDC optional).

## Metrics

| Metric | Target | How |
|--------|--------|-----|
| Daemon idle footprint | ~3-6 MB RSS, 0% CPU (event-driven, no polling) | `bottom` / RSS sampling |
| Nix support | install/remove/search/list via GUI and web — unique among distros | manual test |
| Memory safety | 100% memory-safe userland (Rust), no C in our code | cargo audit, code review |

## Risks

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| PAM dev headers missing on dev machine | Certain (verified) | Low | PAM behind cargo feature; dev builds use stub auth |
| APKINDEX gaps force workarounds (e.g. hplip missing) | Medium | Low | Track feralos-aports; document gaps |
| nix/flatpak per-user execution from root daemon (su/setuid correctness) | Medium | High | `asUser` param, integration tests, review hardening |
| Token/auth mistakes (this is root-access software) | Low | Critical | Small auth surface, audit log every action, short token TTL |

## Out of Scope (today)

- AppImage, glibc builds in v1/v2.
- Non-FeralOS distros before v6 (multi-distro abstraction is a planned phase, not excluded).
- Web/OIDC/printers/scanners/bluetooth/network/firewall/disks/Incus/ZFS+Pirateus are NOT out of scope — they are v2-v5 phases of the same product (see `docs/intent/ursine.md`).
