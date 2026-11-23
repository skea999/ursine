#!/bin/sh
# Canonical static analysis for Ursine. Run inside ursine-ci:1.
#   sh scripts/static-check.sh [rust|repo|sast|deps|duplicates|sbom|all]
set -u

ROOT=$(CDPATH='' cd -- "$(dirname "$0")/.." && pwd)
cd "$ROOT" || exit 2
. scripts/run-dir.sh
GROUP=${1:-all}
STATUS=0
mkdir -p "$RUN_DIR/static" "$RUN_DIR/duplicates" "$RUN_DIR/sbom"

step() {
  printf '\n==> %s\n' "$*"
}

run() {
  step "$*"
  if ! "$@"; then STATUS=1; fi
}

have() { command -v "$1" >/dev/null 2>&1; }

# A missing tool is a gate failure, never a silent skip: the image pins every
# tool, so an absent binary means the wrong environment or a broken image.
need() {
  if ! have "$1"; then
    step "$1: MISSING — the gate cannot run; use ursine-ci:1"
    STATUS=1
    return 1
  fi
}

rust_group() {
  if ! cargo clippy --version >/dev/null 2>&1 || ! cargo fmt --version >/dev/null 2>&1; then
    step "rust lints: clippy/rustfmt missing — run in ursine-ci:1"
    STATUS=1
    return
  fi
  run cargo fmt --check
  # Lints ride the test gate (single pass) — same script as dev/CI.
  run sh scripts/test-gate.sh
  step "cargo doc"
  if ! RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps; then STATUS=1; fi
}

repo_group() {
  need typos && run typos
  if have editorconfig-checker; then
    run editorconfig-checker
  elif have ec; then
    run ec
  else
    step "editorconfig-checker: MISSING — the gate cannot run"
    STATUS=1
  fi
  set -- install.sh scripts/test-host.sh scripts/clean-artifacts.sh scripts/docs-check.sh scripts/duplicates-gate.sh scripts/dev.sh scripts/package-release.sh scripts/run-dir.sh scripts/smoke-installer.sh scripts/static-check.sh scripts/test-gate.sh scripts/verify.sh \
    src/tools/ursine-activate.sh src/tools/ursine-deactivate.sh \
    packaging/openrc/ursined packaging/dinit/ursined.wrapper
  need shellcheck && run shellcheck -x "$@"
  need shfmt && run shfmt -d "$@"
  need taplo && run taplo fmt --check Cargo.toml src/ursine-api/Cargo.toml src/ursine-core/Cargo.toml \
    src/ursine/Cargo.toml src/ursined/Cargo.toml src/ursinectl/Cargo.toml \
    rust-toolchain.toml deny.toml taplo.toml
  need actionlint && run actionlint
  if need zizmor; then
    if [ -n "${ZIZMOR_SARIF:-}" ]; then
      # One run, SARIF instead of plain text: CI uploads this file so the
      # code-scanning alerts track the current workflow instead of going stale.
      step "zizmor --format sarif .github/workflows > $ZIZMOR_SARIF"
      if ! zizmor --format sarif .github/workflows >"$ZIZMOR_SARIF"; then STATUS=1; fi
    else
      run zizmor .github/workflows
    fi
  fi
  # The only rules outside this image: they need a Nix store, so the Nix job
  # runs them in nixos/nix (ci.yml, "Nix lint and flake evaluation").
  step "nix linters (statix/deadnix/alejandra): Nix job — nixos/nix"
}

sast_group() {
  need gitleaks && run gitleaks detect --source "$ROOT" --log-opts=--all --redact --no-banner
  if need cargo-geiger; then
    step "cargo-geiger reports (compile-time unsafe_code=forbid is the gate)"
    # GUI/daemon depend on libcosmic: unsafe_code=forbid covers them at compile time.
    for crate in ursine-api ursine-core; do
      cargo geiger --manifest-path "$ROOT/src/$crate/Cargo.toml" --all-features --output-format Json \
        >"$RUN_DIR/static/geiger-$crate.json" || STATUS=1
    done
  fi
}

deps_group() {
  # Two implementations over the same advisory DB, both zero-tolerance.
  # cargo-deny is target-aware (deny.toml [graph] = musl), so its ignore list
  # is the single source of truth for upstream-pinned COSMIC/iced advisories.
  # cargo-audit is NOT target-aware: it also sees the Windows-only crates the
  # musl graph never compiles, so those two are ignored explicitly.
  run cargo deny check advisories licenses bans sources
  ignores=$(sed -n '/^ignore = \[/,/^\]/p' deny.toml | grep -oE 'RUSTSEC-[0-9]{4}-[0-9]{4}')
  set -- --deny warnings
  for id in $ignores RUSTSEC-2024-0436 RUSTSEC-2026-0441; do
    set -- "$@" --ignore "$id"
  done
  run cargo audit "$@"
  run cargo machete
}

duplicates_group() {
  run sh scripts/duplicates-gate.sh
}

sbom_group() {
  for crate in ursine-api ursine-core ursined ursine; do
    if ! cargo cyclonedx --manifest-path "$ROOT/src/$crate/Cargo.toml" --format json --all \
      --override-filename "$crate" --quiet; then STATUS=1; fi
    mv "$ROOT/src/$crate/$crate.json" "$ROOT/$RUN_DIR/sbom/$crate.cdx.json" 2>/dev/null || STATUS=1
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
