# SPEC — Crontab management

Status: **implemented** (user decision 2026-10-29).

## Requirement

A section to view and replace the **session user's crontab**. Must work on
every distro we support: **Alpine (busybox crond), cronie, and
Debian/Ubuntu (vixie-cron)** — the three only differ behind the `crontab`
binary, so all operations go through it:

- `crontab -l` → current crontab, read with a byte budget matching the set
  cap (an oversized crontab is an error, never a silent truncation);
  absence = "no crontab" → empty editor with an explanatory hint, not an
  error; a missing trailing newline is appended on save (all
  implementations accept a final newline);
- stage the payload in the target user's home (atomic, user-owned, 256 KiB
  cap, NUL rejected) → `crontab <file>` (the portable replace form all
  three implementations accept; empty body clears the crontab).

## Behaviour

- Capability `crontab` (binary present) gates the module; module switch in
  Settings; Overview card; sidebar entry — parity rules apply.
- `as_user`: a NON-root caller may target only its own account (the wire
  rule — the GUI never sends a target). **Root may target any existing
  normal-login account** (the `ursinectl` admin flow, decided 2026-11-02):
  a foreign target is validated BEFORE the capability gate and must exist
  (typed `NotFound` naming the user; system accounts rejected).
- Mechanism (user decision 2026-11-02): the daemon/CLI always run as
  root, so EVERY target uses the portable **`crontab -u <user>`** form —
  no `su` impersonation and no suid bit on the tool. The payload is
  staged as root under `/run/ursine` (per-process name) and handed to
  `crontab -u <user> <file>`. An absent crontab maps to an empty view on
  every implementation (vixie/cronie "no crontab for X"; busybox
  "can't open 'X': No such file …" — permission errors stay errors).
  The tool abstracts the spool layout: the `crontab` binary and the
  system crond always come from the same distro package (dcron, cronie,
  busybox), so writes land where that system's crond reads.
- GUI: editor (text area) seeded with the CURRENT crontab on open
  (config-first), Refresh, Save behind a confirmation step, per-action
  busy/error/message states.
- Audit: `crontab_get` and `crontab_set` are both audited (mutations must
  be visible).
- System-wide files (`/etc/crontab`, `/etc/cron.d/*`) are out of scope for
  v1 (user crontab only).

## Tests

- Payload validation (256 KiB cap, NUL, trailing-newline normalization)
  unit tests; view round-trip; GUI view states; `no crontab` mapping via
  the pure outcome mapper (covers every implementation's wording);
  oversized reads are rejected (bounded `crontab -l`), never truncated.
