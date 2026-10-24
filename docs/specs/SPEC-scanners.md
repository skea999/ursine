# SPEC-scanners

## Objective

One-page scans through SANE, saved as PNG in the target user's Pictures
directory. Devices are discovered and scans run as that user, not as root.

## API surface

| Action | Backend |
|---|---|
| `list_scanners` | `scanimage -L` as the target user |
| `scan_document` | `scanimage --device-name ... --resolution ... --mode ... --source ... --format=png --output-file ...` |

- Capability: `sane` (`scanimage`).
- Module switch: `scanners`.
- Resolution: 75, 100, 150, 200, 300, 600.
- Mode: Color, Gray, Lineart.
- Source validated (`Flatbed`, `ADF`, etc.).
- Output: `~/Pictures/ursine-scan-<timestamp>.png`.
- `mkdir -p` runs as the target user.

## GUI

- Sidebar entry **Scanners**.
- Device dropdown.
- Resolution and mode dropdowns.
- Source text field.
- Scan button, result path, busy/error states.

## Safety

- No root scan process; `su` as the authenticated user.
- Absolute `scanimage`/`mkdir` paths and argv only.
- Bounded options; no shell.
