# SPEC-users

## Objective

Users and group management without D-Bus (invariant 3) — the Webmin
pattern: list straight from `/etc/passwd` + `/etc/group`, create/delete
through the system's own user CLI (shadow `useradd`/`userdel`, BusyBox
`adduser`/`deluser` fallback), and write passwords directly into
`/etc/shadow` (the hash we already computed — no `usermod -p`
portability traps, BusyBox would store that value as plaintext).

## Tech Stack

- No `zbus`, no accountsservice: `/etc/passwd` parsed read-only; account
  creation/deletion execs the system user CLI as root with absolute
  trusted paths and argv only.
- Group-membership tooling is selected at runtime: `usermod` (shadow)
  first, then `addgroup`/`delgroup` (Alpine/BusyBox), then `gpasswd`
  (minimal shadow). No distro is assumed at build time.
- Passwords are hashed in-process (crypt SHA-512 via `pwhash`) and
  written atomically into `/etc/shadow` (tmp + rename, original
  owner/group/mode preserved).

## API surface

| Action | Implementation |
|---|---|
| `users.list` | parse `/etc/passwd` (uid 1000..60000) + supplementary memberships from `/etc/group` |
| `users.create` | `useradd -m -d /home/<n> -c <fullname> -s /bin/sh -U <n>` (BusyBox `adduser -D -g … -s … -h …` fallback) — account is created locked; the GUI sets a real password separately |
| `users.delete` | `userdel [-r] <name>` (BusyBox `deluser [-r]` fallback), name resolved from uid via `/etc/passwd` — CONFIRMATION in GUI |
| `users.setPassword` | write the computed hash into the account's `/etc/shadow` field directly (atomic tmp + rename, owner/group/mode preserved) |
| `users.setGroups` | runtime tool: `usermod -G ...`, `addgroup`/`delgroup` diff, or `gpasswd -a/-d` (validated group names, argv items) |
| `users.groups.list` | parse `/etc/group` read-only |

- `asUser` NOT applicable: user management is root-scope by definition
- `users.list` returns supplementary memberships parsed read-only from `/etc/group`
- `users.setGroups` blocks removing the invoking principal's own `wheel` membership (lockout guard)

## Commands

```text
cargo test -p ursine-core users
```

## Project Structure

```text
src/ursine-core/src/modules/users/
├─ mod.rs        action handlers (backup account files before mutations)
├─ accounts.rs   /etc/passwd reader + useradd/userdel exec + /etc/shadow write
└─ groups.rs     /etc/group reader + usermod exec wrapper
```

## Code Style

```rust
accounts::create(&name, &fullname).await?;          // useradd / adduser
accounts::set_password(uid, &hashed)?;             // direct shadow write
groups::set_groups(&user.name, &new_groups).await?; // usermod / addgroup
```

- Passwords: received in request, hashed immediately (crypt SHA-512 via `pwhash` crate), never stored, never logged, never in audit (audit records "password set for uid X", no value)
- Before every mutation the account files (`/etc/passwd`, `/etc/shadow`, `/etc/group`, `/etc/gshadow`) rotate through the uniform `backup_configs` slots

## Testing Strategy

- Unit: `/etc/passwd` + `/etc/group` parser fixtures; shadow field
  replacement; input validation (username rules reused from FeralOS
  installer validators: `[a-z_][a-z0-9_-]*`)
- Integration: create/delete a throwaway user in a container (system
  user CLI present; no daemon bus required)

## Boundaries

- Always: `ensure_available(shadow)`; validation before exec; audit create/delete/setPassword/setGroups (without secrets); confirmation for delete
- The ONLY allowed `/etc/shadow` access is the atomic hash replacement in `set_password` (invariant 3 permits direct file edits; this is the Webmin path)
- Never: plaintext passwords beyond request lifetime; deleting uid < 1000 (system users); modifying the invoking principal's own admin group without a second confirmation; any D-Bus call

## Success Criteria

- Create a user with full name + admin toggle (wheel) from GUI; user appears in regreet at next login
- Reset a password; remove a user with home cleanup — both confirmed and audited

## Deferred

- Avatars: optional v2 candidate.
- Automatic login: out of scope (not feasible for this product).
