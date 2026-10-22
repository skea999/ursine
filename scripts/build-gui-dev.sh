#!/bin/sh
# Build the current GUI + daemon in the dev container and stage a local Nix
# flake under artifacts/ursine-dev/. Re-run after every code change.
#
#   scripts/build-gui-dev.sh
#   nix run path:$PWD/artifacts/ursine-dev#ursine
#   nix profile add path:$PWD/artifacts/ursine-dev#default
#
# The binaries are dynamic musl (Alpine/FeralOS); they use the host's runtime
# libraries (Wayland, Vulkan/GL, xkbcommon, fontconfig).
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
out="$root/artifacts/ursine-dev"
image=${URSINE_CI_IMAGE:-ursine-ci:1}

runtime=${CONTAINER_RUNTIME:-}
if [ -z "$runtime" ]; then
  if command -v podman >/dev/null 2>&1; then
    runtime=podman
  elif command -v docker >/dev/null 2>&1; then
    runtime=docker
  else
    echo "error: podman or docker is required" >&2
    exit 1
  fi
fi

mkdir -p "$out/bin"

"$runtime" run --rm --network=host \
  -v "$root:/work:ro" -v "$root/artifacts:/work/artifacts:rw" -w /work \
  -e CARGO_HOME=/work/artifacts/cargo-home \
  -e CARGO_TERM_COLOR=never \
  "$image" \
  cargo build --release -p ursined -p ursine

cp "$root/artifacts/target/release/ursined" "$out/bin/ursined"
cp "$root/artifacts/target/release/ursine" "$out/bin/ursine"
cp "$root/src/tools/ursine-activate.sh" "$out/bin/ursine-activate"
cp "$root/src/tools/ursine-deactivate.sh" "$out/bin/ursine-deactivate"
chmod 755 "$out/bin/ursined" "$out/bin/ursine" "$out/bin/ursine-activate" "$out/bin/ursine-deactivate"

rm -rf "$out/packaging"
cp -R "$root/packaging" "$out/packaging"
cp "$root/install.sh" "$out/install.sh"
chmod 755 "$out/install.sh"

cp "$root/nix/dev-flake.nix" "$out/flake.nix"
cp "$root/flake.lock" "$out/flake.lock"

echo "staged:  $out"
echo "run:     nix run path:$out#ursine"
echo "install: nix profile add path:$out#default"
