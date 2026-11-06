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
| `set_firewall_enabled` | default-zone `--set-target=REJECT/ACCEPT` + reload |
| `create_firewall_service` | `--new-service` + `--service=NAME --add-port` + reload |
| `delete_firewall_service` | `--delete-service` + reload |

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
- Per-zone service/port lists with close actions, plus target,
  interfaces and rich rules (active-rules summary).
- Filtering toggle (confirm before disable) and a Custom services
  section (list, create with ports, delete with confirmation).
- Reload.
- Confirm destructive close? The UI currently executes close directly; remove is reversible.
- Busy/error states.

## Safety

- No raw XML editing.
- Permanent + reload for every rule change.
- Enable/disable maps to the default-zone target (`ACCEPT` = off,
  `REJECT` = on) because firewalld has no global switch; the GUI labels
  it "Filtering" and confirms before turning it off, and `ursinectl`
  treats `enabled=false` as destructive. Recorded in
  `state/session-state.json` key_decisions (TASKS v2 required the
  feature; the original SPEC deferred it to the service manager).
- Status parses `--list-all` per zone (target/services/ports/
  interfaces) plus `--list-rich-rules`; custom services are the
  definitions under `/etc/firewalld` (`--path-service` prefix).
