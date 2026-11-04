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
| `set_network_ipv4` | `nmcli connection modify … ipv4.method …` + `connection up` when active |

- Capability: `network_manager` (nmcli + system D-Bus).
- Module switch: `network`.
- SSID/device validation; password only on stdin, never argv/logs.
- Connection names allow spaces (`Home Wi-Fi`); control characters,
  quotes and a leading `-` are rejected (argv has no shell).
- Root commands; absolute nmcli.

## GUI

- Sidebar **Network**.
- Wi-Fi radio toggle.
- SSID/password connect.
- Visible networks and active state.
- Devices and disconnect.
- Per-connection IPv4 editor (DHCP/Static, addresses, gateway, DNS)
  with a reconnect confirmation; inactive profiles only store settings.
- Busy/error states.
