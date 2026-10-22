# SPEC-snapshots

## Objective

Expose btrbk's own functionality — nothing reimplemented. Ursine integrates btrbk: lists what btrbk manages, triggers btrbk runs (including the automatic pre-update snapshot), restores via btrbk, and reads retention from btrbk's config. FeralOS installs btrfs everywhere; btrbk is packaged; snapper/timeshift are not — btrbk is the single owner of snapshot logic.

## Tech Stack

- `btrbk` (packaged on Alpine) executed as root where needed: `btrbk --format json list`, `btrbk run`, `btrbk stats`, `btrbk restore`, `btrbk clean`, `btrbk dryrun`
- Config source of truth: `/etc/btrbk/btrbk.conf` (parsed read-only: sources, targets, retention)

## API surface

| Action | Implementation |
|---|---|
| `snapshots.list` | `btrbk --format json list snapshots` → parsed |
| `snapshots.stats` | `btrbk --format json stats` |
| `snapshots.create` | `btrbk run <section>` (snapshot-only section or `--type snapshot` per btrbk semantics) |
| `snapshots.restore` | `btrbk restore <snapshot> <target>` — CONFIRMATION REQUIRED in GUI (destructive); dryrun preview first |
| `snapshots.delete` | btrbk retention owns deletion; manual delete only via `btrbk clean` (honors config) |
| `snapshots.getConfig` | parsed view of `/etc/btrbk/btrbk.conf` (sources, targets, retention, timestamp/scheduler) |
| `snapshots.setConfig` | **read-write**: change WHICH subvolumes are snapshotted (add/remove `<subvolume>` source entries) and the SCHEDULE (retention `snapshot_preserve*` + `timestamp_format`) → write config → validate with `btrbk dryrun` before commit; backup `.bak` of previous config |
| `snapshots.initConfig` | **create** `/etc/btrbk/btrbk.conf` from scratch: commented default for the FeralOS btrfs layout (hourly keep 24, daily keep 14) pre-filled with detected btrfs subvolumes |
| `snapshots.preUpdate` | internal: `btrbk run` snapshot-only before ANY package install/upgrade (called by packages modules, not exposed to UI as a button-only op) |

- `--format json` everywhere btrbk supports it; fallback parser for missing fields
- Config edits are schema-validated by OUR writer (only lines we manage are rewritten; user's custom directives preserved verbatim, same policy as niri editor) and verified by `btrbk dryrun` — invalid config never lands

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
let out = Exec::root(["btrbk", "--format", "json", "list", "snapshots"]).run().await?;
let snaps = btrbk::parse_list(&out)?;
```

- btrbk is the ONLY implementation of snapshot logic; if btrbk cannot do it, v1 does not do it
- Restore always: dryrun preview → explicit confirm token from frontend → real run
- Config write path: parse → apply managed changes → our-comment validation → `btrbk dryrun` → atomic write + `.bak`

## Testing Strategy

- Unit: JSON parsers against committed btrbk fixture outputs; conf parser against sample configs
- Integration (`#[ignore]`, btrfs-only machines): create/list/restore on a THROWAWAY test subvolume

## Boundaries

- Always: `ensure_available(btrbk)` and `ensure_available(btrfs)`; dryrun before every restore AND every config write; audit every create/restore/clean/config change
- Ask first: any direct `btrfs subvolume` call (only if btrbk lacks the operation — justify in PR)
- Never: manual `btrfs subvolume snapshot/delete` bypassing btrbk in v1; silent restore; deleting snapshots outside btrbk retention; writing btrbk.conf without dryrun validation

## Success Criteria

- GUI shows btrbk-managed snapshots with timestamps/sources from `btrbk list`
- User changes WHICH subvolumes are snapshotted and the retention schedule from the GUI — config written, dryrun-validated, audited
- If btrbk.conf is absent: `snapshots.initConfig` creates it (detected subvolumes pre-filled, commented default schedule)
- Installing a package via the store triggers a pre-update btrbk snapshot automatically (visible in snapshot list + audit)
- Restore requires preview + explicit confirmation, then runs btrbk restore

## Open Questions

- None (default schedule on initConfig: hourly keep 24, daily keep 14 — user editable from GUI).
