#!/bin/sh
# Canonical static analysis for Ursine. Run inside ursine-static:1.
#   sh scripts/static-check.sh [rust|repo|sast|deps|duplicates|sbom|all]
set -u

ROOT=$(CDPATH='' cd -- "$(dirname "$0")/.." && pwd)
cd "$ROOT" || exit 2
GROUP=${1:-all}
STATUS=0
mkdir -p artifacts/static artifacts/duplicates artifacts/sbom

step() {
  printf '\n==> %s\n' "$*"
}

run() {
  step "$*"
  if ! "$@"; then STATUS=1; fi
}

have() { command -v "$1" >/dev/null 2>&1; }

rust_group() {
  run cargo fmt --check
  run cargo check --workspace --all-targets --all-features
  run cargo clippy --workspace --all-targets --all-features -- -D warnings
  step "cargo doc"
  if ! RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps; then STATUS=1; fi
}

repo_group() {
  if have typos; then run typos; else step "typos: not installed"; fi
  if have editorconfig-checker; then
    run editorconfig-checker
  elif have ec; then
    run ec
  else step "editorconfig-checker: not installed"; fi
  set -- install.sh scripts/audit-threshold.sh scripts/build-gui-dev.sh scripts/clean-artifacts.sh scripts/static-check.sh \
    src/tools/ursine-activate.sh src/tools/ursine-deactivate.sh \
    packaging/openrc/ursined packaging/dinit/ursined.wrapper
  if have shellcheck; then run shellcheck -x "$@"; else step "shellcheck: not installed"; fi
  if have shfmt; then run shfmt -d "$@"; else step "shfmt: not installed"; fi
  if have taplo; then
    run taplo fmt --check Cargo.toml src/ursine-api/Cargo.toml src/ursine-core/Cargo.toml \
      src/ursine/Cargo.toml src/ursined/Cargo.toml rust-toolchain.toml deny.toml taplo.toml
  else step "taplo: not installed"; fi
  if have markdownlint-cli2; then run markdownlint-cli2; else step "markdownlint-cli2: not installed"; fi
  if have actionlint; then run actionlint; else step "actionlint: not installed"; fi
  if have zizmor; then run zizmor .github/workflows; else step "zizmor: not installed (CI runs the official image)"; fi
  if have statix && have deadnix && have alejandra; then
    run statix check flake.nix nix
    run deadnix --fail flake.nix nix
    run alejandra --check flake.nix nix
  else
    step "nix linters: run in nixos/nix (see docs/guides/build-and-test.md)"
  fi
}

sast_group() {
  if have semgrep; then
    run semgrep scan --config p/rust --config p/secrets --config .semgrep --error --quiet
  else
    step "semgrep: not installed here; CI runs the semgrep image"
  fi
  if have gitleaks; then run gitleaks detect --source "$ROOT" --log-opts=--all --redact --no-banner; else step "gitleaks: not installed"; fi
  if have cargo-geiger; then
    step "cargo-geiger reports (compile-time unsafe_code=forbid is the gate)"
    # GUI/daemon depend on libcosmic: unsafe_code=forbid covers them at compile time.
    for crate in ursine-api ursine-core; do
      cargo geiger --manifest-path "$ROOT/src/$crate/Cargo.toml" --all-features --output-format Json \
        >"artifacts/static/geiger-$crate.json" || STATUS=1
    done
  else
    step "cargo-geiger: not installed"
  fi
}

deps_group() {
  run cargo deny check
  run sh scripts/audit-threshold.sh
  run cargo vet --locked
  run cargo machete
}

duplicates_group() {
  step "duplicate crates (report only)"
  cargo tree -d >artifacts/duplicates/cargo-tree-duplicates.txt 2>&1 || true
  if have jscpd; then
    step "duplicate code (report only)"
    jscpd src >artifacts/duplicates/jscpd.log 2>&1 || true
  else
    step "jscpd: not installed"
  fi
}

sbom_group() {
  for crate in ursine-api ursine-core ursined ursine; do
    if ! cargo cyclonedx --manifest-path "$ROOT/src/$crate/Cargo.toml" --format json --all \
      --override-filename "$crate" --quiet; then STATUS=1; fi
    mv "$ROOT/src/$crate/$crate.json" "$ROOT/artifacts/sbom/$crate.cdx.json" 2>/dev/null || STATUS=1
  done
  # Never leave generated BOMs inside the source tree.
  rm -f "$ROOT"/src/*/ursine*.json
}

case "$GROUP" in
rust) rust_group ;;
repo) repo_group ;;
sast) sast_group ;;
deps) deps_group ;;
duplicates) duplicates_group ;;
sbom) sbom_group ;;
all)
  rust_group
  repo_group
  sast_group
  deps_group
  duplicates_group
  sbom_group
  ;;
*)
  echo "unknown group: $GROUP" >&2
  exit 2
  ;;
esac

printf '\n==> static-check %s: %s\n' "$GROUP" "$([ "$STATUS" -eq 0 ] && echo PASS || echo FAIL)"
exit "$STATUS"
