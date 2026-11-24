# SPEC-system

## Objective

Machine power state and machine identity: reboot, poweroff, the static host
name and the system time zone. These are the actions an administrator reaches
for first, and on `FeralOS` they have distribution-specific forms the caller
must not have to know about (its `BusyBox` `reboot` and `poweroff` applets
return without doing anything while dinit is PID 1).

## API surface

| Action | Effect | Gate | Machine change |
|---|---|---|---|
| `reboot` | mutating | `--yes` always | the running init reboots the machine |
| `poweroff` | mutating | `--yes` always | the running init stops the machine |
| `set_hostname name=…` | mutating | none | `/etc/hostname` + the running kernel name |
| `set_timezone zone=…` | mutating | none | `/etc/localtime` + `/etc/timezone` |

- Module `system`; no capability switch is required, because every box has a
  name, a zone and an init.
- Wire name of the shutdown action is `poweroff` (one word, the init tool's
  spelling), not the `power_off` the `snake_case` rename would derive.

## Power actions

- Candidate programs are absolute and resolved first-match-wins, dinit first:
  `dinit-reboot`/`dinit-poweroff` (`/usr/bin`, `/usr/sbin`, `/sbin`, `/bin`),
  then `/sbin/reboot` and `/sbin/poweroff` (with the same directory list, so a
  `BusyBox` box with the applets in `/bin` is covered).
- No candidate present → `unavailable` with a `fix` naming the package that
  provides one (`apk add dinit`). A power action never answers success while
  doing nothing.
- Neither action takes arguments (`-f`/`--force` are not ours to choose): the
  argv is empty and the program name is the whole command.
- The daemon answers after the init accepted the request. The connection
  usually drops while the machine stops; that is the expected outcome, not an
  error. A returned error means the init refused.

## `set_hostname`

- Payload: `name`, an RFC 1123 label — 1 to 63 characters of `[a-z0-9-]`, no
  leading or trailing dash. Uppercase, dots, underscores and anything outside
  ASCII are refused with `invalid` and a `fix`, because the name is read back
  at every boot and ends up in the D-Bus names derived from it.
- Write order: validate, resolve `hostname` (an `unavailable` answer with the
  install hint when it is missing, before anything is written), rotate
  `/etc/hostname` into the backup chain, replace it atomically, then run
  `hostname <name>`.
- A failing `hostname` run keeps the new file (it applies at the next boot) and
  answers the tool's error with a `fix` that says so.

## `set_timezone`

- Payload: `zone`, a slash-separated name of components from `[A-Za-z0-9_+-]`.
  `.` is not in that class, which rejects `..`; a leading slash is refused, so
  the name cannot leave the tree through `join`.
- The name is then resolved against `/usr/share/zoneinfo` and must be a
  regular file. The check follows symlinks, because `US/Eastern` is a link to
  `America/New_York` inside the tree, and then requires the resolved target to
  stay under the tree, so a link pointing at `/etc/shadow` is no zone either; a
  directory (`Europe/`) and a missing name are refused with `invalid` and a
  `fix` naming `timedatectl list-timezones`.
- Write order: `/etc/localtime` is replaced atomically (a temp symlink renamed
  over the old one, so no process resolves a missing link), then
  `/etc/timezone` records the name after the rotation of its backup chain.
  `/etc/localtime` carries the rule set and is what every process reads;
  `/etc/timezone` is the name that survives a tool regenerating the link.

## Errors

- `invalid` — a name or a zone that is not shaped like one, or a zone that is
  not a regular file in the tree. Always carries a `fix`.
- `unavailable` — no power command and no `hostname` on the machine. Always
  carries the package that provides one.
- `internal` — the init refused the power request, or the `hostname` tool
  failed; the underlying text is kept.

## Tests (no host mutation)

- Power command builder against fake existence predicates: both inits
  installed (dinit wins), OpenRC only, nothing at all (`unavailable` + `fix`).
- Host name validation table (accepted labels and refused shapes).
- Zone name table (traversal and absolute paths) and zoneinfo resolution
  against a temporary tree with a zone, a directory, an in-tree symlink and a
  link that leaves the tree.
- The host name and the time zone write paths against a temporary directory:
  the previous content lands in the backup chain, the link is replaced, the
  name is recorded.

## GUI

No surface yet: the GUI does not offer power or identity actions. A reboot
button must keep the confirmation it already uses for destructive actions (the
catalogue row is `destructive: always`).
