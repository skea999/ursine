# SPEC-gui (ursine)

## Objective

The native frontend `ursine`: GTK4/libadwaita app, thin client of `ursined`. Login, one navigation shell, capability-gated panels (nix store, snapshots, users, niri forms), the `config.init` button. All UIs of Ursine (GUI now, web/Incus later) follow ONE design language: same navigation structure, same terminology (`CONTEXT.md`), libadwaita HIG. User-friendly: no jargon, no broken panels, confirmation before destructive actions.

## Tech Stack

- **relm4** (Elm-style MVC) over `gtk4-rs` + `libadwaita` — decided 2026-10-16 (skea999, option B): relm4 removes GTK boilerplate and gives message-driven components; the `z-rust-gtk4` skill applies fully
- `AdwApplicationWindow`, `adw::NavigationView`/`NavigationSplitView` for the shell
- Async daemon calls via relm4 commands / `oneshot_command` (tokio underneath); GTK main loop never blocked
- Client types ONLY from `ursine-api`

## Screens (v1)

| Screen | Content |
|---|---|
| Login | PAM prompt (user pre-filled from session, password field); on success token kept in process memory only |
| Home/Overview | module cards: enabled (green, opens panel) / disabled (grey + reason + fix command + "Run fix" if trivial) — from `capabilities.list` |
| Store (nix) | search field (results list w/ description), installed list, install/remove/upgrade buttons with progress, per-user selector (asUser dropdown) |
| Snapshots | btrbk list (source, timestamp), "Create snapshot", restore flow (preview diff/stats → confirm dialog typing the snapshot name), btrbk config summary, retention info |
| Users | list (avatar placeholder, name, admin badge), create dialog (name, full name, admin toggle), set password, delete (confirm), groups editor |
| Niri | dynamic form pages rendered from `niri.schema` (sections sidebar: Input, Outputs, Layout, Animations, Keybinds, Gestures, Autostart, Env); Apply bar with validate state; keybind capture widget |
| Settings | `config.init` button (writes daemon config template), daemon status, audit viewer (read-only, latest entries) |

- Destructive actions (restore snapshot, delete user, remove packages): Adw.MessageDialog confirm, stating EXACTLY what will happen
- Capability-gated: disabled modules render as cards with reason + `fix` (e.g. `apk add btrbk`) — never a broken screen

## Commands

```
cargo build -p ursine
cargo run -p ursine
cargo clippy -p ursine -- -D warnings
```

## Project Structure

```
src/ursine/src/
├─ main.rs           AdwApplication + relm4 setup
├─ client.rs         socket client: connect, auth, Bearer calls, progress events (tokio)
├─ state.rs          shared app model: token, capabilities, asUser selection
└─ components/       relm4 components (one per screen)
   ├─ login.rs, home.rs, store.rs, snapshots.rs, users.rs, niri/, settings.rs
```

## Code Style

```rust
#[relm4::component]
impl SimpleComponent for StorePage {
    view! {
        #[root]
        adw::NavigationPage {
            set_title: "Store",
            adw::ToolbarView {
                add_top = &adw::HeaderBar {},
                #[name = "list"]
                gtk::ListBox {},
            }
        }
    }
    // messages -> update() -> model -> view auto-sync via #[watch]
}
```

- One relm4 component per screen; parent-child via `ComponentController` + relays
- Async work NEVER in `update()` — `oneshot_command` returns `CommandOutput`
- UI strings: user-friendly English v1, collected in one module for future i18n; terminology from CONTEXT.md (never "server", always "daemon"; never "backup", always "snapshot")
- No logic: business decisions (what to run, validation beyond form constraints) come from the daemon; GUI only renders + collects

## Testing Strategy

- Unit: `client.rs` protocol handling against a mock socket (dev); state transitions
- UI: manual test checklist per screen in PR template (GTK testing infra is not worth it v1); screenshots in docs for design-language review
- Consistency check: every panel uses the same header/layout components (enforced in review)

## Boundaries

- Always: capability check before showing an action; confirm dialog before destructive ops; spinner/progress for any op >100ms; error toasts with actionable text
- Ask first: new screen; new dependency (any crate beyond gtk4/adw/tokio); changing navigation structure (design language)
- Never: business logic in GUI; storing the token outside process memory; launching commands directly (NO subprocess calls from GUI — daemon only); raw KDL text input

## Success Criteria

- Non-expert user: logs in, installs a nix package, restores a snapshot, creates a user, changes a niri setting — all by selection, all with clear confirmations
- On a machine missing btrbk: store works, snapshots panel shows reason+fix instead of errors
- Visual language identical across panels (same spacing, headers, button styles)

## Open Questions

- App id + desktop entry: `org.feralos.Ursine`? (assumed yes)
- Light/dark: follow libadwaita system style (assumed yes, no custom theme in v1)
