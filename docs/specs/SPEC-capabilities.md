# SPEC-capabilities

## Objective

Runtime detection of what the machine supports: a `Capabilities` registry built at daemon start, re-checkable per request (`ensure_available()`), exposed via API, and rendered by the GUI as `available / reason / fix`. Feeds tri-state config `"auto" | true | false` resolution. Never persisted — stale capabilities are the failure mode we exist to avoid.

## Tech Stack

- Rust stable, `nix`/std for stat + /proc reads; no external commands where a stat suffices

## Checks (v1)

| Capability | Check (cheap, ordered) |
|---|---|
| `btrbk` | `/usr/bin/btrbk` exists |
| `btrfs` | `/sys/fs/btrfs` present AND `btrfs filesystem show` exit 0 (cached per refresh) |
| `nix` | `/nix/store` exists AND `nix` in PATH |
| `accountsservice` | D-Bus name `org.freedesktop.Accounts` reachable |
| `niri` | `niri` binary AND `$XDG_CONFIG_HOME/niri` or `~/.config/niri` present |
| `cups` (v2) | D-Bus/IPP reachable |
| `dinit` | `/proc/1/comm` == `dinit` |

## Commands

```
cargo test -p ursine-core capabilities
```

## Project Structure

```
src/ursine-core/src/capabilities/
├─ mod.rs        Capability enum, Registry, ensure_available()
├─ checks.rs     one fn per capability, pure where testable
└─ api.rs        capabilities.list, capabilities.refresh, config.init handlers
```

## Code Style

```rust
pub async fn ensure_available(&self, cap: Capability) -> Result<(), CapabilityError> {
    if self.config.resolve(cap) == TriState::Off { return Err(Disabled); }
    if !self.check(cap).await { return Err(Unavailable(reason(cap), fix(cap))); }
    Ok(())
}
```

- Resolution order: config `false` → off; config `true` → force (warn if check fails, surface error in GUI); `"auto"` → check decides
- `reason` and `fix` are human strings with the exact command (`apk add btrbk`), localized later, English v1

## Testing Strategy

- Unit: each check fn against fixtures (temp dirs, fake /proc paths injected via trait `Sysinfo`)
- Unit: TriState resolution table (auto×present, auto×absent, true×absent → warn+error, false×present → off)
- `config.init` golden test: generated TOML parses back and matches defaults

## Boundaries

- Always: `ensure_available()` before ANY module action; refresh on API call; include `reason`+`fix` in errors
- Ask first: adding a capability (new module); changing check order/cost
- Never: persist capabilities anywhere; run expensive probes per request (one stat per request max, heavy checks cached per refresh)

## Success Criteria

- On the dev machine: capabilities list returns real values (mostly unavailable), config.init writes a valid commented `/etc/ursine/config.toml`
- GUI can render every module as available/disabled with reason+fix

## Open Questions

- None blocking.
