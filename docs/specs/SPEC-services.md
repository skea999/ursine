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
- Unit lookup for dinit `enable`/`disable` searches every directory dinit
  loads units from — `/etc/dinit.d`, `/lib/dinit.d`, `/usr/lib/dinit.d`,
  `/usr/local/lib/dinit.d` (constant `DINIT_SERVICE_DIRS`). A unit is also
  accepted when `dinitctl list` reports it as loaded, so a unit kept in an
  admin-added service directory never answers `not_found` for
  `enable`/`disable` while `start`/`stop` work.
- Enable/disable is idempotent. Dinit enable/disable checks `boot.d` first; a
  name with neither a unit file nor a loaded service answers `not_found`.
- Enable/disable under dinit changes the running state as well, because
  `dinitctl enable` starts a stopped service and `dinitctl disable` leaves a
  running one running. The three contract results:
  - run `enable` on a stopped, non-enabled unit → the boot link is created and
    the service starts;
  - run `enable` on a unit whose boot link exists → success, no backend call,
    running state untouched;
  - run `disable` on a running, enabled unit → the boot link is removed and
    the service keeps running.
  The answer carries the service list re-read after the call: callers read
  `state` from it instead of assuming the operation left it alone. `OpenRC`
  (`rc-update`) never starts or stops anything.
- No shell; no command string interpolation.

## GUI

- Sidebar **Services**.
- State and enabled badge.
- Start/stop/restart and enable/disable.
- Confirmation before lifecycle changes.
- Busy/error states.
- Heading identifies the active backend (`OpenRC services` or `dinit services`).
