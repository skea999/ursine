# Tasks — Ursine

**Milestone**: v0.2 — packages.nix (DONE) → v0.3 store GUI
**Updated**: 2026-10-19

## Current Sprint

### Todo
- [ ] v0.3 spike: libcosmic pinned rev builds on musl (add dev deps: cmake, fontconfig-dev, freetype-dev, libxkbcommon-dev, expat-dev, pkgconf) + empty window + one socket call | P0 | agent
- [ ] v0.3: store GUI (libcosmic): login, home cards, nix store view, snapshots panel | P0 | agent
- [ ] `dinit/ursined` service file + PAM service `/etc/pam.d/ursine` (auth-only, pam_unix) shipped for packaging | P1 | agent
- [ ] v0.4 users (accountsservice) + niri editor forms | P1 | agent
- [ ] v3 (deferred): choose web GUI stack — **must be memory-safe** (Rust→WASM preferred: Leptos/Yew/Dioxus; TypeScript acceptable fallback). See `docs/specs/SPEC-web.md` | P2 | skea999

### In Progress
- (empty)

### Done — v0.2 packages.nix (2026-10-19)
- [x] New-CLI-only nix module: list/add/remove/removeAll/removeRegex/upgrade/upgradeRegex/search/history/rollback/wipe-history/diff-closures + **storeGc** (root, total) + **pruneGenerations** (root exec, keep last N, dry-run with current from symlink); `su -l -s /bin/sh` as target user; strict validation; uid 1000..60000; timeouts; 78 tests green. **Verified against real nix 2.31.5** (list JSON schema, history ANSI/no-current, --regex flags, prune lock semantics) | P0

### Done — v0.1 skeleton (2026-10-19)
- [x] Cargo workspace `src/{ursine-api,ursine-core,ursined,ursine}`; template `src/` subdirs removed | P0
- [x] `ursine-api`: envelope with Bearer token, ModuleId, full Action surface, Principal/AsUser, SessionToken (redacted Debug), Capability::ALL, TriState serde, UrsineConfig | P0
- [x] `ursine-core`: config (load/render/write atomic), capability Registry + `ensure_available`, auth trait + stub + PAM (feature), TokenStore (10 min sliding TTL), RateLimiter, JSONL audit, unix socket (std peer creds, 0666), dispatch (auth → wheel check → token → modules) | P0
- [x] `ursined`: arg parsing, tracing, graceful SIGTERM/SIGINT, stub/PAM authenticator switch | P0
- [x] Tests: **55 green** (21 api unit + 6 api integration via root `tests/ursine-api/` + 28 core unit) | P0
- [x] Clippy `-D warnings` + `cargo fmt --check` green, including `--all-features` (pam) | P0
- [x] E2E smoke in container: auth → 64-char token → system.status → capabilities.list (6) → bad token rejected → audit JSONL (3 lines) → clean shutdown; binary dynamic musl (`ldd`) | P0
- [x] `flake.nix` (pkgsMusl, dynamic musl, x86_64, feature pam) + `flake.lock`; `nix flake show` evaluates green (full build runs in CI) | P0
- [x] `.github/workflows/ci.yml` ACTIVE: matrix default/pam in alpine:3.22, clippy+fmt gate, nixos/nix flake check/build, release-on-tag | P0
- [x] `ci/Containerfile.dev` dev image (rust:1-alpine + linux-pam-dev + clippy/rustfmt); all local builds containerized (podman, `--network=host`) | P0
- [x] Dynamic musl decision: `.cargo/config.toml` `-crt-static` off (static musl cannot link libpam.so; Alpine-style) | P0

### Done — planning (2026-10-16)
- [x] Naming, architecture, intent v1-v6, capability map, 9 module specs
- [x] Harness aligned (agents hidden, models removed, workflow Pattern 8, host isolation rules)

## Backlog
- packages.nix (v0.2) → apk + flatpak (v2)
- snapshots: btrbk read-write config (subvolumes + scheduler), restore flow
- web + OIDC (v3), Incus UI (v4), ZFS+Pirateus (v5), multi-distro (v6)
- APKBUILDs in feralos-aports (v1.0)

## Known Issues
| Issue | Status | Workaround |
|-------|--------|------------|
| Nix flake full build never run locally (eval only) | open | first CI run validates; CI flake job is the gate |
| PAM real auth path untested (no libpam on dev host; stub only) | open | test on FeralOS with `/etc/pam.d/ursine`; CI compiles the feature |
| `.opencode/package.json` + lock tracked while `.opencode/.gitignore` exists | resolved | ignore lines dropped; files tracked intentionally |
| Agent fixes (hidden/quote/no-model) diverge from skea-store template | open | promote upstream when skea999 decides |

---

*Update start/end of each session.*
