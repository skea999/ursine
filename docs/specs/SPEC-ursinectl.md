# SPEC — ursinectl (root CLI)

Status: **planned** (user decision 2026-10-30). Supersedes the earlier "thin
CLI client in v3" plan.

## Requirement

A single `ursinectl` binary exposing **every** daemon action for
headless/scriptable admin, working on a live system **and inside a FeralOS
chroot install** (no dinit, no daemon, no socket, no display).

## Architecture (decision A — in-process, root-only)

`ursinectl` is a second **thin host of `ursine-core`**, not a socket client:

```text
ursinectl <module> <action> [flags]
  → refuse unless euid == 0 (HARD requirement — never without root/sudo)
  → load /etc/ursine/config.toml (missing → default, same as daemon)
  → Daemon::new_local(...)  // audit /var/log/ursine/audit.jsonl,
                            // jobs /run/ursine/jobs, caps registry, no socket
  → in-memory session token for principal root (wheel always — root is
    superuser; wheel is an authorization proxy that is meaningless for euid 0)
  → Request → Daemon::handle(req, PeerInfo { uid: 0, user: "root" })
  → print result (human table/JSON), stream job output live, exit code
```

Why: `Daemon::handle` is the single routing/gating/validation/audit path.
Reusing it in-process gives 100 % behavioural parity with the GUI for free and
makes the chroot case trivial: every path (`/etc/ursine`, `/var/log/ursine`,
`/run/ursine/jobs`) is absolute and resolved inside the chroot by the kernel.
The system stays the database; the CLI holds no state.

The daemon remains required for the GUI and (v3) web: non-root frontends need
the PAM/token privilege boundary. The CLI does not replace it and must never
call it over the socket (root has no reason to hop through a token).

## Root gate

- `geteuid() != 0` → stderr message (`ursinectl must run as root (use sudo)`)
  and exit 2, before any side effect (before config/audit open).
- No `--force-non-root`, no env override. The user asked for this to be
  absolute.

## Command surface

One clap subcommand per `Action` variant, grouped by module (mirrors the API
1:1 — new API action ⇒ new subcommand, same PR):

```text
ursinectl status
ursinectl config get | init | set-switch --module M --value auto|on|off
ursinectl capabilities list | refresh
ursinectl nix list | search Q | install P… | remove P… | upgrade … |
         generations | rollback | store-gc | prune --keep N | …
ursinectl apk list | search Q | install P… | remove P… | upgrade … | …
ursinectl flatpak list | search Q | install … | …
ursinectl snapshots list | stats | create --name … | restore … | …
ursinectl users list | create … | delete … | set-password … | groups …
ursinectl niri get | validate | set-section --file …
ursinectl printers … | scanners … | network … | firewall … | disks … |
         bluetooth … | services … | theming … | logs … | crontab …
ursinectl job-output --id … [--follow]
```

- `--as-user U` on profile-scoped nix / theming / crontab actions (same
  dispatch rules; cross-user crontab/theming remains rejected even for root —
  root edits root's crontab via this tool, not other users', matching the
  wire rule).
- `--json` prints the raw result value as JSON (scripting). Default: compact
  human output per module (best effort; JSON is the contract).
- Job mutations (`install`, `upgrade`, `remove`, …) return `job_id`: the CLI
  polls `JobOutput` and streams the log to **stdout live** (binding
  package-progress rule: terminal text, no percentage), then exits with the
  job's exit code.
- Destructive actions: `--yes` required (CLI equivalent of the GUI
  confirmation). Without it, print exactly what would happen and exit 2.
- Exit codes: 0 ok, 1 action/API error, 2 usage/root-gate/confirmation.

## Chroot behaviour

- No daemon start, no socket, no dinit. `ursinectl` is one static-ish musl
  binary copied into the chroot (release tarball already ships it).
- `/run` may be a missing/empty tmpfs: job dir and audit dir are created on
  demand (`create_dir_all`, mode 0700/0750 — same helpers the daemon uses).
- Modules whose backends need a live system (dbus services, nix-daemon)
  fail with the normal capability/error path; the CLI prints reason/fix like
  the GUI. `capabilities list` is the chroot health probe.
- `su`-based as_user helpers read `/etc/passwd` of the chroot (the CLI runs
  with `/` = chroot root).

## Non-goals

- Non-root mode, socket-client mode (`--remote`), remote hosts: rejected for
  now (recorded decision; can be revisited later).
- Interactive TUI: plain subcommands only.

## Security

- euid-0 gate is the entire auth model; no PAM in the CLI.
- Same dispatch validation as the wire (as_user scoping, config read/write
  guards, capability gating, audit on every action with principal root).
- Audit log shared with the daemon (`/var/log/ursine/audit.jsonl`,
  append-only): a chroot install leaves an audit trail inside the target.

## Tests

- Root-gate unit test (euid check extracted as a pure fn over an injected
  value; non-root refusal).
- Arg-parse tests: every `Action` variant has a subcommand; round-trip
  command line → `Action`.
- In-process dispatch integration tests: `status`, `capabilities`,
  `crontab get/set` against a temp HOME as root (container CI runs as root).
- Job streaming test: mutation returning a job id → polled output reaches
  stdout, exit code mirrors the job.

## Packaging

- Release tarball + flake + `install.sh` ship `ursinectl` next to `ursined`.
- Versioned with the workspace; no separate release cadence.
