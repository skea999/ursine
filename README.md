# Ursine

Ursine is the control center for FeralOS: a root `ursined` daemon and a
libcosmic desktop client. The daemon reads system state live, runs package
operations, and exposes a newline-delimited JSON protocol over a Unix socket.
Authentication is a one-time PAM handshake; session tokens stay in RAM.

## Install with Nix

Requirements: Nix with flakes enabled, on Alpine Linux or FeralOS.

```bash
# 1. Add the flake
nix registry add ursine github:skea999/ursine

# 2. Install in the default profile
nix profile add ursine#ursine

# 3. Bootstrap the service (detects systemd, OpenRC or dinit)
sudo nix run github:skea999/ursine#activate
```

The last command installs `/etc/pam.d/ursine` and the init unit, then starts
`ursined`. It needs root; the Nix package itself only writes to the Nix store.

## Update

```bash
nix profile upgrade ursine
sudo nix run github:skea999/ursine#activate
```

The activation script is idempotent: it applies changed PAM/unit files and
restarts the daemon after every upgrade.

## Project overview

- `ursined` — root daemon; owns the Unix socket, PAM session, audit log, and
  privileged module actions.
- `ursine-core` — module implementations: apk, flatpak, nix, PAM, btrfs, and
  runtime capability detection.
- `ursine-api` — shared protocol and configuration types.
- `ursine` — libcosmic desktop client; login, capability-gated home, and store
  views.
- `web/` — static web assets served by the daemon.
- `tests/` — integration tests.
- `packaging/` and `install.sh` — systemd/OpenRC/dinit units, PAM policy,
  release staging, and cross-distro installation.
- `scripts/ursine-activate.sh` — Nix profile activation: detects the init,
  installs PAM/unit files, and restarts the daemon.

Architecture decisions live in
[docs/architecture/overview.md](docs/architecture/overview.md). Build and test
instructions are in [docs/guides/getting-started.md](docs/guides/getting-started.md).
Release installation is covered in
[docs/guides/installation.md](docs/guides/installation.md).

License: ISC.
