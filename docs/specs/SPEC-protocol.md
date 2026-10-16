# SPEC-protocol (ursine-api)

## Objective

Define the ONLY shared contract between daemon and frontends: a crate `ursine-api` with serde types for the newline-delimited JSON protocol, the config model, capabilities and errors. Deterministic: both `ursined` and `ursine` compile against these types, so protocol drift is a compile error.

## Tech Stack

- Rust stable, edition 2021, no unsafe
- `serde`, `serde_json` (derive), `thiserror`
- No IO, no tokio, no GTK in this crate (pure types)

## Commands

```
cargo build -p ursine-api
cargo test  -p ursine-api
cargo clippy -p ursine-api -- -D warnings
```

## Project Structure

Layout (binding, per skea999): **all code under `src/`, all tests under `tests/`, generated files AND build output under `artifacts/`** (gitignored contents via `.gitignore`; folder tracked via `.gitkeep`; `.cargo/config.toml` sets `target-dir = "artifacts/target"`).

```
src/ursine-api/
├─ Cargo.toml
└─ src/
   ├─ lib.rs           re-exports
   ├─ envelope.rs      Request, Response, framing constants
   ├─ module.rs        ModuleId enum ( PackagesNix, Snapshots, Users, Niri, Capabilities, Config, System )
   ├─ action.rs        per-module Action payloads
   ├─ principal.rs     Principal { origin, user/groups }, asUser field type
   ├─ token.rs         SessionToken (opaque string), AuthRequest/AuthResponse
   ├─ capability.rs    CapabilityReport { module, available, reason, fix }
   ├─ config.rs        TriState, UrsineConfig (serde for /etc/ursine/config.toml)
   └─ error.rs         Error enum, error codes stable for UIs
tests/ursine-api/
└─ golden/             committed request/response samples per action
artifacts/             generated + BUILD OUTPUT (target-dir): test outputs, coverage, scratch
```

## Code Style

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Auth { user: String, password: String },
    Call { module: ModuleId, action: Action, #[serde(default)] as_user: Option<AsUser> },
}
```

- serde tag/adjacently tagged enums for every payload; `snake_case` wire names
- Every type: `Debug, Clone, Serialize, Deserialize, PartialEq`
- Wire format is versioned: `Request`/`Response` carry `v: u16` (starts at 1)

## Testing Strategy

- Unit: serde roundtrip for EVERY public type (property of roundtrip == identity); unit tests live with the code (`src/ursine-api/src/`, `#[cfg(test)]`) or under `tests/ursine-api/`
- Golden files: one request/response sample per module action, committed under `tests/ursine-api/golden/`
- Coverage target: all types exercised; no async, so plain `cargo test`

## Boundaries

- Always: roundtrip test per type; golden test when changing any payload; semver-check before publishing versions in-repo
- Ask first: any change to existing wire fields (breaking change to frontends); new ModuleId variants
- Never: IO, blocking, `unsafe`, logic (only types + parsing)

## Success Criteria

- `cargo test -p ursine-api` green with golden + roundtrip suites
- Every message a frontend can send or receive has a type in this crate
- Compiles on musl (checked by CI matrix default target)

## Open Questions

- None blocking.
