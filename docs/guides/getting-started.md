# Getting Started

## Install a release

See [installation.md](installation.md) for the cross-distro installer, PAM
service and systemd/OpenRC/dinit units. This guide covers building from source.

## Prerequisites

- [opencode](https://opencode.ai) CLI
- podman (or docker; same commands)
- Git

No Rust/Nix/scanner toolchain is installed on the host. Everything runs in the
project containers.

## Canonical build and test

One-time image build:

```bash
podman build -t ursine-ci:1 -f ci/Containerfile.dev .
```

Then use the exact commands in [build-and-test.md](build-and-test.md). They are
the same commands CI runs: `cargo fmt --check`, `cargo check`, strict clippy,
`cargo test` in the Alpine/musl container.

Static analysis uses `ci/Containerfile.static` and `scripts/static-check.sh`;
policy and gates are in [../STATIC-ANALYSIS.md](../STATIC-ANALYSIS.md).

## Release and Nix

The release flake (`flake.nix`) packages the pinned dynamic-musl release
tarball in `nix/binary.nix`; it does not compile the workspace inside Nix. CI
builds the release tarball and runs `nix flake check` + `nix build`. See
[README](../../README.md) and [build-and-test.md](build-and-test.md).

## Disk space

Builds accumulate: nix store paths + `artifacts/` links + `artifacts/target`.

```bash
scripts/clean-artifacts.sh        # keep LAST 3 build copies + nix gc
scripts/clean-artifacts.sh --all  # + cargo clean (artifacts/target)
```

Run it after builds or whenever disk is tight.

Note: `libpam0g-dev` is NOT needed on the dev machine — PAM sits behind the
`pam` cargo feature. Dev builds compile a stub auth path. The FeralOS package
builds with `--features pam` (linux-pam is present on every FeralOS install via
sudo-rs).

## Session start (MANDATORY every session)

```bash
.agents/skills/session-memory/scripts/memory.sh context   # recent memories (bash version)
```

Then read, in order:

1. `handoff/latest.md` — where we left off, next steps
2. `state/session-state.json` — goals, decisions, risks
3. `docs/architecture/overview.md` — architecture decisions, do not violate
4. `CONTEXT.md` — domain vocabulary

At session end, update: `handoff/latest.md`, `state/session-state.json`,
`docs/TASKS.md` (and `CHANGELOG.md` for user-visible changes), plus:

```bash
.agents/skills/session-memory/scripts/memory.sh save "summary" "Today: X, next: Y"
```

## Build / test / lint

Use [build-and-test.md](build-and-test.md). Do not run cargo/nix on the host.

## Layout

All code under `src/`, all tests under `tests/`, build output + generated files under `artifacts/` (gitignored contents, folder tracked):

```text
src/ursine-api       shared protocol/config types (serde)
src/ursine-core      module implementations (apk, flatpak, nix, pam, btrfs, caps)
src/ursined          root daemon (dinit service)
src/ursine           libcosmic GUI (COSMIC toolkit, iced-based)
src/tools            shipped shell tools (activation/deactivation)
src/web              static web assets (not implemented yet)
tests/               integration tests (per area)
artifacts/           build output (artifacts/target via .cargo/config.toml) + generated files
```

There is NO `target/` directory — `.cargo/config.toml` sets `target-dir = "artifacts/target"`, so every cargo command (any crate, any container) writes under `artifacts/`.

Build deps for the GUI (libcosmic): `cmake`, `fontconfig-dev`, `freetype-dev`, `libxkbcommon-dev`, `expat-dev`, `pkgconf`; runtime: Mesa (GL/Vulkan) + `fontconfig`/`freetype`.

## Workflow

Agents and daily flow: `docs/agents-guide.md`. Sprint state: `docs/TASKS.md`.
