# SPEC-users

## Objective

Users and group management via the accountsservice D-Bus API (`org.freedesktop.Accounts`) — already present on every FeralOS install (greetd/regreet). Ursine replaces the need for gnome-control-center's user panel with a lightweight libadwaita panel backed by the same daemon API.

## Tech Stack

- `zbus` (pure Rust D-Bus, async, tokio-compatible) — no libdbus C bindings
- accountsservice covers: create/delete users, set real name, set groups via daemon+`usermod` where accountsservice lacks the call (groups: accountsservice has no full group API — group membership via `/etc/group` edit through our own validated code path is NOT allowed in v1; use `usermod` exec as root)

## API surface

| Action | Implementation |
|---|---|
| `users.list` | accountsservice ListCachedUsers + properties (uid, real name, shell, home) |
| `users.create` | accountsservice CreateUser(name, fullname, accountType, hashedPw via crypt) |
| `users.delete` | accountsservice DeleteUser(uid, removeFiles bool) — CONFIRMATION in GUI |
| `users.setPassword` | accountsservice SetPassword (hashed) |
| `users.setGroups` | `usermod -G ...` exec (validated group names, argv items) |
| `users.groups.list` | parse `/etc/group` read-only |
| `users.self` | current principal info |

- `asUser` NOT applicable: user management is root-scope by definition

## Commands

```
cargo test -p ursine-core users
```

## Project Structure

```
src/ursine-core/src/modules/users/
├─ mod.rs        action handlers
├─ accounts.rs   zbus proxy for org.freedesktop.Accounts
└─ groups.rs     /etc/group reader + usermod exec wrapper
```

## Code Style

```rust
let accounts = AccountsProxy::new(&conn).await?;
accounts.create_user(&name, &fullname, account_type, &hashed).await?;
```

- Passwords: received in request, hashed immediately (crypt SHA-512 via `pwhash` crate), never stored, never logged, never in audit (audit records "password set for uid X", no value)

## Testing Strategy

- Unit: /etc/group parser fixtures; input validation (username rules reused from FeralOS installer validators: `[a-z_][a-z0-9_-]*`)
- Integration (`#[ignore]`, needs accountsservice running): create/modify/delete a throwaway user in a container with accountsservice

## Boundaries

- Always: `ensure_available(accountsservice)`; validation before D-Bus; audit create/delete/setPassword/setGroups (without secrets); confirmation for delete
- Ask first: any direct /etc/passwd or /etc/shadow write (forbidden v1); adding user-session settings (avatars etc.)
- Never: plaintext passwords beyond request lifetime; deleting uid < 1000 (system users); modifying the invoking principal's own admin group without a second confirmation

## Success Criteria

- Create a user with full name + admin toggle (wheel) from GUI; user appears in regreet at next login
- Reset a password; remove a user with home cleanup — both confirmed and audited

## Open Questions

- Avatars and automatic login toggles: v2 or v1? (accountsservice supports both; cheap to add). Default: v2.
