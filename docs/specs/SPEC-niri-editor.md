# SPEC-niri-editor

## Objective

A schema-driven settings GUI for the niri Wayland compositor — the user SELECTS, never types raw KDL. Every niri configuration option is rendered as a form widget (toggles, dropdowns, sliders, key-capture fields, text entries for names), paginated like modern distro settings apps. Nothing like it exists anywhere.

## Design (binding)

- **Schema file** in `ursine-api`: declarative description of every niri option (path, type, default, constraints, widget hint, help text). Versioned per supported niri release; `niri --version` checked at runtime
- **Parsing**: existing `~/.config/niri/config.kdl` parsed via `kdl` crate; known nodes rendered into forms; **unknown/user-added nodes preserved verbatim** on write (round-trip safety: we only rewrite what we manage)
- **Writing**: serialize managed sections + preserved unknown nodes; atomic write (temp + rename); `.bak` backup of previous version; `niri validate` runs BEFORE commit — invalid config never lands
- **Sections (v1 forms)**: input (keyboard: layouts, repeat; touchpad: tap, natural scroll, dwt), pointer speed, outputs (mode, scale, position, VRR, transform), layout (gaps, struts, border/focus-ring: width+colors, preset widths/heights), animations (on/off, durations, curves from schema), keybinds (capture widget: press combo → recorded binding + action dropdown), gestures, environment variables (add/remove list), spawn-at-startup (add/remove list), cursor (size, theme), screenshot path prefix, hotkey overlay toggle
- **Live validation**: `niri validate --config <tmp>` on every change batch; errors mapped back to the offending form field

## Implementation status (0.7.0 baseline)

The daemon and GUI currently manage: layout gaps, animations off,
touchpad tap/natural-scroll/dwt, and cursor size/theme. The KDL round-trip,
`niri validate` gate, atomic write, timestamped backup, ownership preservation,
and user-scoped I/O (no root file access) are implemented. Additional v1 form groups below
(outputs, keybinds, gestures, environment/spawn lists, keyboard/pointer,
screenshot path, hotkey overlay) remain to be added to the schema and GUI.

## API surface

| Action | Implementation |
|---|---|
| `niri.getConfig` | parse current config.kdl → schema-typed JSON (managed + unknown count) |
| `niri.setSection` | apply form diff to managed nodes → validate → atomic write + backup → niri reload (niri auto-reloads on file change) |
| `niri.validate` | run `niri validate` on proposed config without writing (GUI shows result before Apply) |
| `niri.schema` | return schema version + option metadata (GUI builds forms dynamically) |

- Per-user file (`~/.config/niri/config.kdl`): `asUser` applies (daemon reads/writes as target user, `su -l` or direct file IO with setuid — file IO with ownership check, simplest and safest)

## Commands

```text
cargo test -p ursine-core niri
```

## Project Structure

```text
src/ursine-api/src/niri_schema.rs      declarative option schema (versioned)
src/ursine-core/src/modules/niri/
├─ mod.rs        handlers
├─ kdl.rs        parse/serialize managed+unknown round-trip
├─ apply.rs      schema -> KDL node transforms
└─ validate.rs   niri validate wrapper + error mapping to schema paths
src/ursine/src/pages/niri/                 GUI: dynamic form renderer per schema
```

## Code Style

```rust
pub struct OptionSchema { pub path: &'static str, pub kind: OptKind, pub default: OptValue,
                          pub widget: Widget, pub help: &'static str }
```

- Schema is data (const/static), not code per option; GUI renders from schema — new niri options = schema entry, no GUI code
- Keybind capture: widget-level (GTK event controller); stored as niri bind syntax (`Mod+Return { spawn "..."; }`)

## Testing Strategy

- Unit: round-trip property test — parse(sample config) → apply no-op → serialize == original (byte-identical or canonical-identical)
- Unit: unknown-node preservation (config with foreign nodes survives our write)
- Unit: schema completeness check — every `OptKind` variant constructible; constraint validation (scale range, durations)
- Integration (`#[ignore]`, needs niri): real `niri validate` against generated configs

## Boundaries

- Always: backup before write; validate before write; preserve unknown nodes; `niri --version` check vs schema version (mismatch → warn in GUI)
- Ask first: editing sections beyond the v1 list; touching includes
- Never: raw free-text KDL editing in v1; writing invalid config (validate gate is absolute); losing user comments (preserve comments where kdl crate allows; if lossy, note in GUI before Apply)

## Success Criteria

- User changes gaps size, enables VRR on an output, binds Super+E to a spawn — all via selection/capture, no typing of KDL — and niri live-reloads without a broken config, ever
- A hand-written exotic config: our write does not destroy the parts we do not manage

## Open Questions

- Comment preservation: `kdl` crate (kdl 6) preserves comments/format imperfectly — if round-trip loses comments we must decide: (a) accept loss with GUI warning, (b) comment-aware patching (edit only changed nodes in original text). Default proposal: (a) v1 with explicit warning, (b) v1.1. Needs your call.
