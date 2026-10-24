# SPEC-scanners

## Objective

SANE **setup**, not image acquisition. Manage the airscan backend in
`/etc/sane.d/airscan.conf` and show discovered devices.

## API surface

| Action | Backend |
|---|---|
| `list_scanners` | `scanimage -L` as the target user |
| `get_scanner_config` | read `/etc/sane.d/airscan.conf` + `scanimage -L` |
| `set_scanner_device` | add/update `[devices]` entry |
| `remove_scanner_device` | remove `[devices]` entry |
| `set_scanner_discovery` | toggle `[options] discovery = disable` |

- Capability: `sane` (`scanimage`).
- Module switch: `scanners`.
- Manual fields: name, address (`http://`/`https://`), protocol `WSD`/`eSCL`.
- Config file fixed at `/etc/sane.d/airscan.conf`.
- Backup before write; atomic replacement.
- No scan, no resolution/mode/source, no image output.

## GUI

- Sidebar **Scanners**.
- Detected devices from `scanimage -L`.
- Manual airscan devices.
- Add/update form and remove confirmation.
- Discovery toggle.
- Path and busy/error states.

## Safety

- Fixed root-owned config path; no user path.
- Reject symlinks and non-regular files.
- Validate name/address/protocol.
- No shell; no scanning command.
