# SPEC-bluetooth

## Objective

Basic BlueZ setup: adapter power, scan, known devices and pair/connect/trust/
forget actions.

## API surface

| Action | Backend |
|---|---|
| `get_bluetooth` | `bluetoothctl show`, `devices`, `info` |
| `set_bluetooth_power` | `bluetoothctl power on/off` |
| `scan_bluetooth` | `bluetoothctl --timeout 8 scan on` |
| `bluetooth_action` | pair/connect/disconnect/trust/untrust/remove |

- Capability: `bluez` (bluetoothctl + system D-Bus).
- Module switch: `bluetooth`.
- MAC address validation.
- Scan bounded; all commands have timeouts.
- Pairing uses `NoInputNoOutput` agent; complex PIN/passkey flows are out of
  scope for this version.

## GUI

- Sidebar **Bluetooth**.
- Adapter address/name/power.
- Scan, device list, pair/connect/trust/untrust/forget.
- Busy/error states.
