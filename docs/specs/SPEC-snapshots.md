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
| `snapshots.restore` | `btrfs subvolume snapshot <snapshot> <target>` — CONFIRMATION REQUIRED in GUI (destructive); preview validates paths and exact argv first. A target that is a mount point is refused by the preview AND the run (`conflict` + `fix`: restore from a live environment or unmount it) — a mount point cannot be renamed, so a mounted subvolume (FeralOS `@home` on `/home`) can never be restored in place. The live target is parked in `<target>.ursine-backup.tmp` first and the chain is committed only AFTER the snapshot succeeded (slot `<target>.ursine-backup`, `backup_configs.copies` copies, the previous version shifts to `.1`, `.2`, …; failure moves the tree back with the chain untouched). Legacy timestamped backups from older releases are pruned to the same cap. |
| `snapshots.delete` | GUI single-snapshot dropdown + bulk delete for selected stats sources; daemon validates each path against the current btrbk list, then `btrfs subvolume delete`. Confirmation required. |
| `snapshots.getConfig` | parsed view of `/etc/btrbk/btrbk.conf` (sources, targets, retention, timestamp/scheduler) |
| `snapshots.setConfig` | One `# --- Ursine managed options ---` … `# --- end Ursine managed options ---` block at the top, replaced IN PLACE on every call (duplicates and the blank runs they left are removed); everything outside the markers, comments included, is preserved; uniform `.ursine-backup[.N]` rotation (replaces the single `.conf.bak`); validated with btrbk before atomic replace. Idempotent: the same settings give byte-identical output. GUI exposes dropdowns. |
| `snapshots.initConfig` | **create** `/etc/btrbk/btrbk.conf` from scratch with a layout the machine can actually run (hourly keep 24, daily keep 14): the source subvolume is `/home` when the machine has it (its own subvolume or a directory), else the first mounted btrfs subvolume, else `@`; the snapshot directory is an existing one btrbk already uses (`/btrbk/snapshots` on FeralOS, then `/.snapshots`), else `/.snapshots` — which init creates with `mkdir -p` and reports as a typed error when it cannot. The written config is then proved with a read-only btrbk run (`list snapshots` parses the config and resolves the source and the snapshot directory); if btrbk rejects it the file is removed and the action fails instead of reporting a configuration that cannot run. Skipped only when btrbk is not installed (the capability gate owns that case). |
| `snapshots.preUpdate` | **deferred**: package install/upgrade hook not wired yet. |

- `--format raw`; parser for the documented raw keys. btrbk 0.32 has no JSON output.
- `setConfig` rewrites only managed global keys, rotates the previous config through the uniform `.ursine-backup[.N]` slots, and rejects values that fail btrbk validation. Per-subvolume overrides are preserved.

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
- Restore order is validate → park → snapshot → commit: nothing rotates before the snapshot succeeded, and a target that cannot be renamed is refused before anything moves
- `setConfig` owns only the managed block: it never rewrites, reorders or re-comments user content outside the markers
- Ask first: any direct `btrfs subvolume` call (only if btrbk lacks the operation — justify in PR)
- Never: silent restore; deleting snapshots outside btrbk retention; writing btrbk.conf without validation. Restore is the only approved btrfs fallback (btrbk 0.32 has no restore command).

## Success Criteria

- GUI shows btrbk-managed snapshots with timestamps/sources from `btrbk list`
- GUI shows btrbk config as dropdowns and saves managed global options
- If btrbk.conf is absent: `snapshots.initConfig` creates a conservative default (`/home` or `@`, hourly+daily retention) and validates it
- Pre-update snapshots on package changes are deferred (separate hook)
- Restore requires preview + explicit confirmation, then creates a writable btrfs snapshot (targets that are mount points are refused, not attempted)
- `setConfig` is idempotent: two calls with the same settings leave one managed block and identical bytes
- `initConfig` never reports success for a configuration btrbk cannot read

## Open Questions

- Per-subvolume overrides and pre-update hooks remain future work.

## GUI retention

Preset dropdowns remain for common values; custom text fields accept
`btrbk` retention syntax such as `14d 8w 6m` and `24h 14d`.
