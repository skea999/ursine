# Build and test (canonical)

Every build, test and static check runs inside the project container. The host
is never used for compilation or tooling. This is the only supported workflow:
CI and local runs use the same commands and the same environment.

## 1. One-time image build

```bash
podman build -t ursine-ci:1 -f ci/Containerfile.dev .
```

The image is Alpine 3.24/musl, matching FeralOS and the static/docs tooling
images. It installs system libraries from apk and Rust 1.99.1 via the apk
`rustup` package (pinned by `rust-toolchain.toml`); it includes clippy,
rustfmt, PAM headers and the GUI build dependencies.

## 2. Standard checks

Canonical shortcut (same images and commands as CI):

```bash
scripts/dev.sh check        # fmt + clippy + test
scripts/dev.sh static all   # analyzers
scripts/dev.sh docs         # Markdown
```

Manual equivalents, if you need one command:

```bash
podman run --rm --network=host -v "$PWD:/work" -w /work \
  -e CARGO_HOME=/work/artifacts/cargo-home ursine-ci:1 \
  sh -c 'cargo fmt --check'

podman run --rm --network=host -v "$PWD:/work" -w /work \
  -e CARGO_HOME=/work/artifacts/cargo-home ursine-ci:1 \
  sh -c 'cargo clippy --workspace --all-targets --all-features -- -D warnings'

podman run --rm --network=host -v "$PWD:/work" -w /work \
  -e CARGO_HOME=/work/artifacts/cargo-home ursine-ci:1 \
  sh -c 'cargo test --workspace --all-features --locked'
```

`--network=host` is required on this host: pasta cannot create the tap device.

## 3. Static analysis

Analyzers live in `ci/Containerfile.static`; Rust lints use `ursine-ci:1`.
Build both:

```bash
scripts/dev.sh image
```

Run everything (analyzers only; Rust lints run in `ursine-ci`):

```bash
scripts/dev.sh static all
```

Groups: `repo`, `sast`, `deps`, `duplicates`, `sbom`, `all`. The `rust` group
is available only in `ursine-ci:1` (`scripts/dev.sh check`).

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

- No host `cargo`, `rustc`, `nix`, `clippy`, `cargo-deny` or
  scanner. Everything in a container.
- Build output: `artifacts/target` (`.cargo/config.toml`). Never `target/`.
- Temporary files: `artifacts/<task>/`. Never host `/tmp`.
- `Cargo.lock` is committed; every CI cargo command uses `--locked`.
- Do not skip hooks or checks. Fix the finding.

## 8. GitHub Actions pipeline

`ci.yml` is ordered for wall-clock time, not for duplicating builds:

1. **image** builds the Alpine musl dev image once and pushes
   `ursine-ci:main` (or uses the GHA layer cache). Test/lint/gui/package pull
   that image.
2. **test (default)** and **test (pam)** run `cargo test` for API/core/daemon
   and integration tests. The GUI is excluded here because it has its own job.
3. **lint** runs one `cargo clippy --workspace --all-targets --all-features`,
   `cargo fmt --check`, and rustdoc with `-D warnings`. The GUI clippy pass is
   no longer a second job.
4. **gui** runs `cargo test -p ursine --all-targets --locked`. This compiles
   and runs the GUI tests; there is no separate `cargo build`, and clippy is
   not repeated.
5. **analyzer**, **docs**, and **zizmor** run in parallel. `analyzer` runs the
   repo/sast/deps/duplicates/sbom groups; `docs` runs markdownlint; `zizmor`
   audits workflow security.
6. **version** resolves the workspace version and whether a package is due.
   **changes** supplies the PR package/nix path filters.
7. **package** starts after `image`/`version`/`changes`, in parallel with the
   test/lint/gui/analyzer/docs jobs. It builds release PAM `ursined` and the GUI,
   runs the installer smoke tests, creates the tarball/checksum, and uploads
   `ursine-release`. This path has no dependency cache on purpose.
8. **publish** waits for `package`, `analyzer`, `docs` and `zizmor`; analyzer
   and docs can finish while the release build runs, but nothing is published
   until they pass. It publishes tag assets or the rolling `latest`, and
   updates `nix/binary.nix` on tags.
9. **flake** runs on PR/main only when `flake.nix`, `flake.lock`, or `nix/**`
   changed. It is not a dependency of `package`, so a tag does not try to
   fetch a not-yet-published artifact.
10. **verify-pin** runs after a tag publish and executes `nix flake check` and
    `nix build` against the new pin on `main`.

Cargo cache policy: test/lint/gui/analyzer use `actions/cache/restore`; only
pushes to `main` save (`actions/cache/save`). Tag/PR refs are restore-only, and the
release package path is never cached. This keeps zizmor's cache-poisoning
audit clean while avoiding repeated dependency compilation on main.

`cargo test` builds the test binaries before running them; a separate
`cargo build` cannot replace that work. The optimization is to remove a
second GUI build and to start the release build before the tests finish,
with `publish` still blocked on every quality gate.

## Static image contents

`ci/Containerfile.static` is based on `alpine:3.24`. It installs from apk:

- `cargo`, `cargo-deny`, `cargo-audit`, `cargo-cyclonedx`
- `typos`, `taplo`, `zizmor`, `actionlint`, `shellcheck`, `shfmt`, `editorconfig-checker`
- `cargo-geiger`, `cargo-machete`, `gitleaks` (Alpine edge/testing)

The Rust compile/clippy/fmt group runs in `ursine-ci:1`; the static image keeps
only analyzers so its build stays fast.

## Documentation checks

Docs are not baked into any image; the repo is mounted at runtime.

```bash
podman build -t ursine-docs:1 -f ci/Containerfile.docs .

podman run --rm --network=host -v "$PWD:/work" -w /work \
  ursine-docs:1 sh scripts/docs-check.sh
```

CI runs the same image and script in the `docs` job.
