# SPEC-protocol (ursine-api)

## Objective

Define the ONLY shared contract between daemon and frontends: a crate `ursine-api` with serde types for the newline-delimited JSON protocol, the config model, capabilities and errors. Deterministic: both `ursined` and `ursine` compile against these types, so protocol drift is a compile error.

## Tech Stack

- Rust stable, edition 2021, no unsafe
- `serde`, `serde_json` (derive), `thiserror`
- No IO, no tokio, no GTK in this crate (pure types)

## Commands

```text
cargo build -p ursine-api
cargo test  -p ursine-api
cargo clippy -p ursine-api -- -D warnings
```

## Project Structure

Layout (binding, per skea999): **all code under `src/`, all tests under `tests/`, generated files AND build output under `artifacts/`** (gitignored contents via `.gitignore`; folder tracked via `.gitkeep`; `.cargo/config.toml` sets `target-dir = "artifacts/target"`).

```text
src/ursine-api/
├─ Cargo.toml
└─ src/
   ├─ lib.rs           re-exports
   ├─ envelope.rs      Request, Response, framing constants
   ├─ module.rs        ModuleId enum ( PackagesNix, Snapshots, Users, Niri, Capabilities, Config, System )
   ├─ action.rs        per-module Action payloads
   ├─ catalogue.rs     action catalogue: read-only vs mutating, --yes gate, parameters
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
- An action wire name carries its module when more than one module could own the
  verb: `install_apk`, `install_flatpak`, `install_nix`. A renamed action keeps
  its old tag as `#[serde(alias = "…")]`, so the change is additive: a client
  written against the old name deserializes to the same variant and the
  protocol version does not move.
- Every type: `Debug, Clone, Serialize, Deserialize, PartialEq`
- Wire format is versioned: `Request`/`Response` carry `v: u16` (currently 7; v2 added the search `{results, truncated}` page and exact prune preview fields, v3 package mutations `job_id`, v4 `list_apk_repositories` `{url, tag}` rows, v5 no-D-Bus removals: `set_bluetooth_pairing_pin`, `pairing_pin_set`, capability `accountsservice`→`shadow`, v6 Nix repository scopes (`system: bool`) and the per-source remove, v7 the unused `refresh_apk` action removed). A request whose `v` differs from `PROTOCOL_VERSION` is refused with `invalid`, the message naming the sent and the accepted version, before any other work; frontends must tolerate older daemons' response shapes by treating parse failures as capability errors. The `spec_protocol_states_the_current_wire_version` test in `src/ursine-api/src/envelope.rs` fails when this sentence and the constant disagree.

## Testing Strategy

- Unit: serde roundtrip for EVERY public type (property of roundtrip == identity); unit tests live with the code (`src/ursine-api/src/`, `#[cfg(test)]`) or under `tests/ursine-api/`
- Golden files: one request/response sample per module action, committed under `tests/ursine-api/golden/`
- Coverage target: all types exercised; no async, so plain `cargo test`

## Boundaries

- Always: roundtrip test per type; golden test when changing any payload; semver-check before publishing versions in-repo
- Ask first: any change to existing wire fields (breaking change to frontends); new ModuleId variants
- Never: IO, blocking, `unsafe`, logic (only types + parsing)

## Action catalogue and the read-only rule

The crate also owns the metadata about an action, so no frontend keeps its
own list:

- `catalogue::Effect` (`ReadOnly` | `Mutating`) says what a request does to
the machine. `Action::effect()` answers for the *request*: a `dry_run`
preview is read-only even though the variant mutates.
- `catalogue::Gate` (`Never` | `Always` | `Request`) says whether the
destructive confirmation (`--yes` in the CLI, a dialog in the GUI) is
required. `Action::is_destructive()` resolves `Request` from the payload
(`dry_run`, `apparmor_set_mode` with `mode=disable`, `set_firewall_enabled`
with `enabled=false`).
- `catalogue::ACTIONS` is one row per `Action` variant (wire name, module,
effect, gate, parameters with a `required` flag). `Action::spec()` is an
exhaustive match: a new variant does not compile until it is classified,
and the drift tests in `src/ursinectl/tests/samples.rs` fail until the row
is in `ACTIONS`.
- `catalogue::json_document()` renders the stable client document, schema id
`ursine.actions/1` (printed by `ursinectl actions --json`):

```json
{"schema":"ursine.actions/1","count":143,"actions":[
  {"name":"install_apk","module":"packages_apk","effect":"mutating",
   "destructive":"never","previewable":false,
   "params":[{"name":"package","required":true}]}]}
```

Rules for adding an action: classify it in `catalogue.rs` (the match is
exhaustive, so this is forced), keep a read-only action ungated, keep
dry-run/preview variants read-only, and never mark a read-only action as
gated. A payload may override the nominal effect the same way `dry_run`
does: `list_apk_updates` is `read-only` (cached index) and its
`refresh=true` request is `mutating`, because that request runs
`apk update`. The catalogue is metadata only: it changes no wire field
and no dispatch.

Parameters are names plus `required` (`true` when the wire rejects a
request without the field: not an `Option`, no `serde(default)`). A full
JSON Schema (types, ranges, enums) is deliberately **not** shipped: the
catalogue would have to stay true to serde, which it cannot without a
derive, so it stays at what can be derived by hand and be tested.

## Auth envelope and the credential scope

`auth` is the only unauthenticated request. Two optional fields scope the
session the daemon issues; both tolerate absence, so a frontend that predates
them keeps exactly the read-write access it had and shows no client in the
audit log.

| Field | Direction | Type | Default | Meaning |
| --- | --- | --- | --- | --- |
| `read_only` | request | bool | `false` | Ask for a read-only credential. Dispatch refuses every mutating request of that session (`Action::is_mutating()`: the request-level effect, so a `dry_run` preview or a plain read still works) with code `forbidden` and a `fix` naming the scope. |
| `client` | request | string, optional | absent | Frontend name: 1–64 bytes from `[A-Za-z0-9_.-]`. Copied into the audit log as `on_behalf_of`; never used for authorization. A malformed value is `invalid` with a `fix`, answered before any credential work. |
| `read_only` | `auth_ok` | bool | `false` | The scope the daemon granted, fixed for the life of the token. |
| `client` | `auth_ok` | string, optional | absent | Echo of the accepted client id, so a client can confirm the name the audit log will show. |

The scope belongs to the session, not to the token payload: `SessionToken`
stays an opaque string, nothing is written to disk, and neither the shadow
handshake nor the `wheel` check changes. A client that wants write access
after authenticating read-only authenticates again — the scope cannot be
widened on an existing token.

## Success Criteria

- `cargo test -p ursine-api` green with golden + roundtrip suites
- Every message a frontend can send or receive has a type in this crate
- Compiles on musl (checked by CI matrix default target)

## Open Questions

- None blocking.
