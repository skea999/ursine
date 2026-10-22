# SPEC-packaging-infra

## Objective

Everything that makes Ursine buildable, testable, releasable and packagable — musl everywhere: `flake.nix` (dynamic musl builds), GitHub CI (per-feature test matrix, clippy/fmt gate, flake check/build, Release artifacts), Docker Alpine local builds, dinit service files. Written last per module but its skeleton lands with the workspace (kept green from day one).

## Tech Stack

- Nix flake: packages the **pinned musl release artifact** (`nix/binary.nix`: URL + sha256) — a `stdenvNoCC` derivation, no compilation. WHY: nixpkgs does not cache `pkgsMusl`/`pkgsStatic` toolchains (stable branches included — verified on nixos-26.05), so a from-source musl build compiles musl+rustc for hours on every run. Binaries are built in the Alpine CI container (same libc, fast) and the flake makes them consumable reproducibly (`nix build github:skea999/ursine#ursine`). The tarball also contains `install.sh` and `packaging/`, and the flake builds the dependency chain `ursine` (GUI) → `ursined` (daemon + packaging) → `ursine-activate` (system tools); `apps.activate`/`apps.deactivate` invoke the versioned tools to install/remove the PAM service and init unit against a stable profile. CI updates the pin on every `v*` tag; the pinned v0.3.0 tarball predates the scripts, but the flake adds them as a dependency and the next tarball carries them too.
- Binaries are **dynamic musl** (never static): PAM loads `pam_unix.so` via `dlopen`; static musl has no dlopen. glibc variants are a v6 concern (two artifacts from the same source).
- **Release checklist**: CI updates `nix/binary.nix` version/url/sha256 on every `v*` tag; manual bump remains the fallback (`sha256sum` of the tarball). The tarball must contain `bin/`, `install.sh` and `packaging/`; CI smoke-tests the installer into a staging root.
- **CI fully containerized (binding)**: every job runs inside a container on GitHub-hosted runners — NO build toolchain installed on the host:
  - cargo jobs: `container: alpine:3.22` (FeralOS target branch) + `apk add rust cargo git nodejs` (`nodejs` required so JS actions like checkout run inside the musl container); `linux-pam-dev` added only for `pam` matrix jobs
  - flake/release jobs: `container: nixos/nix` (git present / `nix-env -iA nixpkgs.git` if missing) — nix itself runs INSIDE the container
- Local dev mirrors CI exactly: `podman run (or docker) --rm -it -v "$PWD:/work" -w /work rust:alpine …`
- Packaging: APKBUILDs in `skea999/feralos-aports` (out of this repo). The release tarball ships `install.sh` + `packaging/` (PAM service and systemd/OpenRC/dinit units) so the same installer works on every Linux distribution; package builds stage it with `--destdir` and an explicit `--init`.

## CI workflow (`.github/workflows/ci.yml`)

1. `test` (matrix over cargo features that EXIST: `default`, `pam`; `full` returns when web/oidc land in v3):
   - **Docker**: `alpine:3.22` container
   - `apk add rust cargo git nodejs` (+ `linux-pam-dev` on pam jobs)
   - `cargo test --workspace --no-default-features` / `--features pam` / `--features full`
   - gate: `cargo clippy --workspace --all-features -- -D warnings` + `cargo fmt --check`
2. `version` gate: reads `Cargo.toml`; on a tag it must equal `v<version>`. The expensive `package` job runs only for a tag or a version without an existing tag; PRs run it only when build inputs change. Cargo registry and `artifacts/target` are cached with `actions/cache`.
3. `flake`: **container** `nixos/nix` → `nix flake check` + `nix build --out-link artifacts/result` on push/PR — builds the pinned binary package in seconds (out-link rule: nothing but source files ever appears at repo root)
4. `package` job (alpine rust image) builds release binaries → versioned GitHub Release on tags, rolling `latest` prerelease on main. It is skipped when the workspace version is already tagged, unless a PR changes build inputs.
5. `publish` verifies the published checksum, updates `nix/binary.nix` on main on tags, and attaches `ursine-flake-<tag>.tar.gz` (`flake.nix`, `flake.lock`, `nix/`, `README.md`). Flake consumption: `github:skea999/ursine` (pinned via `nix/binary.nix`).
6. `verify-pin` re-runs `nix flake check` against main after a tag release, proving the auto-updated pin resolves.
7. NO job installs rustup/cargo/nix on the host runner; actions that need node work because containers provide `nodejs` (alpine) or ship it (nix image)

## Commands

```text
# local (dev machine, no toolchain needed)
docker run --rm -it -v "$PWD:/work" -w /work rust:alpine cargo test --workspace
docker run --rm -it -v "$PWD:/work" -w /work rust:alpine sh -c "apk add linux-pam-dev && cargo build -p ursined --features pam"

# nix
nix build --out-link artifacts/result .#ursine   # both binaries, dynamic musl; symlink in artifacts/
nix flake check

# release tarball installer (staging; no host changes)
./install.sh --destdir "$PWD/artifacts/install-root" --prefix /usr \
  --init systemd --no-start
./install.sh --check
```

## Project Structure

```text
flake.nix                  package chain: ursine -> ursined -> ursine-activate
flake.lock
.github/workflows/ci.yml   test matrix + flake + release (installer smoke test)
.cargo/config.toml         target-dir = "artifacts/target" + musl dynamic (-crt-static off)
ci/Containerfile.dev       dev/CI image: rust:1-alpine + linux-pam-dev + clippy/rustfmt
src/tools/ursine-activate.sh Nix profile activation: init detection, PAM symlink, unit render, restart
src/tools/ursine-deactivate.sh Nix profile deactivation: stop/disable, remove PAM/unit files
scripts/clean-artifacts.sh keep LAST 3 nix out-links + nix-collect-garbage (-d); --all = + cargo clean
install.sh                 cross-distro installer: paths, PAM, systemd/OpenRC/dinit
packaging/
├─ pam.d/ursine           auth + account PAM service (pam_unix)
├─ systemd/ursined.service
├─ openrc/ursined
└─ dinit/                 ursined service + wrapper + /etc/dinit.d/config template
.dockerignore              artifacts/, target/
docs/guides/getting-started.md  local build instructions
docs/guides/installation.md     release install, paths and packaging staging
```

**Retention policy (local disk):** keep only the LAST 3 build copies. Every
`nix build --out-link artifacts/result*` registers a GC root; deleting old
links unroots those store paths and `nix-collect-garbage -d` reclaims them.
Run `scripts/clean-artifacts.sh` after builds or whenever disk is tight
(`--all` also cleans `artifacts/target`).

## Code Style

- Workflow YAML: one job per concern, `fail-fast: false`, concurrency cancel on branch
- flake.nix: `stdenvNoCC` package over the pinned release artifact; outputs: `packages.x86_64-linux.ursine`, default package; `devShells` NOT promised (Docker is the dev env; use `nix/dev-flake.nix` for current-tree testing)

## Testing Strategy

- CI IS the test infra: green matrix = merge gate
- flake: `nix flake check` must pass locally before tag
- Release artifacts smoke-tested by extracting tarball in an Alpine container, staging systemd/OpenRC/dinit, checking PAM `0644`, and running the staged `ursined --version` to require `auth: pam`

## Boundaries

- Always: keep CI green on every push (skeleton lands with workspace); musl targets only
- Ask first: adding CI providers beyond GitHub Actions; changing flake inputs; adding a third build target (aarch64?)
- Never: glibc builds or artifacts; publishing binaries without the Release smoke test; secrets in workflow files

## Success Criteria

- Push → green CI: every job ran inside a container (alpine for cargo, nixos/nix for flake), matrix (default/pam) on musl + flake check/build
- Tag → GitHub Release with musl binaries + installer + PAM/init assets; installer staging smoke test passes
- A dev machine with only Docker can build and test everything
- `sudo ./install.sh` on systemd, OpenRC and dinit installs the PAM service and starts `ursined`
- `nix build github:skea999/ursine#ursine` works on any nix-enabled musl host

## Open Questions

- aarch64 musl target needed in v1? (FeralOS is x86_64-only today — default: no)
- Cachix binary cache for the flake (faster local nix builds): skip v1?
