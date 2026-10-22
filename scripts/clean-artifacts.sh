#!/usr/bin/env bash
# clean-artifacts.sh — keep only the LAST 3 build copies (nix out-links),
# then garbage-collect nix store paths inside a CONTAINER.
#
# ⚠️ HOST ISOLATION (binding rule): this script NEVER touches host state.
#    No nix gc, no installs, no profile changes on the host. The nix store
#    GC runs inside the nixos/nix container (read/write via bind mount),
#    WITHOUT -d (host profile generations are never deleted).
#
# Usage:
#   scripts/clean-artifacts.sh          # keep 3 nix results + gc in container
#   scripts/clean-artifacts.sh --all    # also cargo clean (in container)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ART="$ROOT/artifacts"
KEEP=3
IMG_NIX="nixos/nix"
IMG_RUST="rust:alpine"

die() {
  echo "ERROR: $1" >&2
  exit 1
}
# Container runtime: podman preferred (present on this machine), docker fallback
RUNTIME="${CONTAINER_RUNTIME:-}"
[ -n "$RUNTIME" ] || RUNTIME=$(command -v podman || command -v docker || true)
[ -n "$RUNTIME" ] || die "no container runtime (podman/docker) found — host tooling is not allowed"

# --- 1. keep newest 3 nix out-links (repo-local file ops only) ---
links=$(find "$ART" -maxdepth 1 -name 'result*' -type l 2>/dev/null | sort | head -n -"$KEEP" || true)
if [ -n "$links" ]; then
  echo "Removing old out-links (keeping last $KEEP):"
  # shellcheck disable=SC2001 # simple, readable two-space indent
  echo "$links" | sed 's/^/  /'
  # shellcheck disable=SC2086
  rm -f $links
else
  echo "No old out-links to remove."
fi

# --- 2. nix store GC — INSIDE container, never with -d (protects host profiles) ---
echo "Running nix store gc inside $IMG_NIX container (host untouched)..."
$RUNTIME run --rm -v /nix:/nix "$IMG_NIX" nix-collect-garbage

# --- 3. optional: cargo target dir — INSIDE container, root READ-ONLY, artifacts writable ---
if [ "${1:-}" = "--all" ]; then
  echo "cargo clean inside $IMG_RUST container (root ro, artifacts rw)..."
  $RUNTIME run --rm -v "$ROOT:/work:ro" -v "$ART:/work/artifacts" -w /work "$IMG_RUST" cargo clean
fi

echo "Done. (Host system was not modified.)"
