# SPEC-daemon-core (ursined)

## Objective

Root daemon `ursined`: the single process owning all logic. Accepts frontend connections on a unix socket, authenticates principals via a one-time PAM handshake (wheel-gated), issues RAM-only session tokens, rate-limits logins, dispatches authenticated requests to modules, and writes a JSONL audit line for every action. Stateless: survives restart with zero reconciliation.

## Tech Stack

- Rust stable, edition 2021, no unsafe
- `tokio` (net, signal), `nix` crate (unix socket peer creds, umask), `serde_json`
- PAM behind cargo feature `pam` via a maintained Rust PAM crate (links system linux-pam); dev builds use a stub authenticator that accepts any user/password and logs a loud warning — stub never ships
- `tracing` + `tracing-subscriber` for logs (stderr), JSON lines for audit (separate file)

## Commands

```text
cargo build  -p ursined                # dev (stub auth)
cargo build  -p ursined --features pam # FeralOS build (needs linux-pam)
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
   │  ├─ pam.rs         feature "pam": PAM service "ursine", pam_unix
   │  └─ stub.rs        dev-only: accepts everything, WARN log per attempt
   ├─ session.rs        token store in RAM: HashMap, TTL 10 min idle (config), sliding expiry
   ├─ ratelimit.rs      failed-login rate limit in RAM (default 5/min per uid, config)
   ├─ socket.rs         /run/ursine.sock root:root 0666, peer creds (SO_PEERCRED)
   ├─ dispatch.rs       Request -> module router; enforces auth, asUser resolution
   ├─ audit.rs          JSONL append: ts, principal, action, asUser, result; fsync per line
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
2. Client sends `Auth { user, password }`
3. Daemon: rate-limit check → PAM verify (service `/etc/pam.d/ursine`) → wheel group check → issue 256-bit random token, RAM only
4. Every subsequent `Call` carries `Authorization: Bearer <token>`; daemon validates + slides TTL (default 10 min idle, `token_ttl_min` in config)
5. Auth failure → generic error (no user-exists oracle), audit line, rate-limit tick

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
- Integration (unix, dev): socket loop with stub auth — auth OK, auth fail, expired token, unknown module
- PAM path: NOT tested in dev (feature off); CI matrix runs `pam` feature compile + unit tests without real PAM calls

## Boundaries

- Always: audit line for every action (success AND failure); token in RAM only; wheel check before token issue
- Ask first: changing socket path/permissions; changing wire auth flow; adding a new Authenticator impl (that's the OIDC seam — v3)
- Never: tokens on disk or in logs; passwords in logs or audit; `unsafe`; a second binary with logic; direct module execution without `ensure_available()`

## Success Criteria

- Dev build runs on this machine, accepts connections, auth via stub, dispatch returns `NotImplemented` for all modules
- Audit JSONL written and parseable; restart loses tokens only (clients must re-auth — by design)
- CI green: default + `pam` feature matrix, musl target

## Open Questions

- None blocking (PAM crate choice finalized at implementation time from maintained options).
