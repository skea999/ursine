# SPEC-packages-nix

## Objective

The flagship: GUI + API for Nix user profiles — install, remove, search, list, upgrade — per-user via `asUser`. No GUI for Nix exists on any distro; this module is why Ursine exists. The daemon executes `nix profile ...` AS the target user (never as root) against that user's profile.

## Tech Stack

- Commands executed as target user: `su -l <user> -c "nix profile <subcmd>"` (login shell: correct HOME, NIX_LINK, XDG paths) or `runuser` equivalent; exact exec mechanism decided at implementation (su -l baseline)
- `nix` multi-user (daemon mode) already installed by FeralOS; builds run as nixbld users — we never build as root

## API surface

| Action | Executed as | Command shape |
|---|---|---|
| `nix.listInstalled` | asUser | `nix profile list --profile /nix/var/nix/profiles/profile` (user default profile) — parse `--json` |
| `nix.install` | asUser | `nix profile install nixpkgs#<pkg>` |
| `nix.remove` | asUser | `nix profile remove <attr|index>` |
| `nix.upgrade` | asUser | `nix profile upgrade <attr|index>` / `--all` |
| `nix.search` | asUser | `nix search nixpkgs <query>` — parsed, cached briefly in RAM |
| `nix.generations` | asUser | `nix profile history` (read-only) |

- `asUser` defaults to the authenticated principal; daemon verifies target user exists and is a normal user (uid >= 1000) before exec
- Long ops (install/upgrade) stream progress lines over the socket (JSON progress events, same connection)

## Commands

```
cargo test -p ursine-core packages_nix
```

## Project Structure

```
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
