# SPEC-apparmor

## Objective

Module **`apparmor`** (user decision 2026-11-10): AppArmor management
through its CLI surface, no D-Bus (invariant 3). At minimum every headless
AppArmor CLI command; interactive learning tools are excluded. Reference:
Alpine/FeralOS packages `apparmor` (parser) + `apparmor-utils` (`aa-*`),
`apparmor-openrc` for the boot load (service control stays in the Services
module — no duplicate logic).

## API surface

| Action | Backend |
|---|---|
| `apparmor_status` | `aa-status` (default verbose) through the tolerant runner — exit 1/2/3 are DATA (not enabled / no policy / securityfs missing), 4/42 are errors; text format parsed (stable since 2.x) |
| `apparmor_set_mode {path, mode}` | `enforce`→`aa-enforce`, `complain`→`aa-complain`, `disable`→`aa-disable`, `enable`→remove `/etc/apparmor.d/disable/<basename>` then `apparmor_parser -r -W <path>` |
| `apparmor_load {path}` | `apparmor_parser -r -W <path>` |
| `apparmor_check {path}` | `apparmor_parser -d <path>` (syntax only, never loads); returns `{ok, output}` so the parser's own error text is shown |
| `apparmor_remove {path}` | `apparmor_parser -R <path>` |
| `apparmor_reload_all` | `apparmor_parser -r -W /etc/apparmor.d` |
| `apparmor_unconfined` | `aa-unconfined` (raw output) |
| `apparmor_remove_unknown` | `aa-remove-unknown` (destructive → `--yes`) |
| `apparmor_profile_get {path}` | raw file content |
| `apparmor_profile_set {path, content}` | path validation → **`apparmor_parser -d` on stdin of the NEW content** (invalid refused before any write) → `backup_configs` rotation → atomic tmp+rename 0644 |
| `apparmor_notifications_enable` | surgical edit of `show_notifications="yes"` in `/etc/apparmor/notify.conf` (backup first) + write `~/.config/autostart/aa-notify.desktop` (`Exec=aa-notify -p`) owned by the session user; effective from next login (user decision: no same-session spawn) |
| `apparmor_cleanprof {name}` | `aa-cleanprof <name>` — rewrites the profile file (drops comments, reorders) → whole `/etc/apparmor.d` rotated first; destructive (`--yes`) |
| `apparmor_autodep {path}` | `aa-autodep <path>` (adds hardlink deps to the file → rotated first) |
| `apparmor_features_abi` | `aa-features-abi --extract` (read-only) |
| `apparmor_decode {hex}` | `aa-decode <hex>` (argv form; stdin multi-line decoding is out of scope — the shared runners reject newlines) |

All profile paths: canonicalized, must live under `/etc/apparmor.d`, must
be regular files; `..` and symlink escapes rejected.

Excluded with rationale: `aa-genprof`/`aa-logprof`/`aa-mergeprof`
(interactive learning loops), `aa-exec` (executes programs), `aa-notify`
as a reader (exposed instead as the activation button), `aa-audit` (not
shipped by Alpine), `aa-easyprof` (policy GENERATOR — deferred with the
profile-creation scope; pending verification of its default write target).

## Capability

- **`AppArmor`**: probes `aa-status` AND `apparmor_parser` (candidate
  absolute paths), fix `apk add apparmor apparmor-utils`. Kernel disabled
  is DATA in the status view with the exact hint
  (`apparmor=1 security=apparmor` on the kernel cmdline).
- `aa-notify` presence is checked by the notifications action only
  (missing → `Unavailable` with the exact install fix).

## Notifications (one button)

- Writes `show_notifications="yes"` into `/etc/apparmor/notify.conf`
  (line-surgical, comments preserved, `backup_configs` first) and installs
  the autostart entry for the session user's home (owner = that user).
- The poller runs inside the desktop session (session bus) — the root
  daemon cannot deliver to it; activation is from the NEXT LOGIN (user
  decision 2026-11-10). Status (`configured`, `autostart`, binary
  present) rides in every view.

## GUI

- Sidebar **AppArmor** (capability-gated; Home fix row when missing).
- Status card: enabled / mounted / profile counts / process counts /
  notifications state + **Enable notifications** (confirm names the
  next-login activation).
- File table (`/etc/apparmor.d`) with loaded mode cross-reference and
  `disabled` flag; selected-file action bar: Enforce, Complain, Disable
  (confirm), Enable (when disabled), Check, Apply/Load (confirm),
  Cleanprof (confirm + `--yes` semantics), Autodep (confirm).
- Editor for the selected file (crontab-style text editor): **Save is
  behind confirmation** and the daemon syntax-checks the new content
  before writing; applying stays a separate **Apply** button.
- Global: Reload all (confirm), Remove unknown (confirm), Unconfined and
  Features-ABI report buttons, hex **Decode** input; one terminal-style
  output box for check/report results.

## CLI (parity, same task)

- `ursinectl apparmor <action> key=value …` for every action;
  `--yes` required for `set_mode mode=disable`, `remove_unknown`,
  `cleanprof` (destructive classification in `is_destructive`).
- `docs/guides/ursinectl.md` + `packaging/man/ursinectl.1` updated in
  the same task.

## Testing

- Unit: `aa-status` text fixtures (loaded / not loaded / not mounted /
  enforce+complain lists with pids), path validation (outside
  `/etc/apparmor.d`, `..`, symlink escape, non-file), mode→argv mapping,
  enable = symlink-removal + parser compose, notify.conf surgical edit
  (comments preserved), autostart file content, check-result mapping.
- Samples: every action `Dispatch::Gate` in CI (no aa-status in
  ursine-ci); destructive flags drift-checked against `is_destructive`.
- VM smoke: status on a real system; complain→enforce round-trip visible
  in `aa-status`; broken profile rejected by Check before Load;
  disable/enable cycle; Save refuses bad syntax with the parser's error;
  notifications button → config + autostart present, poller after login;
  reload_all output; remove_unknown behind `--yes`.

## Deferred (explicit)

- `aa-easyprof` (policy generator, not a report — needs verification of
  its default write target; belongs to profile creation).
- Profile CREATION from scratch (editor edits existing files only).
- Same-session notification poller spawn (next login only, per user).
