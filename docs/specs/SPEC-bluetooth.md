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

- Capability: `bluez` (`bluetoothctl` binary only — no D-Bus from Ursine).
- Module switch: `bluetooth`.
- MAC address validation.
- Scan bounded; all commands have timeouts.
- Pairing uses `bluetoothctl --agent NoInputNoOutput pair` — no custom
  agent, no PIN storage (invariant 3: Ursine never opens D-Bus;
  bluetoothctl owns its own agent).

## GUI

- Sidebar **Bluetooth**.
- Adapter address/name/power.
- Scan, device list, pair/connect/trust/untrust/forget.
- No PIN section: pairing delegates to bluetoothctl's agent
  (the value never round-trips to any frontend).
- Busy/error states.
