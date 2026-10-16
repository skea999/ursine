# Capability Map — Ursine v1

Phase-0 artifact of spec-driven-development (`docs/intent/ursine.md` is the source of truth for scope).
Gate: skea999 approves module boundaries, dependency direction and build order BEFORE module specs are written.

## Modules

| Module id | Responsibility | Depends on |
|---|---|---|
| `protocol` | `ursine-api` crate: newline-JSON envelope, ModuleId, Error, Principal, session token type, TriState config model, Capability types, `asUser` field. Token issuance behind a trait (minimal seam for future OIDC/web — no oidc feature in v1) | — |
| `daemon-core` | `ursined` + `ursine-core`: config load/save, unix socket `/run/ursine.sock` (root:root, 0666), peer creds, PAM handshake (cargo feature `pam`, stub on dev), wheel check, RAM session tokens (TTL 10 min idle, configurable) + rate limit, JSONL audit, request dispatch | `protocol` |
| `capabilities` | runtime detection (stat/proc checks), `ensure_available()` per action, `capabilities.refresh` + `config.init` APIs | `daemon-core` |
| `packages-nix` | nix module: list installed (per-user profiles), install, remove, search, upgrade — via `nix profile` executed as `asUser` | `daemon-core`, `capabilities` |
| `snapshots` | btrbk integration module: expose btrbk's own functionality (list, run, stats, restore, retention) against its config; automatic pre-update `btrbk run` hook. Never reimplement snapshot logic | `daemon-core`, `capabilities` |
| `users` | users module via accountsservice D-Bus: list, create, modify, delete users; group membership | `daemon-core`, `capabilities` |
| `niri-editor` | niri module: schema-driven FORMS for every niri config option (user selects, never types raw KDL); keybind capture UI; unknown KDL nodes preserved verbatim; atomic write + `.bak` backup; live `niri validate` | `daemon-core`, `capabilities` |
| `gui` | `ursine` GTK4/libadwaita via **relm4**: login (PAM prompt), navigation shell, nix store view, snapshots panel, users panel, niri forms, capability-gated panels, `config.init` button. Thin API client, zero logic | `protocol` (client of `daemon-core`) |
| `packaging-infra` | `flake.nix` (musl static), `.github/workflows/ci.yml` (feature test matrix — every feature combination THAT EXISTS, clippy/fmt gate, flake check/build, Release on tag), Docker Alpine local build docs, dinit service file | all (written last, kept green) |

## Dependency direction

```
protocol ← daemon-core ← capabilities ← {packages-nix, snapshots, users, niri-editor}
                                     ↖ gui (consumes protocol only)
packaging-infra wraps everything (no runtime deps)
```

No cycles. `gui` never imports `ursine-core`; only `ursine-api`.

## Build order

`protocol` → `daemon-core` → `capabilities` → `packages-nix` → `snapshots` → `users` → `niri-editor` → `gui` → `packaging-infra`

(parallelizable after `daemon-core`: the four module specs; `packaging-infra` can start as soon as `protocol` lands)

## Spec artifacts

Each module gets `docs/specs/SPEC-<module-id>.md` after map approval.
