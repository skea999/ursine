# SPEC-firewall

## Objective

Manage firewalld zones, services and ports from the GUI without editing
`firewalld` XML or running arbitrary shell commands.

## API surface

| Action | Backend |
|---|---|
| `get_firewall` | `firewall-cmd --state`, zones, default zone, services, ports |
| `set_default_firewall_zone` | `--set-default-zone`, permanent + reload |
| `add_firewall_service` | `--add-service`, permanent + reload |
| `remove_firewall_service` | `--remove-service`, permanent + reload |
| `add_firewall_port` | `--add-port`, permanent + reload |
| `remove_firewall_port` | `--remove-port`, permanent + reload |
| `reload_firewall` | `--reload` |

- Capability: `firewalld`.
- Module switch: `firewall`.
- Zone: `[A-Za-z0-9_-]`.
- Service: `[A-Za-z0-9_-]`.
- Port: `number/tcp|udp|sctp|dccp`, number 1..65535.
- Root command; absolute `firewall-cmd`; argv only; no shell.

## GUI

- Sidebar **Firewall**.
- Running/not-running state.
- Default-zone dropdown.
- Zone dropdown for edits.
- Add service and port.
- Per-zone service/port lists with close actions.
- Reload.
- Confirm destructive close? The UI currently executes close directly; remove is reversible.
- Busy/error states.

## Safety

- No raw XML editing.
- Permanent + reload for every rule change.
- No firewall enable/disable in this version; use the distro service manager.
