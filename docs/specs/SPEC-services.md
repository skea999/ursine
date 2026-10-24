# SPEC-services

## Objective

Manage init services from the GUI. v2 supports OpenRC; dinit remains the
FeralOS boot manager and can be added with the same API later.

## API surface

| Action | Backend |
|---|---|
| `list_services` | `rc-status --servicelist` + `rc-update show` |
| `service_action` | `rc-service start/stop/restart`; `rc-update add/del` |

- Capability: `openrc`.
- Module switch: `services`.
- Name: `[A-Za-z0-9_.-]`.
- Root commands; absolute paths; argv only.
- Status: started, stopped, crashed.
- Enable/disable uses the `default` runlevel.

## GUI

- Sidebar **Services**.
- State and enabled badge.
- Start/stop/restart and enable/disable.
- Confirmation before lifecycle changes.
- Busy/error states.
