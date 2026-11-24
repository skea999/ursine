# SPEC-packages-nix

## Objective

The flagship: GUI + API for Nix user profiles — install, remove, search, list, upgrade — per-user via `asUser`. No GUI for Nix exists on any distro; this module is why Ursine exists. The daemon executes `nix profile ...` AS the target user (never as root) against that user's profile.

## Tech Stack

- Commands executed as target user: `su -l <user> -c "nix profile <subcmd>"` (login shell: correct HOME, NIX_LINK, XDG paths) or `runuser` equivalent; exact exec mechanism decided at implementation (su -l baseline)
- `nix` multi-user (daemon mode) already installed by FeralOS; builds run as nixbld users — we never build as root

## API surface

**Only the NEW nix CLI is supported** (`nix profile ...`, flake refs). Classic
channels / `nix-env` are out of scope by decision (skea999).

The `Action` column is the wire tag: every nix action names its module, like
`install_apk` and `install_flatpak` do, so an audit line or a client log is
unambiguous without the module next to it. The bare tags of earlier versions
(`list_installed`, `search`, `install`, `remove`, `remove_all`, `remove_regex`,
`upgrade`, `upgrade_regex`, `generations`, `rollback`, `wipe_history`,
`diff_closures`, `store_gc`, `prune_generations`) stay accepted as serde
aliases: an existing client or script keeps deserializing to the same variant.

| Action | Executed as | Command shape |
|---|---|---|
| `list_installed_nix` | asUser | `nix profile list --json` → `InstalledPackage[]` + best-effort `meta.description` (one `nix eval --impure --json --expr` per unique flake reference, validated names/attr segments only; eval failure/timeout leaves descriptions absent, never fails the list) |
| `install_nix` | asUser | `nix profile add <nixpkgs#pkg\|flake#attr>` (bare name → `nixpkgs#name`) |
| `remove_nix` | asUser | `nix profile remove <element>` |
| `remove_all_nix` | asUser | `nix profile remove --all` |
| `remove_regex_nix` | asUser | `nix profile remove --regex <pattern>` |
| `upgrade_nix` | asUser | `nix profile upgrade <element>` / `--all`. `all` is optional on the wire and defaults to `false` (upgrade the named package); a request that names neither a package nor `all=true` answers `invalid`. |
| `upgrade_regex_nix` | asUser | `nix profile upgrade --regex <pattern>` |
| `search_nix` | asUser | `nix search <repo> <query> --json` → `SearchResult[]` (repo defaults to `nixpkgs`; custom repos prefix bare installables). Synchronous, no job: a cold Nix cache makes the first call take minutes, timeout 300s (see *Search cost*) |
| `list_nix_repositories` | asUser | `nix registry list` + `nix-channel --list` → `NixRepository[]` for the selected scope (`system` or the target user). Only configured entries: builtin `global` defaults are never returned (no file to edit, no way to restore them). A channel fills a name the registry does not define. |
| `add_nix_repository` | asUser | user scope: `nix registry add <name> <flake-ref>`; system scope: merge an `indirect` entry into `/etc/nix/registry.json` (`github:owner/repo[/ref]` only), backup + atomic write |
| `remove_nix_repository` | asUser | destructive: `source=registry` → `nix registry remove` (user) or the system-file edit; `source=channel` → `nix-channel --remove` |
| `list_nix_updates` | asUser | `nix profile list --json` + the current lock of every unique reference, fetched in parallel (bounded 4). `flake:` aliases resolve through the registry; a target that parses as `github:owner/repo[/ref]` and whose elements all carry a pinned rev uses `git ls-remote` (optional `git`, ~0.5 s, no fetch/eval); everything else uses `nix flake metadata <url> --json`. An element is outdated when a comparable lock differs (rev first, else narHash) — same proof rule as before, verified on nix 2.31.5 (no `profile upgrade --dry-run`) → `NixUpdates {count, packages}`; read-only, on demand |
| `generations_nix` | asUser | `nix profile history` → lines (returned NEWEST first) |
| `rollback_nix` | asUser | `nix profile rollback [--to N]` |
| `wipe_history_nix` | asUser | `nix profile wipe-history [--older-than D] [--dry-run]` |
| `diff_closures_nix` | asUser | plain `nix profile diff-closures` → lines. `--to` is not passed: that flag belongs to `rollback` and nix rejects it here. With `generation=N` only the block whose header ends at N (`Version M -> N:`) is returned, because nix prints one block per generation pair; a generation that is no step in the history is an `invalid` error, not an empty diff |
| `store_gc_nix` | **root** | `nix store gc` — total GC (unreachable paths only), timeout 3600s |
| `prune_generations_nix` | **root** (exec) | plan from `nix profile history` (ANSI stripped; current generation read from `profile-N-link` symlink — nix 2.31 does not mark it); `dry_run` returns `current` + `would_delete`; exec `nix-env -p <profile> --delete-generations +N` (root: users cannot lock the store profile in multi-user mode). Both reads of the profile link share one helper: a missing link (or one that vanished between the two reads) answers the empty plan, never `Invalid` |

Execution rules (binding): `su -l -s /bin/sh <user> -c 'nix ...'` (login env:
HOME/PATH/XDG for the user's own profile); every argument validated against a
strict charset and single-quoted (no shell injection); target user must exist
and have uid in `1000..60000` (system users/root rejected); path installs
(`./flake`, `/path`) rejected in v1; timeouts: list/history 60s, search 300s,
mutations 900s, store gc 3600s, child killed on timeout.

Search cost (binding): `search_nix` stays a synchronous read — it answers one
`SearchResult[]` page, streams no progress and returns no job id. The first
search of a machine whose user cache (`~/.cache/nix`) is cold makes `nix search`
download the flake source and evaluate every attribute of it: 135s measured for
the query `firefox` on a slow VM without AVX, with ~103 MiB of cache written
and 29 results returned. A warm search of the same query answers in seconds
because that cache is reused. Clients therefore size their deadline from the
daemon's budget, not from the warm-search latency: `timeouts::SEARCH_SECS`
(search 300s) in the daemon, plus the write and reply margins in the client.
When the budget runs out the daemon kills the child's whole process group and
answers `internal` `command timed out after 300s`; a retry is fast because the
cache is now warm. Turning the search into a pollable job is NOT part of this
version: it changes the protocol shape and remains an owner decision.

GC notes: `storeGc` runs as root and only removes paths unreachable from any
GC root (profile generations are roots — that is why prune exists). The active
generation is never deleted; `pruneGenerations` computes the plan first, so the
GUI can preview it before running. Verified against nix 2.31.5: `history` emits
ANSI escapes and has no `(current)` marker; `--delete-generations +N` needs root;
`diff-closures` accepts no `--to`, prints one `Version A -> B:` block per pair of
consecutive generations (header from `CmdProfileDiffClosures`, `profile.cc`) and
separates blocks with a blank line.

- Repositories mirror the FeralOS model: the installer adds the `nixos-26.05` channel as root and pins `nixpkgs` in `/etc/nix/registry.json` plus the install user's registry. Ursine lists exactly those configured entries per scope — `System` = system registry + root channels, `User` = the target user's — and never the builtin registry defaults (a user cannot restore one, so it is not offered for removal).
- Registry list/add/remove are user-scoped actions: `asUser` picks the registry to read or edit. A NON-root caller may target only its own account; root may retarget an existing normal-login account. Every authenticated Ursine user is in `wheel` (login gate), which is the distro's trusted-users boundary.
  Both mutations rotate the registry JSON first (uniform `.ursine-backup[.N]` slots, `backup_configs.copies`; chowned back to the user).
- `asUser` defaults to the authenticated principal; daemon verifies target user exists and is a normal user (uid >= 1000) before exec
- GUI: the `System | User` scope drives repositories; the `User` scope offers a dropdown of normal login accounts (plain input until the list loads) and the installed list names the profile owner. The `System` scope hides the user choice — the system registry belongs to root, while the profile list stays on the signed-in account because a non-root user cannot read root's profile.
- Long ops (install/upgrade) run as daemon jobs: exact output streams to `/run/ursine/jobs/<id>.log` and frontends poll `job_output` (supersedes the earlier same-connection JSON progress-event design; see Package progress)

## Commands

```text
cargo test -p ursine-core packages_nix
```

## Project Structure

```text
src/ursine-core/src/modules/packages_nix/
├─ mod.rs        action handlers
├─ exec.rs       asUser exec helper (su -l, env sanitization, timeout, output capture)
└─ parse.rs      `nix profile list --json` / `nix search` parsers
```

Progress is not parsed: mutations stream raw terminal bytes into the shared
job log (`src/ursine-core/src/jobs.rs`); the frontend polls `job_output`.

## Code Style

```rust
// Mutations return a job id; the shared registry streams exact output.
let started = ctx.jobs.start(ModuleId::PackagesNix, "install", ctx.principal, Some(ctx.as_user), move |sink| async move {
    exec::run_as_user_nix_log(&user, &borrowed, MUTATE_TIMEOUT, sink).await
})?;
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

- Search index latency: `nix search` is slow (it evaluates nixpkgs). v1 = accept
  the slowness, keep the call synchronous, and document the cost plus the 300s
  budget (see *Search cost*); the earlier "progress indicator" is not implemented
  and is not promised by the protocol. v2 options: a prebuilt search index cache
  on the machine, or a warm-up step outside the request. Both are open.

## Package progress (binding, implemented 2026-10-27)

Mutations must stream the exact process output as terminal text, not a
percentage bar. The daemon owns a RAM job registry and writes stdout/stderr to
`/run/ursine/jobs/<id>.log`; frontends poll `job_output`. A **Close** button is
shown only after the job completes. This applies to APK, Flatpak and Nix.
