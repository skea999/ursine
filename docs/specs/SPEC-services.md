# SPEC-services

## Objective

Manage init services from the GUI. Supports the running init: OpenRC and dinit.
The backend chooses by capability, not by the presence of tooling: on FeralOS
both OpenRC and dinit packages can be installed, but PID 1 is dinit.

## API surface

| Action | OpenRC backend | dinit backend |
|---|---|---|
| `list_services` | `rc-status --servicelist` + `rc-update show` | `dinitctl list` + `boot.d` links |
| `service_action` | `rc-service start/stop/restart`; `rc-update add/del` | `dinitctl start/stop/restart`; `dinitctl enable/disable` |

- Capability: `openrc` **or** `dinit`; the services module switch applies to both.
- Name: `[A-Za-z0-9_.-]`.
- Root commands; absolute paths; argv only.
- States: OpenRC `started`, `stopped`, `crashed`; dinit `started`, `stopped`,
  `failed`, `unknown`.
- Enabled: OpenRC from `rc-update show`; dinit from `boot.d/<service>` links in
  `/etc/dinit.d`, `/lib/dinit.d`, `/usr/lib/dinit.d`, or
  `/usr/local/lib/dinit.d`.
- No shell; no command string interpolation.

## State semantics

- `enable`/`disable` are idempotent: when the state already holds the action
  answers success with the refreshed list, the same answer as a change that
  was just applied. Locally readable state (dinit `boot.d` links) short-circuits
  the call; for OpenRC the tool's own refusal is classified.
- A refusal that describes the wanted state is not an error: `rc-update: service
  'bluetooth' is not in the runlevel 'default'` (disable), `... is already in
  the runlevel 'default'` (enable), `dinitctl: ... already enabled` / `... is
  not enabled`. The init's stderr is classified by a pure helper
  (`classify_openrc`, `classify_dinit`); nothing parses the service list twice.
- A name the init does not know is `not_found` for every operation, `enable`
  and `disable` included — the local state test alone cannot tell "no such
  service" from "not enabled". OpenRC asks `rc-service --exists`; dinit looks
  for a description file of that name in `/etc/dinit.d`, `/usr/lib/dinit.d`,
  `/lib/dinit.d` or `/usr/local/lib/dinit.d`.

## Errors

| Situation | Code | Message / fix |
|---|---|---|
| no `openrc` and no `dinit` capability | `unavailable` | reason "no supported init service manager", fix "boot OpenRC or dinit" |
| name rejected by the character/length rule | `invalid` | "invalid service name" |
| no service of that name | `not_found` | "<init> has no service named <name>", fix "check the service name; list services" |
| `enable`/`disable` where the state already holds | success | list refreshes, no change |
| the init tool rejects a valid name for another reason | `internal` | "command failed: <tool stderr>" |

## GUI

- Sidebar **Services**.
- State and enabled badge.
- Start/stop/restart and enable/disable.
- Confirmation before lifecycle changes.
- Busy/error states.
- Heading identifies the active backend (`OpenRC services` or `dinit services`).
