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
| `set_bluetooth_pairing_pin` | BlueZ `org.bluez.Agent1` on the system bus (zbus) + `AgentManager1.RegisterAgent` |

- Capability: `bluez` (bluetoothctl + system D-Bus).
- Module switch: `bluetooth`.
- MAC address validation.
- Scan bounded; all commands have timeouts.
- Pairing without a configured PIN keeps the `NoInputNoOutput` agent; with
  a PIN set, a registered `org.bluez.Agent1` answers `RequestPinCode`,
  `RequestPasskey` and `RequestConfirmation` (numeric comparison accepts
  when no PIN is set or the passkey matches, otherwise denies). The PIN
  lives in daemon RAM only and is never logged.

## GUI

- Sidebar **Bluetooth**.
- Adapter address/name/power.
- Scan, device list, pair/connect/trust/untrust/forget.
- Pairing PIN section: password input, set/clear, status flag only
  (the value never round-trips to any frontend).
- Busy/error states.
