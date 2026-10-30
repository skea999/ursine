# SPEC — Crontab management

Status: **implemented** (user decision 2026-10-29).

## Requirement

A section to view and replace the **session user's crontab**. Must work on
every distro we support: **Alpine (busybox crond), cronie, and
Debian/Ubuntu (vixie-cron)** — the three only differ behind the `crontab`
binary, so all operations go through it:

- `crontab -l` → current crontab (absence = "no crontab" → empty editor
  with an explanatory hint, not an error);
- stage the payload in the target user's home (atomic, user-owned, 256 KiB
  cap, NUL rejected) → `crontab <file>` (the portable replace form all
  three implementations accept; empty body clears the crontab).

## Behaviour

- Capability `crontab` (binary present) gates the module; module switch in
  Settings; Overview card; sidebar entry — parity rules apply.
- `as_user`: session user only (same rule as theming — no cross-user edits
  from the wire).
- GUI: editor (text area) seeded with the CURRENT crontab on open
  (config-first), Refresh, Save behind a confirmation step, per-action
  busy/error/message states.
- Audit: `crontab_get` and `crontab_set` are both audited (mutations must
  be visible).
- System-wide files (`/etc/crontab`, `/etc/cron.d/*`) are out of scope for
  v1 (user crontab only).

## Tests

- Payload validation (cap, NUL) unit tests; view round-trip; GUI view
  states; `no crontab` mapping exercised when a `crontab` binary exists.
