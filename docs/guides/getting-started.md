# Getting Started

## Prerequisites

- [opencode](https://opencode.ai) CLI
- Rust stable via rustup (NOT installed on this machine yet)
- Git

## Toolchain setup (once)

Container runtime: **podman** on this machine (docker works identically — same commands).

Preferred: NO local toolchain — build in the dev image (Alpine/musl, same as CI and FeralOS):

```bash
# one-time: build the dev image (rust + linux-pam-dev + clippy/rustfmt)
podman build -t ursine-ci:1 -f ci/Containerfile.dev .

# build / test / lint (network=host: pasta cannot set up tap on this host)
podman run --rm --network=host -v "$PWD:/work" -w /work \
  -e CARGO_HOME=/work/artifacts/cargo-home ursine-ci:1 cargo build --workspace

podman run --rm --network=host -v "$PWD:/work" -w /work \
  -e CARGO_HOME=/work/artifacts/cargo-home ursine-ci:1 cargo test --workspace --all-features

podman run --rm --network=host -v "$PWD:/work" -w /work \
  -e CARGO_HOME=/work/artifacts/cargo-home ursine-ci:1 sh -c \
  "cargo clippy --workspace --all-features -- -D warnings && cargo fmt --check"
```

Binaries land in `artifacts/target/debug/`. Run the daemon against a test config:

```bash
artifacts/target/debug/ursined --config artifacts/ursine-dev/config.toml
```

Optional native toolchain (if you want cargo on the host):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustup component add clippy rustfmt
rustup target add x86_64-unknown-linux-musl
cargo install cargo-audit   # optional: dependency vulnerability scan
```

Note: the nix flake (`flake.nix`, to be created at v0.1) builds statically on
musl via `pkgsMusl`/`pkgsStatic` — never glibc. CI runs `nix flake check` +
`nix build` and publishes Release artifacts on tags.

```bash
# nix build (local): ALWAYS out-link into artifacts/ — nothing at repo root
nix build --out-link artifacts/result
```

## Disk space

Builds accumulate: nix store paths + `artifacts/` links + `artifacts/target`.

```bash
scripts/clean-artifacts.sh        # keep LAST 3 build copies + nix gc
scripts/clean-artifacts.sh --all  # + cargo clean (artifacts/target)
```

Run it after builds or whenever disk is tight (it freed 443 MB on first run here).

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

```bash
cargo build --workspace                    # build all (dev profile, stub auth)
cargo build -p ursined --features pam      # daemon with real PAM (needs libpam)
cargo test --workspace                     # tests
cargo clippy --workspace -- -D warnings    # lint gate
cargo fmt                                  # format
cargo run -p ursined                       # run daemon (skeleton)
```

## Layout

All code under `src/`, all tests under `tests/`, build output + generated files under `artifacts/` (gitignored contents, folder tracked):

```
src/ursine-api       shared protocol/config types (serde)
src/ursine-core      module implementations (apk, flatpak, nix, pam, btrfs, caps)
src/ursined          root daemon (dinit service)
src/ursine           libcosmic GUI (COSMIC toolkit, iced-based)
tests/               integration tests (per area)
artifacts/           build output (artifacts/target via .cargo/config.toml) + generated files
```

There is NO `target/` directory — `.cargo/config.toml` sets `target-dir = "artifacts/target"`, so every cargo command (any crate, any container) writes under `artifacts/`.

Build deps for the GUI (libcosmic): `cmake`, `fontconfig-dev`, `freetype-dev`, `libxkbcommon-dev`, `expat-dev`, `pkgconf`; runtime: Mesa (GL/Vulkan) + `fontconfig`/`freetype`.

## Workflow

Agents and daily flow: `docs/agents-guide.md`. Sprint state: `docs/TASKS.md`.
