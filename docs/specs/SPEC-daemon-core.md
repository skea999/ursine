# SPEC-daemon-core (ursined)

## Objective

Root daemon `ursined`: the single process owning all logic. Accepts frontend connections on a unix socket, authenticates principals via a one-time shadow handshake (wheel-gated), issues RAM-only session tokens, rate-limits logins, dispatches authenticated requests to modules, and writes a JSONL audit line for every action. Stateless: survives restart with zero reconciliation.

## Tech Stack

- Rust stable, edition 2021, no unsafe
- `tokio` (net, signal), `nix` crate (unix socket peer creds, umask), `serde_json`
- Shadow auth (Webmin pattern): read `/etc/shadow` in-process, verify with `pwhash` (pure Rust, the same crate that writes `$6$` hashes); fail-closed on locked/expired/unsupported accounts. No cargo features, no C dependencies — daemon and CLI link fully static
- `tracing` + `tracing-subscriber` for logs (stderr), JSON lines for audit (separate file)

## Commands

```text
cargo build  -p ursined                # static (no C deps)
cargo test   -p ursined
cargo clippy -p ursined -- -D warnings
cargo run -p ursined -- --config /etc/ursine/config.toml
```

## Project Structure

```text
src/ursined/
├─ Cargo.toml
└─ src/
   └─ main.rs           arg parsing, config load, serve loop
src/ursine-core/src/
   ├─ lib.rs
   ├─ config.rs         load/validate/write /etc/ursine/config.toml (TriState)
   ├─ auth/
   │  ├─ mod.rs         Authenticator trait (issue, verify) — the ONLY seam
   │  └─ shadow.rs      /etc/shadow verify (pwhash, Webmin pattern) + tests
   ├─ session.rs        token store in RAM: HashMap, TTL 10 min idle (config), sliding expiry, per-session scope (read-only + client id)
   ├─ ratelimit.rs      failed-login rate limit in RAM (default 5/min per uid, config)
   ├─ socket.rs         /run/ursine.sock root:root 0666, peer creds (SO_PEERCRED)
   ├─ dispatch.rs       Request -> module router; enforces auth, asUser resolution
   ├─ audit.rs          JSONL append: ts, principal, action, asUser, on_behalf_of, result; fsync per line
   └─ modules/          one dir per module (see module specs)
tests/                  integration tests (socket loop, auth flows)
artifacts/              generated + build output (target-dir, gitignored contents)
```

## Code Style

```rust
#[async_trait::async_trait]
pub trait Authenticator: Send + Sync {
    async fn authenticate(&self, peer: PeerCreds, user: &str, password: &str) -> Result<Principal, AuthError>;
}
```

- One trait per boundary (authenticator, module); tokio everywhere, no std blocking on the runtime
- Errors: `thiserror`, mapped to `ursine_api::Error` at dispatch edge; never panic on request path

## Authentication flow (binding)

1. Connect to `/run/ursine.sock` (root:root, 0666 — any local user may connect, like a Webmin login page)
2. Client sends `Auth { user, password, read_only?, client? }`
3. Daemon: rate-limit check → `client` shape check (1–64 bytes of
   `[A-Za-z0-9_.-]`, `invalid` + `fix` otherwise) → shadow verify
   (`/etc/shadow`, fail-closed + timing-equalized) → wheel group check →
   issue 256-bit random token, RAM only, carrying the requested scope
4. Every subsequent `Call` carries `Authorization: Bearer <token>`; daemon validates + slides TTL (default 10 min idle, `token_ttl_min` in config)
5. Auth failure → generic error (no user-exists oracle), audit line, rate-limit tick
6. A read-only session (`read_only: true`) passes dispatch's token check and
   then refuses every mutating action (`Action::is_mutating()`) with code
   `forbidden` and a `fix` naming the scope. The refusal is audited like any
   other failed action. Reads, `dry_run` previews and a read-write session are
   untouched.

## Audit line (binding)

One JSON object per line, fsynced before the next entry: `ts_millis`,
`principal`, `origin`, `module` (omitted for the handshake), `action`,
`as_user` (omitted when the request carried no honored target), `outcome`
(`{"result":"ok"}`, or `{"result":"err","code":…,"message":…}`).

`on_behalf_of` carries the session's `client` id and is omitted when the
session named none: it answers "which program acted", the question a review
of an admin action asks right after "who". A job completion line has no
session behind it (the job outlives the request that started it), so it
carries the principal and no `on_behalf_of`.

## Update-check preference (binding)

`[updates]` in `/etc/ursine/config.toml` — the per-manager GUI preference
"Check updates automatically" (persisted through `config.set_update_auto
manager=… enabled=…`, surgical TOML edit with `backup_configs` rotation):

- `apk = true`, `flatpak = true` — fast checks run on screen open and on
  Refresh.
- `nix = false` — the nix check compares each profile element's pinned
  lock with the current `nix flake metadata` lock of its flake reference
  (network): with the preference off it runs only when the user presses
  Refresh ("Check for updates"); with it on it runs on open exactly like
  the others.
- With the preference off, Refresh is the manual path that still fetches
  the count; until a check runs the GUI shows "—", never a fake zero.

## File backups (binding)

Every configuration file Ursine writes — directly or by invoking a CLI —
keeps its previous versions through ONE uniform rotation mechanism
(`ursine_core::backup_configs`):

- Slots: `<file>.ursine-backup` (newest), then `<file>.ursine-backup.1`
  … `<file>.ursine-backup.(copies-1)` (oldest). Rotation runs BEFORE the
  live file is replaced; a rotation failure aborts the write (shell
  scripts exit 42) so the live file stays untouched.
- Count: `[backup_configs] copies = N` in `/etc/ursine/config.toml`
  (default 3, clamped 1..=10). The config file itself is governed by
  the value it contains; editing it requires a daemon restart (same as
  `disks_format`).
- Direct writes: `/etc/ursine/config.toml`, niri config, APK
  repositories, all theming targets (incl. COSMIC
  entries and the dconf database before `gsettings`), `airscan.conf`,
  `btrbk.conf`.
- CLI-changed files (rotated before the CLI runs): crontab (previous
  content archived under `/var/backups/ursine/crontab.<user>`, mode
  0600), the nix flake registry JSON, Flatpak installation
  `repo/config`, `/etc/apk/world` (on `apk add`/`del`),
  `/etc/firewalld`, `/etc/NetworkManager/system-connections`,
  `/etc/cups/printers.conf`, and the account files
  (`/etc/passwd`, `/etc/shadow`, `/etc/group`, `/etc/gshadow`).
- Snapshot restores rotate the live tree into slot 0 (moved, not
  copied); legacy timestamped backups from older releases are pruned
  to the same cap.
- Excluded with rationale: nix profile generations (nix's own
  rollback), Flatpak/apk package databases (transactional by design),
  systemd enable symlinks (reversible by re-running enable/disable),
  audit JSONL (append-only log).

## Testing Strategy

- Unit: session TTL expiry (fake clock), rate limiter, config validation, asUser resolution rules
- Integration (unix, dev): socket loop with a test authenticator — auth OK, auth fail, expired token, unknown module
- Shadow path: unit-tested against fixture shadow files (valid/wrong/locked/expired/unsupported format); one test job runs the whole workspace suite via `scripts/test-gate.sh`

## Boundaries

- Always: audit line for every action (success AND failure); token in RAM only; wheel check before token issue
- Ask first: changing socket path/permissions; changing wire auth flow; adding a new Authenticator impl (that's the OIDC seam — v3)
- Never: tokens on disk or in logs; passwords in logs or audit; `unsafe`; a second binary with logic; direct module execution without `ensure_available()`

## Success Criteria

- Dev build runs on this machine, accepts connections, auth via shadow, dispatch returns `NotImplemented` for all modules
- Audit JSONL written and parseable; restart loses tokens only (clients must re-auth — by design)
- CI green: single test gate (clippy lints + full suite, one pass), musl target

## Open Questions

- None blocking.
