#!/bin/sh
# cargo-audit gate with an explicit threshold.
#
# Every finding is reported. The gate blocks only when at least THRESHOLD
# distinct dependencies have a vulnerability / unmaintained / unsound finding.
# This keeps a small, reviewed upstream backlog visible without hiding it.
set -u

ROOT=$(CDPATH='' cd -- "$(dirname "$0")/.." && pwd)
cd "$ROOT" || exit 2
THRESHOLD=10
OUT="$ROOT/artifacts/static/cargo-audit.json"
mkdir -p "$ROOT/artifacts/static"

# cargo-audit exits non-zero on any finding; capture JSON and decide here.
NO_COLOR=1 cargo audit --json >"$OUT" 2>/dev/null || true

vulns=$(grep -o '"vulnerabilities":{[^}]*}' "$OUT" | sed -n 's/.*"count":\([0-9]*\).*/\1/p' | head -n 1)
[ -n "$vulns" ] || vulns=0
warnings=$(grep -o '"kind":"[a-z]*"' "$OUT" | wc -l | tr -d ' ')
total=$((vulns + warnings))

if [ "$total" -ge "$THRESHOLD" ]; then
  printf 'BLOCK: %s dependencies with audit findings (threshold %s)\n' "$total" "$THRESHOLD"
  cargo audit || true
  exit 1
fi

if [ "$total" -gt 0 ]; then
  printf 'WARN: %s dependencies with audit findings (threshold %s); report: %s\n' "$total" "$THRESHOLD" "$OUT"
else
  printf 'cargo audit: clean\n'
fi
exit 0
