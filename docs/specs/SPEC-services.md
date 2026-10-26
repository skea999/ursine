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
- Enable/disable is idempotent. Dinit enable/disable checks `boot.d` first.
- No shell; no command string interpolation.

## GUI

- Sidebar **Services**.
- State and enabled badge.
- Start/stop/restart and enable/disable.
- Confirmation before lifecycle changes.
- Busy/error states.
- Heading identifies the active backend (`OpenRC services` or `dinit services`).
