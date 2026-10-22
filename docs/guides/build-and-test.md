# Build and test (canonical)

Every build, test and static check runs inside the project container. The host
is never used for compilation or tooling. This is the only supported workflow:
CI and local runs use the same commands and the same environment.

## 1. One-time image build

```bash
podman build -t ursine-ci:1 -f ci/Containerfile.dev .
```

The image is Alpine/musl, matching FeralOS and CI. It contains Rust, clippy,
rustfmt, PAM headers and the GUI build dependencies.

## 2. Standard checks

```bash
podman run --rm --network=host -v "$PWD:/work" -w /work \
  -e CARGO_HOME=/work/artifacts/cargo-home ursine-ci:1 \
  sh -c 'cargo fmt --check'

podman run --rm --network=host -v "$PWD:/work" -w /work \
  -e CARGO_HOME=/work/artifacts/cargo-home ursine-ci:1 \
  sh -c 'cargo check --workspace --all-targets --all-features'

podman run --rm --network=host -v "$PWD:/work" -w /work \
  -e CARGO_HOME=/work/artifacts/cargo-home ursine-ci:1 \
  sh -c 'cargo clippy --workspace --all-targets --all-features -- -D warnings'

podman run --rm --network=host -v "$PWD:/work" -w /work \
  -e CARGO_HOME=/work/artifacts/cargo-home ursine-ci:1 \
  sh -c 'cargo test --workspace --all-features'
```

`--network=host` is required on this host: pasta cannot create the tap device.

## 3. Static analysis

Static tools live in `ci/Containerfile.static` and are driven by
`scripts/static-check.sh`. Build the image once:

```bash
podman build -t ursine-static:1 -f ci/Containerfile.static .
```

Run everything:

```bash
podman run --rm --network=host -v "$PWD:/work" -w /work \
  -e CARGO_HOME=/work/artifacts/cargo-home ursine-static:1 \
  sh scripts/static-check.sh
```

Run one group:

```bash
podman run --rm --network=host -v "$PWD:/work" -w /work \
  -e CARGO_HOME=/work/artifacts/cargo-home ursine-static:1 \
  sh scripts/static-check.sh rust
```

Groups: `rust`, `repo`, `sast`, `deps`, `duplicates`, `all`.

Policy and gates: `docs/STATIC-ANALYSIS.md`.

## 4. Feature matrix

```bash
# default (stub auth)
... cargo test --workspace --no-default-features

# real PAM build
... cargo test --workspace --features pam

# everything
... cargo test --workspace --all-features
```

## 5. Release builds

Release tarballs are built only by GitHub Actions (`package` job, Alpine/musl).
Local builds are debug/test builds; never publish a local artifact. Nix does
not compile Rust: `flake.nix` fetches the pinned GitHub release tarball from
`nix/binary.nix`.

## 6. Nix checks

All Nix work runs in `nixos/nix`; never install Nix on the host.

```bash
podman run --rm --network=host -v "$PWD:/work" -w /work nixos/nix \
  sh -c 'nix --extra-experimental-features "nix-command flakes" flake check'
podman run --rm --network=host -v "$PWD:/work" -w /work nixos/nix \
  sh -c 'nix --extra-experimental-features "nix-command flakes" build --out-link /work/artifacts/result'
```

## 7. Rules

- No host `cargo`, `rustc`, `nix`, `clippy`, `cargo-deny`, `cargo-vet` or
  scanner. Everything in a container.
- Build output: `artifacts/target` (`.cargo/config.toml`). Never `target/`.
- Temporary files: `artifacts/<task>/`. Never host `/tmp`.
- `Cargo.lock` is committed; every CI cargo command uses `--locked`.
- Do not skip hooks or checks. Fix the finding.

## Static image contents

`ci/Containerfile.static` is based on `alpine:3.24`. It installs from apk:

- `cargo`, `cargo-deny`, `cargo-audit`, `cargo-cyclonedx`
- `typos`, `taplo`, `zizmor`, `actionlint`, `shellcheck`, `shfmt`, `editorconfig-checker`
- `nodejs`/`npm` for `markdownlint-cli2` and `jscpd`

Missing upstream tools are downloaded as musl/static release binaries:
`cargo-machete` and `gitleaks`. `cargo-vet` and `cargo-geiger` have no Linux
musl binaries; they are optional. `cargo-vet` is replaced by
`cargo-deny`/`cargo-audit`; `cargo-geiger` is replaced by
`unsafe_code = "forbid"`. Semgrep runs from its official image in CI.

The Rust compile/clippy/fmt group runs in `ursine-ci:1`; the static image keeps
only analyzers so its build stays fast.
