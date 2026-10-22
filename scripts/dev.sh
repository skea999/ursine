#!/bin/sh
# One entry point for local build/test/static/docs. Uses the same images and
# commands as CI.
#
#   scripts/dev.sh image          build ursine-ci
#   scripts/dev.sh build          cargo build (all features)
#   scripts/dev.sh test           cargo test (all features)
#   scripts/dev.sh clippy         cargo clippy -D warnings
#   scripts/dev.sh fmt            cargo fmt --check
#   scripts/dev.sh check          fmt + clippy + test
#   scripts/dev.sh static [group] static analysis (default: all)
#   scripts/dev.sh docs           markdownlint
set -eu

ROOT=$(CDPATH='' cd -- "$(dirname "$0")/.." && pwd)
cd "$ROOT"
RUNTIME=${CONTAINER_RUNTIME:-podman}
CI_IMAGE=${URSINE_CI_IMAGE:-ursine-ci:1}
STATIC_IMAGE=${URSINE_STATIC_IMAGE:-ursine-static:1}
DOCS_IMAGE=${URSINE_DOCS_IMAGE:-ursine-docs:1}
CARGO_HOME_DIR=${CARGO_HOME:-/work/artifacts/cargo-home}

run_ci() {
  "$RUNTIME" run --rm --network=host -v "$ROOT:/work" -w /work \
    -e "CARGO_HOME=$CARGO_HOME_DIR" "$CI_IMAGE" "$@"
}

run_static() {
  "$RUNTIME" run --rm --network=host -v "$ROOT:/work" -w /work \
    -e "CARGO_HOME=$CARGO_HOME_DIR" "$STATIC_IMAGE" "$@"
}

run_docs() {
  "$RUNTIME" run --rm --network=host -v "$ROOT:/work" -w /work \
    "$DOCS_IMAGE" "$@"
}

cmd=${1:-check}
shift || true

case "$cmd" in
image)
  "$RUNTIME" build -t "$CI_IMAGE" -f ci/Containerfile.dev .
  "$RUNTIME" build -t "$STATIC_IMAGE" -f ci/Containerfile.static .
  "$RUNTIME" build -t "$DOCS_IMAGE" -f ci/Containerfile.docs .
  ;;
build)
  run_ci cargo build --workspace --all-features --locked
  ;;
test)
  run_ci cargo test --workspace --all-features --locked
  ;;
clippy)
  run_ci cargo clippy --workspace --all-targets --all-features -- -D warnings
  ;;
fmt)
  run_ci cargo fmt --check
  ;;
check)
  run_ci sh -c 'cargo fmt --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --workspace --all-features --locked'
  ;;
static)
  run_static sh scripts/static-check.sh "${1:-all}"
  ;;
docs)
  run_docs sh scripts/docs-check.sh
  ;;
*)
  echo "usage: scripts/dev.sh {image|build|test|clippy|fmt|check|static [group]|docs}" >&2
  exit 2
  ;;
esac
