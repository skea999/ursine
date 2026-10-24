# SPEC-printers

## Objective

Manage CUPS printers and queued jobs from the GUI. The daemon runs trusted
CUPS tools as root; the GUI only renders state and sends actions.

## API surface

| Action | Backend |
|---|---|
| `list_printers` | `lpstat -p -d -l` |
| `printer_jobs` | `lpstat -o [printer]` |
| `add_printer` | `lpadmin -p <name> -E -v <uri> -m everywhere` |
| `remove_printer` | `lpadmin -x <name>` |
| `set_default_printer` | `lpadmin -d <name>` |
| `pause_printer` | `cupsdisable` / `cupsenable` |
| `cancel_printer_jobs` | `cancel -a [printer]` |

- Capability: `cups` (`lpstat` or `/run/cups/cups.sock`).
- Module switch: `printers`.
- Printer names: `[A-Za-z0-9_.-]`, no leading `-`.
- URI: `ipp://`, `ipps://`, `usb://`, `socket://`, `lpd://`.
- Description/location are bounded and control-character-free.
- Add uses driverless IPP (`-m everywhere`).

## GUI

- Sidebar entry **Printers**.
- Printer list with state/default badge.
- Set default, pause/resume, remove with confirmation.
- Add form: name, URI, description, location.
- Queued jobs table.
- Cancel all jobs with confirmation.
- Empty/busy/error states.

## Safety

- Absolute tool paths and argv only.
- No shell.
- Root actions audited through the normal module/action path.
- Destructive remove/cancel confirmed in the GUI.
