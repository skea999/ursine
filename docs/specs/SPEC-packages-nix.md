# SPEC-packages-nix

## Objective

The flagship: GUI + API for Nix user profiles — install, remove, search, list, upgrade — per-user via `asUser`. No GUI for Nix exists on any distro; this module is why Ursine exists. The daemon executes `nix profile ...` AS the target user (never as root) against that user's profile.

## Tech Stack

- Commands executed as target user: `su -l <user> -c "nix profile <subcmd>"` (login shell: correct HOME, NIX_LINK, XDG paths) or `runuser` equivalent; exact exec mechanism decided at implementation (su -l baseline)
- `nix` multi-user (daemon mode) already installed by FeralOS; builds run as nixbld users — we never build as root

## API surface

**Only the NEW nix CLI is supported** (`nix profile ...`, flake refs). Classic
channels / `nix-env` are out of scope by decision (skea999).

| Action | Executed as | Command shape |
|---|---|---|
| `nix.listInstalled` | asUser | `nix profile list --json` → `InstalledPackage[]` |
| `nix.install` | asUser | `nix profile add <nixpkgs#pkg\|flake#attr>` (bare name → `nixpkgs#name`) |
| `nix.remove` | asUser | `nix profile remove <element>` |
| `nix.removeAll` | asUser | `nix profile remove --all` |
| `nix.removeRegex` | asUser | `nix profile remove --regex <pattern>` |
| `nix.upgrade` | asUser | `nix profile upgrade <element>` / `--all` |
| `nix.upgradeRegex` | asUser | `nix profile upgrade --regex <pattern>` |
| `nix.search` | asUser | `nix search nixpkgs <query> --json` → `SearchResult[]` |
| `nix.generations` | asUser | `nix profile history` → lines |
| `nix.rollback` | asUser | `nix profile rollback [--to N]` |
| `nix.wipeHistory` | asUser | `nix profile wipe-history [--older-than D] [--dry-run]` |
| `nix.diffClosures` | asUser | `nix profile diff-closures` → lines |
| `nix.storeGc` | **root** | `nix store gc` — total GC (unreachable paths only), timeout 3600s |
| `nix.pruneGenerations` | **root** (exec) | plan from `nix profile history` (ANSI stripped; current generation read from `profile-N-link` symlink — nix 2.31 does not mark it); `dry_run` returns `current` + `would_delete`; exec `nix-env -p <profile> --delete-generations +N` (root: users cannot lock the store profile in multi-user mode) |

Execution rules (binding): `su -l -s /bin/sh <user> -c 'nix ...'` (login env:
HOME/PATH/XDG for the user's own profile); every argument validated against a
strict charset and single-quoted (no shell injection); target user must exist
and have uid in `1000..60000` (system users/root rejected); path installs
(`./flake`, `/path`) rejected in v1; timeouts: list/history 60s, search 300s,
mutations 900s, store gc 3600s, child killed on timeout.

GC notes: `storeGc` runs as root and only removes paths unreachable from any
GC root (profile generations are roots — that is why prune exists). The active
generation is never deleted; `pruneGenerations` computes the plan first, so the
GUI can preview it before running. Verified against nix 2.31.5: `history` emits
ANSI escapes and has no `(current)` marker; `--delete-generations +N` needs root.

- `asUser` defaults to the authenticated principal; daemon verifies target user exists and is a normal user (uid >= 1000) before exec
- Long ops (install/upgrade) stream progress lines over the socket (JSON progress events, same connection)

## Commands

```text
cargo test -p ursine-core packages_nix
```

## Project Structure

```text
src/ursine-core/src/modules/packages_nix/
├─ mod.rs        action handlers
├─ exec.rs       asUser exec helper (su -l, env sanitization, timeout, output capture)
├─ parse.rs      `nix profile list --json` / `nix search` parsers
└─ progress.rs   subprocess stdout -> JSON progress events
```

## Code Style

```rust
let mut cmd = Exec::as_user(user, ["nix", "profile", "install", &flake_ref])?;
cmd.timeout(Duration::from_secs(600));
let events = cmd.stream(|line| progress_tx.send(parse_line(line))).await?;
```

- Environment sanitized for su: `PATH`, `HOME`, `USER`, `LOGNAME`, `NIX_PROFILE` set explicitly; daemon env never leaks into user shells
- All outputs parsed into `ursine_api` types; raw strings never cross the API boundary

## Testing Strategy

- Unit: parsers against committed fixture outputs of `nix profile list --json` and `nix search` (captured from real nix on Alpine)
- Unit: asUser exec target validation (rejects root, rejects nonexistent user)
- Integration (skipped if no `nix` on machine, `#[ignore]`-gated): real install/remove of a tiny package in a THROWAWAY test profile (never the user's)

## Boundaries

- Always: `ensure_available(nix)` first; execute as asUser (never root); audit line with principal + asUser + package
- Ask first: any command flag change (parser coupling); new action
- Never: `nix profile` as root; touching `/nix/store` directly; shell string interpolation of user input (args as argv items only); installing without explicit user request

## Success Criteria

- From GUI (or CLI client): search `hello`, install it as current user, see it in list, remove it — without ever typing a command
- Works for a second user via asUser dropdown (daemon executes in that user's profile)
- Audit log shows who installed what for whom

## Open Questions

- Search index latency: `nix search` is slow (evaluates nixpkgs). v1 = accept slowness + progress indicator; v2 option = prebuilt search index cache. Accepted?
