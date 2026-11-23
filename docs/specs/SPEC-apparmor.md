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
| `apparmor_profile_set {path, content}` | path validation → **`apparmor_parser -d` on stdin of the NEW content** (invalid refused before any write) → `backup_configs` rotation **into the mirror root** (see Backups) → atomic tmp+rename 0644 |
| `apparmor_notifications_enable` | surgical edit of `show_notifications="yes"` in `/etc/apparmor/notify.conf` (backup first) + write `~/.config/autostart/aa-notify.desktop` (`Exec=aa-notify -p`) owned by the session user; effective from next login (user decision: no same-session spawn) |
| `apparmor_cleanprof {name}` | `aa-cleanprof -s <name>` (`-s` = silent: without it the tool prompts, has no terminal behind the daemon and dies in a Python traceback) — rewrites the profile file (drops comments, reorders) → whole `/etc/apparmor.d` backed up first; destructive (`--yes`); an interactive refusal is `unavailable` with a fix, never the traceback as the message |
| `apparmor_autodep {path}` | `aa-autodep <path>` (adds hardlink deps to the file → backed up first) |
| `apparmor_features_abi` | `aa-features-abi --extract` (read-only) |
| `apparmor_decode {hex}` | `aa-decode <hex>` (argv form; stdin multi-line decoding is out of scope — the shared runners reject newlines) |
| `apparmor_easyprof {binary, template?, policy_groups?, template_vars?}` | `aa-easyprof [-t T] [-p G] [--template-var=@{NAME}=VALUE …] <binary>` (two-argv short options — optparse-portable; one argv element per declared variable) — generation ONLY: `--output-directory` is never passed, so the profile goes to stdout (man: "If unspecified, policy is sent to stdout") and NO profile file is written (the tool may drop a transient /tmp file while consulting apparmor_parser); `binary`/options go through the quoted-value guard (`*` is legal in profile paths) |
| `apparmor_easyprof_list {what}` | `aa-easyprof --list-templates` or `--list-policy-groups` (`what` ∈ templates\|policy_groups) |

All profile paths: canonicalized, must live under `/etc/apparmor.d`, must
be regular files; `..` and symlink escapes rejected.

`template_vars` (added 2026-11-24, issue #16): optional list of
`NAME=VALUE` entries (`serde(default)`, so older clients still
deserialize). Each entry becomes one `--template-var=@{NAME}=VALUE`
argv element — the only way to satisfy a policy group that writes a
variable (`opt-application` uses `@{APPNAME}`), which otherwise fails
verification. Validation: name = letters/digits/`_`, 64 chars max;
value non-empty, 512 chars max, no newline/CR/NUL; at most 32 entries.
`aa-easyprof` refusing the generated policy (`ERROR: Invalid policy`)
is `invalid` with a fix naming `template_vars` (and the variable, when
the verification message names it), never `Internal`.

## Backups

The profile directory is machine-read: every `AppArmor` tool parses every
file in `/etc/apparmor.d`, so a rotated copy parked next to a profile
defines that profile twice and breaks the load of UNRELATED profiles
(`ERROR: Conflicting profiles for /usr/bin/true defined in two files`).

The `backup_configs` slot chain for profiles therefore lives in a mirror
root, outside the read tree (user decision, issue #11):

```text
/etc/apparmor.d                       → /var/lib/ursine/backups/apparmor/etc/apparmor.d
/etc/apparmor.d/usr.bin.jq            → /var/lib/ursine/backups/apparmor/etc/apparmor.d/usr.bin.jq
slot 0 / .N                           → <mirror>.ursine-backup[.N]
```

Same slot naming, same rotation order (backup before replace, fail
closed) and the same `backup_configs.copies` cap as every other module
(`backup_configs::rotate_into`, used by one shared helper for the
single-file write and the whole-directory rotation). `/etc/apparmor.d`
never gains an `*.ursine-backup*` file again.

A pre-existing `*.ursine-backup*` entry inside `/etc/apparmor.d` (left by
v0.48.x and older) is a real conflict: every profile write answers
`conflict` with a fix that names the files and shows the `mv` command
into the mirror root. Ursine never deletes files under
`/etc/apparmor.d` on its own.

Excluded with rationale: `aa-genprof`/`aa-logprof`/`aa-mergeprof`
(interactive learning loops), `aa-exec` (executes programs), `aa-notify`
as a reader (exposed instead as the activation button), `aa-audit` (not
shipped by Alpine apparmor-utils).

`aa-easyprof` (added 2026-11-11 after verification): the man AND the upstream
`easyprof.py` confirm the default sends the generated policy to
**stdout** (`--output-directory` is opt-in; the file-write branch is
only taken when that flag is passed), so the action is
generation-to-output only — persisting the result stays inside the
deferred profile-creation scope (the editor writes existing files
only).

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
  backup mirror path mapping + stray-backup scan (`rotate_into` keeps
  the chain outside the read tree), `template_vars` → argv and its
  validation, `Invalid policy`/interactive-tool error mapping,
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

- Profile CREATION from scratch (editor edits existing files only;
  easyprof output is shown, not persisted).
- Same-session notification poller spawn (next login only, per user).
