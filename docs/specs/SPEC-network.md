# SPEC-network

## Objective

NetworkManager setup: devices, Wi-Fi scan and connection using nmcli, without
placing secrets in argv.

## API surface

| Action | Backend |
|---|---|
| `get_network` | `nmcli device status`, `device wifi list`, `radio wifi` |
| `set_wifi_enabled` | `nmcli radio wifi on/off` |
| `connect_wifi` | `nmcli --ask device wifi connect`, password on stdin |
| `disconnect_network` | `nmcli device disconnect` |

- Capability: `network_manager` (nmcli + system D-Bus).
- Module switch: `network`.
- SSID/device validation; password only on stdin, never argv/logs.
- Root commands; absolute nmcli.

## GUI

- Sidebar **Network**.
- Wi-Fi radio toggle.
- SSID/password connect.
- Visible networks and active state.
- Devices and disconnect.
- Busy/error states.
