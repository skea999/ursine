# Ursine

Ursine is the control center for FeralOS: a root `ursined` daemon and a
libcosmic desktop client. The daemon reads system state live, runs package
operations, and exposes a newline-delimited JSON protocol over a Unix socket.
Authentication is a one-time PAM handshake; session tokens stay in RAM.

## Install with Nix

The repository is a Nix flake for `x86_64-linux`. The package installs the
pinned dynamic-musl release tarball from `nix/binary.nix`; it does not compile
the Rust workspace inside Nix.

Requirements:

- Nix with flakes enabled.
- Alpine Linux or FeralOS to run the packaged dynamic-musl binaries.
- Network access to fetch the pinned GitHub release tarball.

Build and inspect the package:

```bash
nix build --out-link artifacts/result .#ursine
./artifacts/result/bin/ursined --version
./artifacts/result/bin/ursine
nix flake check
```

Install the binaries into a Nix profile:

```bash
nix profile add .#ursine
```

The flake package places the binaries, `install.sh`, and `packaging/` in the
Nix store. It does not modify system PAM policy, init units, or configuration.
For the root daemon service, use `install.sh` as described in
[docs/guides/installation.md](docs/guides/installation.md).

### Add the flake as an input

Add the repository to another flake:

```nix
{
  inputs.ursine.url = "github:skea999/ursine";
}
```

Update it with:

```bash
nix flake update ursine
```

After a tag release, CI publishes the release tarball and updates the
`nix/binary.nix` pin on `main`. Updating the flake input therefore pulls the
current release pin. Tagged releases also attach a standalone
`ursine-flake-<tag>.tar.gz` source archive. Substitute the published tag in its
release URL if you want to pin the flake source directly instead of using
GitHub.

The current checked-in pin is the latest published release; it may lag the
working tree until the next release is tagged.

### Local development flake

The release flake packages a release artifact. To test the current working tree,
build the current binaries in the development container, then use the generated
local flake:

```bash
scripts/build-gui-dev.sh
nix run "path:$PWD/artifacts/ursine-dev#ursine"
```

The development binaries are dynamic musl and target Alpine/FeralOS. The GUI
also needs a Wayland compositor and Mesa/GL runtime libraries.

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

Architecture decisions live in
[docs/architecture/overview.md](docs/architecture/overview.md). Build and test
instructions are in [docs/guides/getting-started.md](docs/guides/getting-started.md).
Release installation is covered in
[docs/guides/installation.md](docs/guides/installation.md).

License: ISC.
