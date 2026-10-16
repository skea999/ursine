# SPEC-packaging-infra

## Objective

Everything that makes Ursine buildable, testable, releasable and packagable — musl everywhere: `flake.nix` (static musl builds), GitHub CI (per-feature test matrix, clippy/fmt gate, flake check/build, Release artifacts), Docker Alpine local builds, dinit service files. Written last per module but its skeleton lands with the workspace (kept green from day one).

## Tech Stack

- Nix flake: `pkgsMusl`/`pkgsStatic` (x86_64-linux musl static) — NEVER the default glibc stdenv
- **CI fully containerized (binding)**: every job runs inside a Docker container on GitHub-hosted runners — NO build toolchain installed on the host:
  - cargo jobs: `container: alpine:3.22` + `apk add rust cargo git nodejs` (`nodejs` required so JS actions like checkout run inside the musl container); `linux-pam-dev` added only for `pam` matrix jobs
  - flake/release jobs: `container: nixos/nix` (git present / `nix-env -iA nixpkgs.git` if missing) — nix itself runs INSIDE the container
- Local dev mirrors CI exactly: `podman run (or docker) --rm -it -v "$PWD:/work" -w /work rust:alpine …`
- Packaging: APKBUILDs in `skea999/feralos-aports` (out of this repo — dinit service + config example shipped here in `dinit/` for the APKBUILD to install)

## CI workflow (`.github/workflows/ci.yml`)

1. `test` (matrix over cargo features that EXIST: `default`, `pam`, `full`; web/oidc enter the matrix when their features land in v3):
   - **Docker**: `alpine:3.22` container
   - `apk add rust cargo git nodejs` (+ `linux-pam-dev` on pam jobs)
   - `cargo test --workspace --no-default-features` / `--features pam` / `--features full`
   - gate: `cargo clippy --workspace --all-features -- -D warnings` + `cargo fmt --check`
2. `flake`: **Docker**: `nixos/nix` container → `nix flake check` + `nix build --out-link artifacts/result` on push/PR (out-link rule: nothing but source files ever appears at repo root)
3. `release` (tags only): same `nixos/nix` container builds musl binaries → attach to GitHub Release (ursined, ursine + dinit files); flake published by existing in repo → `github:skea999/ursine`
4. NO job installs rustup/cargo/nix on the host runner; actions that need node work because containers provide `nodejs` (alpine) or ship it (nix image)

## Commands

```
# local (dev machine, no toolchain needed)
docker run --rm -it -v "$PWD:/work" -w /work rust:alpine cargo test --workspace
docker run --rm -it -v "$PWD:/work" -w /work rust:alpine sh -c "apk add linux-pam-dev && cargo build -p ursined --features pam"

# nix
nix build --out-link artifacts/result   # both binaries, musl static; symlink in artifacts/
nix flake check
```

## Project Structure

```
flake.nix                  musl static: ursined + ursine
flake.lock
.github/workflows/ci.yml   test matrix + flake + release (written, COMMENTED until v0.1)
.cargo/config.toml         target-dir = "artifacts/target" (no target/ at root)
scripts/clean-artifacts.sh keep LAST 3 nix out-links + nix-collect-garbage (-d); --all = + cargo clean
dinit/
├─ ursined                 system service (depend on: dbus; respawn)
└─ ursined.conf
.dockerignore              artifacts/, target/
docs/guides/getting-started.md  local build instructions (exists)
```

**Retention policy (local disk):** keep only the LAST 3 build copies. Every
`nix build --out-link artifacts/result*` registers a GC root; deleting old
links unroots those store paths and `nix-collect-garbage -d` reclaims them.
Run `scripts/clean-artifacts.sh` after builds or whenever disk is tight
(`--all` also cleans `artifacts/target`).

## Code Style

- Workflow YAML: one job per concern, `fail-fast: false`, concurrency cancel on branch
- flake.nix: crane or naersk for musl builds; outputs: `packages.x86_64-linux.ursined`, `.ursine`, default package set; `devShells` NOT promised (Docker is the dev env)

## Testing Strategy

- CI IS the test infra: green matrix = merge gate
- flake: `nix flake check` must pass locally before tag
- Release artifacts smoke-tested by extracting tarball in an Alpine container and running `ursined --version`

## Boundaries

- Always: keep CI green on every push (skeleton lands with workspace); musl targets only
- Ask first: adding CI providers beyond GitHub Actions; changing flake inputs; adding a third build target (aarch64?)
- Never: glibc builds or artifacts; publishing binaries without the Release smoke test; secrets in workflow files

## Success Criteria

- Push → green CI: every job ran inside a container (alpine for cargo, nixos/nix for flake), matrix (default/pam/full) on musl + flake check/build
- Tag → GitHub Release with musl static binaries + dinit files
- A dev machine with only Docker can build and test everything
- `nix run github:skea999/ursine#ursined -- --version` works on any nix-enabled musl host

## Open Questions

- aarch64 musl target needed in v1? (FeralOS is x86_64-only today — default: no)
- Cachix binary cache for the flake (faster local nix builds): skip v1?
