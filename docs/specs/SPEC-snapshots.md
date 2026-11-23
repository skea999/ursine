# SPEC-snapshots

## Objective

Expose btrbk's own functionality — nothing reimplemented. Ursine integrates btrbk: lists what btrbk manages, triggers btrbk runs (including the automatic pre-update snapshot), restores via btrbk, and reads retention from btrbk's config. FeralOS installs btrfs everywhere; btrbk is packaged; snapper/timeshift are not — btrbk is the single owner of snapshot logic.

## Tech Stack

- `btrbk` (packaged on Alpine) executed as root where needed: `btrbk --format raw list`, `btrbk snapshot`, `btrbk --format raw stats`, `btrbk clean`, `btrbk dryrun`. Restore uses `btrfs subvolume snapshot` because btrbk 0.32 has no restore command.
- Config source of truth: `/etc/btrbk/btrbk.conf` (parsed read-only: sources, targets, retention)

## API surface

| Action | Implementation |
|---|---|
| `snapshots.list` | `btrbk --format raw list snapshots` → parsed |
| `snapshots.stats` | `btrbk --format raw stats` |
| `snapshots.create` | `btrbk snapshot [<section>]` — destructive: btrbk applies the retention rules (`snapshot_preserve`, `snapshot_preserve_min`) after the pass and deletes the snapshots they no longer keep, so the action is `Gate::Always` |
| `snapshots.restore` | `btrfs subvolume snapshot <snapshot> <target>` — CONFIRMATION REQUIRED in GUI (destructive); preview validates paths and exact argv first, and refuses a target that is a mount point (a mounted subvolume cannot be renamed; unmount it or restore from a recovery boot; mount points are compared with trailing slashes trimmed). Existing target rotates into slot `<target>.ursine-backup` (the uniform chain, `backup_configs.copies` copies; the previous version shifts to `.1`, `.2`, …) before the snapshot; failure moves it back. The chain is capped: the oldest saved tree is deleted to make room, and the preview and apply answer name it in `would_delete`. Legacy timestamped backups from older releases are pruned to the same cap. |
| `snapshots.delete` | GUI single-snapshot dropdown + bulk delete for selected stats sources; daemon validates each path against the current btrbk list, then `btrfs subvolume delete`. Confirmation required. |
| `snapshots.getConfig` | parsed view of `/etc/btrbk/btrbk.conf` (sources, targets, retention, timestamp/scheduler) |
| `snapshots.setConfig` | Managed global options written as a top block, preserving sections/comments; a call replaces only the options it names (earlier managed values are kept), and the whole block is rewritten once — one start marker, one end marker, no blank runs. A managed option that also appears below the first `volume`/`subvolume`/`target` line is refused with a `conflict` error naming the key and line, because btrbk would use the section value and ignore the managed one. `setConfig` also rotates the previous file through the uniform `.ursine-backup[.N]` slots (replaces the single `.conf.bak`), validated with `btrbk --format raw list config` before atomic replace. GUI exposes dropdowns. |
| `snapshots.initConfig` | **create** `/etc/btrbk/btrbk.conf` from scratch: conservative default for the FeralOS btrfs layout (hourly keep 24, daily keep 14), with one `subvolume` line per top-level subvolume `btrfs subvolume list -a /` reports (nested subvolumes and the snapshot store are skipped). Uses an existing `/btrbk/snapshots` (or `/.snapshots`) as `snapshot_dir`, creates `/.snapshots` when neither exists, and probes each named subvolume with `btrfs subvolume show` — a layout that cannot run is refused with a typed error instead of being written. Falls back to the historical single name (`home`, else `@`) when nothing can be read. |
| `snapshots.preUpdate` | **deferred**: package install/upgrade hook not wired yet. |

- `--format raw`; parser for the documented raw keys. btrbk 0.32 has no JSON output.
- `setConfig` rewrites only managed global keys, keeps the values an earlier call wrote, rotates the previous config through the uniform `.ursine-backup[.N]` slots, and rejects values that fail btrbk validation. Per-subvolume overrides are preserved.
- `setConfig` error contract: `conflict` (with a `fix`) when a managed key is also set below the first section — patching in place would edit lines the call does not own, and silence is the bug; `invalid` when btrbk rejects the merged document or a value fails validation.
- `create` is gated `Always` (`Effect::Mutating`): the btrbk pass deletes snapshots the retention rules drop. No no-prune btrbk flag is used — its availability depends on the installed version and a never-pruning pass would let the disk fill.

## Commands

```text
cargo test -p ursine-core snapshots
```

## Project Structure

```text
src/ursine-core/src/modules/snapshots/
├─ mod.rs        action handlers
├─ btrbk.rs      exec + JSON parse of btrbk output
└─ conf.rs       parser/writer of /etc/btrbk/btrbk.conf (managed lines + preserved directives)
tests/snapshots/ integration tests (btrfs-only machines)
```

## Code Style

```rust
let out = Exec::root(["btrbk", "--format", "raw", "list", "snapshots"]).run().await?;
let snaps = btrbk::parse_snapshots(&out)?;
```

- btrbk is the ONLY implementation of snapshot logic; if btrbk cannot do it, v1 does not do it
- Restore always: dryrun preview → explicit confirm token from frontend → real run
- Config write path: parse → apply managed changes → our-comment validation → `btrbk dryrun` → backup rotation → atomic write

## Testing Strategy

- Unit: JSON parsers against committed btrbk fixture outputs; conf parser against sample configs
- Integration (`#[ignore]`, btrfs-only machines): create/list/restore on a THROWAWAY test subvolume

## Boundaries

- Always: `ensure_available(btrbk)` and `ensure_available(btrfs)`; preview before every restore and config write; audit every create/restore/clean/config change
- Ask first: any direct `btrfs subvolume` call (only if btrbk lacks the operation — justify in PR)
- Never: silent restore; deleting snapshots outside btrbk retention; writing btrbk.conf without validation. Restore is the only approved btrfs fallback (btrbk 0.32 has no restore command).

## Success Criteria

- GUI shows btrbk-managed snapshots with timestamps/sources from `btrbk list`
- GUI shows btrbk config as dropdowns and saves managed global options
- If btrbk.conf is absent: `snapshots.initConfig` creates a conservative default from the machine's subvolumes (hourly+daily retention) and validates it
- Pre-update snapshots on package changes are deferred (separate hook)
- Restore requires preview + explicit confirmation, then creates a writable btrfs snapshot

## Open Questions

- Per-subvolume overrides and pre-update hooks remain future work.

## GUI retention

Preset dropdowns remain for common values; custom text fields accept
`btrbk` retention syntax such as `14d 8w 6m` and `24h 14d`.
