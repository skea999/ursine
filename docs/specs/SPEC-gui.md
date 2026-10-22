# SPEC-gui (ursine)

## Objective

The native frontend `ursine`: **libcosmic** app, thin client of `ursined`. Login, one navigation shell, capability-gated panels (nix store, snapshots, users, niri forms), and a Settings screen for capabilities and module modes. UX principles (toolkit-independent): one navigation structure, terminology from `CONTEXT.md`, no jargon, no broken panels, confirmation before destructive actions, keyboard-first, empty/error states designed.

## Tech Stack

- **libcosmic** (COSMIC toolkit, iced-based) — decided 2026-10-19: security wins over visual coherence with the Wayle shell. No GTK/gdk-pixbuf/librsvg C parsers in the GUI process.
- Pin: `libcosmic = { git = "https://github.com/pop-os/libcosmic", rev = "430f3d48a97d8779f9c1985473912e594ccca1ae" }` (git dependency, pre-1.0 — pin exact rev, isolate behind our own UI layer).
- COSMIC design language for widgets (nav_bar, cards, dialogs, toggles, spin_button, table_view). Visual divergence from the GTK shell accepted (distro already mixes COSMIC apps on niri).
- Async daemon calls via iced subscriptions/commands (tokio underneath); UI thread never blocked.
- Client types ONLY from `ursine-api`.

## Screens (v1)

| Screen | Content |
|---|---|
| Login | PAM prompt (user pre-filled from session, password field); on success token kept in process memory only |
| Home/Overview | module cards: enabled (green, opens panel) / disabled (grey + reason + fix command + "Run fix" if trivial) — from `capabilities.list` |
| Store (nix) | search field (results list w/ description), installed list, install/remove/upgrade buttons with progress, per-user selector (asUser dropdown) |
| Snapshots | btrbk list (source, timestamp), "Create snapshot", restore flow (preview diff/stats → confirm dialog typing the snapshot name), btrbk config summary, retention info |
| Users | list (avatar placeholder, name, admin badge), create dialog (name, full name, admin toggle), set password, delete (confirm), groups editor |
| Niri | dynamic form pages rendered from `niri.schema` (sections sidebar: Input, Outputs, Layout, Animations, Keybinds, Gestures, Autostart, Env); Apply bar with validate state; keybind capture widget |
| Settings | detected capabilities with refresh, module modes (`auto` / manual on / manual off) from `config.get_config` and `config.set_module_switch`, configuration file path |

- Destructive actions (restore snapshot, delete user, remove packages): COSMIC dialog confirm, stating EXACTLY what will happen
- Capability-gated: disabled modules render as cards with reason + `fix` (e.g. `apk add btrbk`) — never a broken screen

## Commands

```text
cargo build -p ursine
cargo run -p ursine
cargo clippy -p ursine -- -D warnings
```

## Project Structure

```text
src/ursine/src/
├─ main.rs           cosmic::Application setup
├─ client.rs         socket client: connect, auth, Bearer calls (tokio)
├─ state.rs          shared app model: token, capabilities, asUser selection
└─ pages/            one module per screen, mapped from libcosmic widgets
   ├─ login.rs, home.rs, store.rs, snapshots.rs, users.rs, niri/, settings.rs
```

## Code Style

```rust
impl cosmic::Application for Ursine {
    type Message = Message;
    fn update(&mut self, message: Message) -> cosmic::Task<Message> {
        match message {
            Message::Install(pkg) => cosmic::task::future(async move { /* client.call(...) */ }),
        }
    }
}
```

- One module per screen, `cosmic::Application` at the root; screens are plain functions returning `Element`
- Async work goes through `Task`/subscriptions (never blocking the UI thread)
- UI strings: user-friendly English v1, collected in one module for future i18n; terminology from CONTEXT.md (never "server", always "daemon"; never "backup", always "snapshot")
- No logic: business decisions (what to run, validation beyond form constraints) come from the daemon; GUI only renders + collects

## Web frontend (v3)

Not part of v1. Same API, same UX model (navigation, terminology, capability
gating, confirmations). **Binding constraint: the web GUI must be memory-safe**
— Rust→WASM preferred, TypeScript acceptable fallback. Stack decision deferred
to v3: see `docs/specs/SPEC-web.md`.

## Testing Strategy

- Unit: `client.rs` protocol handling against a mock socket (dev); state transitions
- UI: manual test checklist per screen in PR template (GUI testing infra is not worth it v1); screenshots in docs for UX review
- Consistency check: every panel uses the same header/layout components (enforced in review)

## Boundaries

- Always: capability check before showing an action; confirm dialog before destructive ops; spinner/progress for any op >100ms; error toasts with actionable text
- Ask first: new screen; new dependency (any crate beyond libcosmic/tokio); changing navigation structure
- Never: business logic in GUI; storing the token outside process memory; launching commands directly (NO subprocess calls from GUI — daemon only); raw KDL text input

## Success Criteria

- Non-expert user: logs in, installs a nix package, restores a snapshot, creates a user, changes a niri setting — all by selection, all with clear confirmations
- On a machine missing btrbk: store works, snapshots panel shows reason+fix instead of errors
- Visual language identical across panels (same spacing, headers, button styles)

## Open Questions

- App id + desktop entry: `org.feralos.Ursine`? (assumed yes)
- Theme: follow COSMIC system theme (dark/light); no custom theme in v1
