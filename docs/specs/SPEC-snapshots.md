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
| `snapshots.create` | `btrbk snapshot [<section>]` |
| `snapshots.restore` | `btrfs subvolume snapshot <snapshot> <target>` — CONFIRMATION REQUIRED in GUI (destructive); preview validates paths and exact argv first. Existing target is moved to `<target>.ursine-backup-<ts>` before the snapshot; failure restores it. |
| `snapshots.delete` | GUI single-snapshot dropdown + bulk delete for selected stats sources; daemon validates each path against the current btrbk list, then `btrfs subvolume delete`. Confirmation required. |
| `snapshots.getConfig` | parsed view of `/etc/btrbk/btrbk.conf` (sources, targets, retention, timestamp/scheduler) |
| `snapshots.setConfig` | Managed global options written as a top block, preserving sections/comments; `.bak` backup; validated with `btrbk --format raw list config` before atomic replace. GUI exposes dropdowns. |
| `snapshots.initConfig` | **create** `/etc/btrbk/btrbk.conf` from scratch: conservative default for the FeralOS btrfs layout (hourly keep 24, daily keep 14), `/home` or `@` subvolume |
| `snapshots.preUpdate` | **deferred**: package install/upgrade hook not wired yet. |

- `--format raw`; parser for the documented raw keys. btrbk 0.32 has no JSON output.
- `setConfig` rewrites only managed global keys, backs up `.conf.bak`, and rejects values that fail btrbk validation. Per-subvolume overrides are preserved.

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
- Config write path: parse → apply managed changes → our-comment validation → `btrbk dryrun` → atomic write + `.bak`

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
- If btrbk.conf is absent: `snapshots.initConfig` creates a conservative default (`/home` or `@`, hourly+daily retention) and validates it
- Pre-update snapshots on package changes are deferred (separate hook)
- Restore requires preview + explicit confirmation, then creates a writable btrfs snapshot

## Open Questions

- Per-subvolume overrides and pre-update hooks remain future work.
