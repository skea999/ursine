# Statement of Intent — Ursine

Confirmed by skea999, 2026-10-16 (interview-me protocol: hypothesis → corrections → explicit yes).

**Amendment 2026-10-19**: GUI toolkit changed from GTK4/libadwaita to **libcosmic** (COSMIC toolkit, iced-based Rust UI). Reason: security — no GTK/gdk-pixbuf/librsvg C parsers in the GUI process (see `docs/architecture/overview.md`). Visual coherence with the GTK Wayle shell is now best-effort, not binding.

## Outcome

Ursine is the complete control center of FeralOS, in the spirit of Webmin: one root daemon (`ursined`) exposing a unified API over every system area — packages (nix, apk, flatpak), users, snapshots, niri config, printers, scanners, bluetooth, network, firewall, disks, and later Incus, ZFS/Pirateus — with interchangeable frontends (native libcosmic GUI, later web, later thin CLI `ursinectl`, later Incus UI). Modules turn themselves on or off via the runtime capability mechanism (`"auto"`): the machine's reality drives what is visible.

## User

skea999 on the FeralOS desktop (niri/Wayle) and as admin of FeralOS servers/hypervisors.

## Why now

FeralOS has no control center; Nix has no GUI on any distro; the FeralOS desktop ecosystem (Wayle, Veila, Raffi) is Rust — Ursine completes it with a memory-safe (libcosmic) app.

## Success

- Every module whose backend exists on the machine is fully operable from the GUI (and later web).
- CI compiles and tests every cargo feature combination and builds+publishes the musl nix flake.
- Daemon idles at ~3-6 MB RSS, 0% CPU.
- The UIs look and behave like one product, and a non-expert can use them.

## Binding constraints

- Memory-safe (Rust only, no C of ours), musl for FeralOS (nix included; dynamic musl binaries). GUI must use a Rust UI stack (libcosmic); web GUI must be memory-safe too.
- No polkit, no database, stateless daemon, frontends thin.
- Backends are the existing daemons, driven via D-Bus/CLI — never reimplemented.
- Local builds via Docker + Alpine images; CI per-feature test matrix + nix flake publish.

## Version plan (order of construction — everything is in scope)

| Ver | Content |
|-----|---------|
| **v1** | Backend skeleton (socket, PAM handshake, capabilities, audit) + **GUI libcosmic** (mandatory in v1): packages **nix** (the unique gap), users (accountsservice), snapshots (btrfs/btrbk), niri config editor |
| **v2** | Packages **apk** + **flatpak**; modules printers (CUPS), scanners (SANE), bluetooth (BlueZ), network (NetworkManager), firewall (firewalld), disks (UDisks2). No OIDC in v2 |
| **v3** | **OIDC + web interface + thin CLI** (web is the LAST GUI frontend; CLI `ursinectl` is a thin `ursine-api` client for headless/scriptable admin, decided 2026-10-27) |
| **v4** | **Incus UI** |
| **v5** | **ZFS + Pirateus** (FeralOS hyperconverged storage stack) |
| **v6** | **Support for other distros** (package-manager/init abstraction; glibc revisited here) |

## UI principles (binding)

- **All UIs must behave as one product**: same layout patterns, same navigation structure, same terminology (`CONTEXT.md` vocabulary is the source of truth), consistent interaction model. Visual theme follows each toolkit (COSMIC for the GUI, browser-native for the web); pixel-identical theming across toolkits is NOT required by default (security decision 2026-10-19: memory-safe toolkit wins over visual coherence). **Uniform theming decision (2026-10-27)**: a user-triggered "apply" propagates the shell theme (Wayle/DankMaterialShell) to every toolkit and application so all windows look identical — see `docs/specs/SPEC-theming.md`; the toolkit-independence rule above still governs default, unthemed rendering.
- **User-friendly**: COSMIC design conventions for the GUI; capability-gated panels (never show a broken module — show why it is off and how to enable it); explicit confirmation for destructive actions; plain language, no jargon; every action visible in the audit log is explainable in the UI.

## Out of scope (today)

AppImage, glibc builds in v1/v2, non-FeralOS distros before v6.

## Phasing ≠ exclusion

Web, OIDC, printers, disks, other distros are NOT out of scope: they are later phases of the same product. Out of scope means never, phasing means later.
