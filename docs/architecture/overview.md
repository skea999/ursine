# Architecture — Ursine

Decided 2026-10-16 (session with skea999). Each decision lists the rationale. Do not silently deviate.

## Components

```
ursined (root daemon, dinit service)          ← all logic, exactly once
├─ modules: packages (apk/flatpak/nix), users,
│           snapshots, niri, capabilities
├─ transport: newline-delimited JSON over unix socket (/run/ursine.sock)
└─ web: static assets served by the daemon + WebSocket for progress

ursine   (GUI, libcosmic / COSMIC toolkit — iced-based, Rust UI)
ursine-web (web frontend, v3; STACK UNDECIDED — binding constraint: memory-safe.
           Rust→WASM preferred (Leptos/Yew/Dioxus), TypeScript acceptable fallback;
           see docs/specs/SPEC-web.md)
ursine-api  (shared crate: request/response types, Capability, config model)
ursine-core (shared crate: module implementations)
```

Monorepo Rust workspace, ONE git repo. Packaging (APKBUILDs, dinit files) goes in the existing `feralos-aports` repo.

Why one repo: `ursine-api` types change atomically with daemon and GUI; one Cargo.lock, one CI; web assets are just a folder.

## Auth model (decided after several iterations)

No polkit. The polkit/JS-rule machinery was rejected as over-engineering for software where the daemon, GUI and rules are all ours. polkit stays installed on desktops for third-party tools (NetworkManager, fwupd, cups-pk-helper use it independently); Ursine does not touch it.

```
Desktop GUI: connect unix socket → prompt password ONCE per session
             → daemon verifies via PAM (service /etc/pam.d/ursine, pam_unix)
             + group check (wheel)
             → session token issued (RAM only, TTL) → Bearer token per request

Web:         login → PAM today / OIDC later → session token → Bearer
```

Rules:

- The check is enforced by the DAEMON, never the GUI (any process can open a socket).
- No token ever on disk (leaks via backups/dotfiles). Tokens live in daemon RAM, die on restart.
- No dedicated "app user" with root: the daemon is root, principals are real (socket peer uid / PAM user / OIDC sub) so the audit log records who did what.
- Rate-limit login attempts in the daemon.
- Per-action user context: requests carry optional `asUser` (defaults to authenticated principal). `apk` and flatpak `--system` ignore it (root); `flatpak --user` and `nix profile` run as that user (`su`/setuid from the root daemon) because Nix is multi-user (profiles per-user under `/nix/var/nix/profiles/per-user/$USER`) and flatpak `--user` is per-user.
- Accepted residual risk: after the handshake, a token-holding session has root power. Mitigations: short TTL, audit log every action, destructive actions require explicit confirmation in the frontend (UX, not security).

## Stateless principle: no database

The system IS the database. The daemon is stateless:

| Data | Lives in |
|------|----------|
| apk packages | apk's own db |
| flatpak | its own repo |
| nix | /nix/store + per-user profiles |
| users | shadow/accountsservice |
| snapshots | btrfs subvolumes |
| audit | JSONL append-only + logrotate (both already in FeralOS base) |
| session tokens | daemon RAM |
| rate limiting | daemon RAM |
| daemon config | /etc/ursine/config.toml |
| OIDC | none: JWKS cached in RAM |

Daemon restart mid-operation → re-read real state from the system; nothing to reconcile.

## Feature model (two separate axes)

1. Compile-time (cargo features): whether code exists in the binary — `web`, `oidc` trim binary size and RSS (~2-3 MB binary, ~3-6 MB RSS idle without TLS/JWT stack).
2. Runtime (capabilities): whether the machine can use a module. NEVER persisted (it would go stale at the first `apk add`). Detected at daemon start via cheap checks (stat binaries, /sys/fs/btrfs, PID1 = dinit); re-checked per request (`ensure_available()?` — one stat before executing); `capabilities.refresh` API exists.

Config tri-state per module: `"auto" | true | false`
- `"auto"` (default): detection decides
- `true`: force (warn if missing)
- `false`: off even if present

API `config.init`: daemon scans capabilities and writes a commented `/etc/ursine/config.toml` template only when the file is absent; existing files must be changed with module switches or edited deliberately by the admin. API `config.get_config` and `config.set_module_switch`: Settings reads module modes and persists one tri-state switch atomically/durably (temp `create_new`, fsync, parent checks). Module switches (`packages_nix`, `snapshots`, `users`, `niri`) apply live; startup-critical settings still require a daemon restart.

Capabilities API returns state + reason + fix, e.g.:
`{ "snapshots": { "available": false, "reason": "btrbk not installed", "fix": "apk add btrbk" } }`

## Platform facts (verified 2026-10-16, Alpine v3.22 main+community APKINDEX + feralos repo)

- `sudo-rs` is in FeralOS standardPackages and depends on `so:libpam.so.0` → **linux-pam is present on EVERY FeralOS install, servers included**. PAM is safe to depend on.
- polkit is installed ONLY by desktop DEs (niri/niriWayle/cosmic pull polkit-elogind); server/hypervisor (`desktopEnvironment: "none"`) has none. Hence the no-polkit design.
- PackageKit does not exist on Alpine. gnome-software-plugin-apk and -flatpak exist (apk+flatpak store possible off-the-shelf); Nix has no GUI anywhere — that gap is Ursine's flagship feature.
- NOT packaged on Alpine: snapper, timeshift, hplip (HP USB printers — network HP works via sane-airscan/eSCL). btrbk IS packaged.
- Scanning: sane-backends + sane-airscan + simple-scan — covered, no code needed.
- FeralOS already ships: niri + Wayle (bar/notifications/OSD/wallpaper), Veila lock (PAM), Raffi launcher, oo7 secrets, greetd+regreet+accountsservice, elogind/turnstile, firewalld, flatpak, nix (multi-user daemon), podman+webmin (container), dinit (develop branch).
- AppImage does not work on musl (glibc runtime) — irrelevant: we package apk.
- Rust compiles natively on musl. libcosmic build deps exist on Alpine (`cmake`, `fontconfig-dev`, `freetype-dev`, `libxkbcommon-dev`, `expat-dev`, `pkgconf`); FeralOS already builds COSMIC components for musl (feralos-aports).
- GUI toolkit decision (2026-10-19): **libcosmic** for the app — security over visual coherence with the GTK Wayle shell (no GTK/gdk-pixbuf/librsvg in the GUI process). Web GUI must also be memory-safe (stack TBD in v3).

## Build & CI (decided 2026-10-16)

**musl everywhere, including Nix.** Ursine targets FeralOS (musl). No glibc builds are supported or published.

Local builds: Docker with Alpine Linux images (dev machine needs no toolchain):

```bash
docker run --rm -it -v "$PWD:/work" -w /work rust:alpine cargo build --workspace
docker run --rm -it -v "$PWD:/work" -w /work rust:alpine sh     # interactive shell
```

`rust:alpine` is musl-based; `linux-pam-dev` can be added inside the container for `--features pam` builds.

GitHub CI (`.github/workflows/ci.yml`, to be created at v0.1) — **all jobs in Docker containers, no toolchain on the host**:

1. **test matrix, musl**: `alpine:3.22` container (FeralOS target; `apk add rust cargo git nodejs`; `linux-pam-dev` only on pam jobs); `cargo test` runs once per cargo feature combination that exists — default, `pam` (`full` when web/oidc land in v3). Clippy + fmt gate on all features. (`nodejs` in-container so JS actions work on musl.)
2. **nix flake**: `nixos/nix` **container** → `nix flake check` + `nix build --out-link artifacts/result` on every push. `flake.nix` packages the **pinned musl artifact** (`nix/binary.nix`, URL+sha256): nixpkgs does not cache musl toolchains (stable included), so from-source musl builds take hours; Alpine builds the binary, Nix publishes it reproducibly.
3. **publish**: on tag, same `nixos/nix` container builds musl binaries and CI attaches them to a GitHub Release; the flake itself is "published" by living in the repo (`github:skea999/ursine`).

## Release policy (2026-10-21)

- Default feature releases bump the minor version (`0.X.0`).
- Patch releases (`0.X.Y`) only for an explicitly requested corrective release.
- Major releases are made only when the workflow marks them explicitly.
- Every release is an annotated tag (`vX.Y.Z`); CI requires the tag to match `Cargo.toml`.

## Module inventory (v1)

| Module | Backend | Notes |
|--------|---------|-------|
| packages.apk | apk | system scope, root |
| packages.flatpak | flatpak | --user via asUser, --system root |
| packages.nix | nix | profiles per asUser; flagship (no GUI exists anywhere) |
| snapshots | btrfs/btrbk | store owns snapshots: manual + auto pre-update; settings panel dropped (single owner avoids duplication) |
| users | accountsservice D-Bus | installed already (greetd/regreet) |
| niri | config.kdl editor | validate via `niri validate` live; nothing like it exists |
| capabilities | internal | feeds GUI panel gating |

## Build order

1. Workspace + `ursine-api` (types, Capability, tri-state config, errors)
2. `ursined` skeleton: unix socket, peer creds, PAM handshake, capabilities scan, JSONL audit
3. packages module: nix first (the unique piece), then apk, flatpak
4. GUI: store + snapshots
5. users, niri modules
6. web (static) once API is stable
