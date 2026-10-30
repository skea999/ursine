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

Generic, serde-driven — NO per-action subcommand code:

```text
ursinectl [--json] [--yes] [--as-user U] [--config PATH] \
          <module> <action> [key=value ...]
```

- Module names are the wire names (`packages_nix`, …) with short aliases
  (`nix`, `apk`, `flatpak`, `caps`).
- Action names and field names are the serde `Action` tags (`install_apk`,
  `dry_run`, …): every action that exists in the API is reachable from the
  CLI **by construction** — a new API action needs zero CLI changes (parity
  rule satisfied automatically; the CLI table in `--help` is the only doc
  touch-up).
- Values parse as JSON when valid (`dry_run=true`, `keep=3`,
  `value={"layout":{...}}`), else fall back to plain strings. `key=@path`
  reads a file, `key=-` reads stdin.
- `password` / `new_password` MUST be `key=-` (read from stdin): secrets
  never ride on argv (visible in `/proc`).
- `--as-user U` on profile-scoped nix / theming / crontab actions (same
  dispatch rules; cross-user crontab/theming remains rejected even for root —
  root edits root's crontab via this tool, not other users', matching the
  wire rule). Root's own crontab runs the trusted tool directly (no `su`).
- `--json` prints the raw result value as JSON (scripting). Default:
  pretty-printed JSON (the contract; per-module human tables are a possible
  refinement, not a requirement).
- Job mutations (`install`, `upgrade`, `remove`, …) return `job_id`: the CLI
  polls `JobOutput` and streams the log to **stdout live** (binding
  package-progress rule: terminal text, no percentage), then exits with the
  job's exit code. CLI jobs live under `/run/ursinectl/jobs` (never the
  daemon's `/run/ursine/jobs` — the registry purges its dir at startup).
- Destructive actions (removals, deletes, rollbacks, gc, restore-apply,
  crontab set, theme apply, …): `--yes` required (CLI equivalent of the GUI
  confirmation); without it, refuse with the action name and exit 2.
  Dry-run/preview variants never require `--yes`.
- Exit codes: 0 ok, 1 action/API error, 2 usage/root-gate/confirmation.
- Auth: the euid-0 gate replaces PAM entirely; the CLI's in-process
  authenticator rejects every `auth` request outright (no stub backdoor in
  a root binary).

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
