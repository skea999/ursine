---
name: z-rust-gtk4
description: Build GTK4 desktop apps in Rust with relm4 and libadwaita — the Elm-style component loop, libadwaita widget hierarchy, preferences dialogs, system tray, message-driven UI patterns. Use when asked to "build a GTK4 app in Rust", "create a libadwaita UI", "use relm4", "implement a GTK4 widget", "add system tray", or the request mentions AdwApplicationWindow, AdwPreferencesPage, or message-driven GTK4 UI. Does not cover general Rust ownership/error/async patterns; see [[z-rust-core]].
---

# Rust GTK4 (relm4 + libadwaita)

Build HIG-compliant GTK4 desktop apps with the relm4 Elm-style loop: message-driven, composable components with clean separation between model state and view rendering.

## Core Architecture: relm4 Elm Pattern

relm4 implements an Elm-like model-update-view loop:

- **`Component::init()`** — construct initial model state + build widget tree
- **`Component::update()`** — receive a typed message, mutate model
- **`view!` macro** — declarative widget tree; auto-syncs to model on `#[watch]` fields

The `#[relm4::component]` macro generates the boilerplate; the `view!` macro is the widget DSL.

## Key Concepts

### Sync vs Async Components

| Trait | When to use |
|---|---|
| `SimpleComponent` | No async work; instant model updates |
| `Component` | Background commands via `type CommandOutput` + `sender.oneshot_command(...)` — the pattern used everywhere below |
| `AsyncComponent` | `update()` itself is async; reach for it only if `Component` + `oneshot_command` genuinely can't fit |

### Message Flow

```
User action (signal) → sender.input(Msg::X)         → update()     → model mutates → view re-renders
Background task      → sender.oneshot_command(...)  → update_cmd() → sender.input(...) / model mutates
```

### Background Work Pattern

Never block `update()`. Dispatch an async block with `sender.oneshot_command()`; the block's return value becomes the `CommandOutput` message, handled in `update_cmd()`:

```rust
fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
    match msg {
        Msg::Refresh(id) => {
            let svc = self.service.clone();
            sender.oneshot_command(async move {
                match svc.fetch(id).await {
                    Ok(data) => CmdOut::FetchDone(id, data),
                    Err(e) => CmdOut::FetchFailed(id, e),
                }
            });
        }
    }
}

fn update_cmd(&mut self, msg: Self::CommandOutput, sender: ComponentSender<Self>, _root: &Self::Root) {
    match msg {
        CmdOut::FetchDone(id, data) => self.apply(id, data),
        CmdOut::FetchFailed(_, err) => {
            let _ = sender.output(Output::Notice(err.to_string()));
        }
    }
}
```

## libadwaita Widget Hierarchy

Real apps compose the shell by hand rather than reaching for `AdwToolbarView`:

```
AdwApplicationWindow
└── gtk::Box (vertical)
    ├── AdwHeaderBar
    └── AdwToastOverlay
        └── [primary content]
```

The example uses a vertical `gtk::Box`; `AdwToolbarView` is another supported shell. Preserve the target application's composition unless a concrete layout need warrants changing it.

### Preferences Dialog Structure

```
AdwPreferencesDialog
└── AdwPreferencesPage (tab)
    └── AdwPreferencesGroup (section)
        ├── AdwActionRow (entry, switch suffix)
        └── AdwSwitchRow
```

```rust
let dialog = adw::PreferencesDialog::new();
dialog.set_title("Preferences");
dialog.add(&page);       // one add() per AdwPreferencesPage
dialog.present(Some(&parent_window));
```

`AdwPreferencesWindow` is deprecated since libadwaita 1.6 in favor of `AdwPreferencesDialog` (same page/group/row API, presented as a dialog instead of a standalone window). Only reach for `PreferencesWindow` when targeting pre-1.6 libadwaita.

### Empty + Loading States

```rust
// Empty state
adw::StatusPage {
    set_icon_name: Some("document-open-symbolic"),
    set_title: "No Items",
    set_description: Some("Add an item to get started"),
}

// Inline alert
adw::Banner {
    set_title: "Connection failed",
    set_revealed: model.has_error,
    set_button_label: Some("Retry"),
    connect_button_clicked[sender] => move |_| sender.input(Msg::Retry),
}
```

### Toast Notifications

```rust
adw::ToastOverlay {
    // wraps main content; show toasts programmatically:
}

// In update():
self.toast_overlay.add_toast(
    adw::Toast::new("Settings saved")
);
```

## Signal → Message Pattern

All signals close over `sender` and call `sender.input()`:

```rust
gtk::Button {
    set_label: "Connect",
    connect_clicked[sender] => move |_| {
        sender.input(AppMsg::Connect);
    }
}

gtk::Entry {
    connect_changed[sender] => move |entry| {
        sender.input(AppMsg::SetUrl(entry.text().to_string()));
    }
}

adw::SwitchRow {
    set_title: "Auto-restart",
    set_active: model.auto_restart,
    connect_active_notify[sender] => move |row| {
        sender.input(AppMsg::SetAutoRestart(row.is_active()));
    }
}
```

## System Tray Integration

Use `ksni` — a pure-Rust StatusNotifierItem implementation, no GTK/AppIndicator dependency. Implement `Tray` on a plain struct, spawn it to get a `Handle<T>`, and mutate state through `handle.update()`:

```rust
use ksni::{Handle, Tray, TrayMethods};
use ksni::menu::{MenuItem, StandardItem};

struct AppTray {
    connected: bool,
    on_action: Arc<dyn Fn(TrayAction) + Send + Sync>,
}

impl Tray for AppTray {
    fn id(&self) -> String {
        "myapp".into()
    }

    fn icon_name(&self) -> String {
        if self.connected { "myapp-connected" } else { "myapp-disconnected" }.into()
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let cb = Arc::clone(&self.on_action);
        vec![StandardItem {
            label: "Quit".into(),
            activate: Box::new(move |_| (cb)(TrayAction::Quit)),
            ..Default::default()
        }
        .into()]
    }
}

let handle: Handle<AppTray> = AppTray { connected: false, on_action }.spawn().await?;

handle.update(|tray| tray.connected = true).await;
```

Choose a tray implementation from the target desktop's supported protocols and the application's existing dependencies. Verify behavior on each supported desktop; no private anchor project establishes a universal choice here.

## Cargo.toml Dependencies

Prefer relm4's compatible re-exports where they cover the required APIs. Direct gtk/glib/gio dependencies can be legitimate; keep versions compatible and avoid duplicate incompatible crate types. The dependency block below is illustrative, not a current-version recommendation:

```toml
[dependencies]
relm4 = { version = "0.11", features = ["gnome_49", "libadwaita"] }
tokio = { version = "1", features = ["full"] }
ksni  = "0.3"
```

```rust
use relm4::gtk;
use relm4::adw;
```

Inspect the installed relm4 feature table and the minimum GTK/libadwaita runtime shipped on supported systems. Choose only available features that fit that runtime; do not infer a feature name from the calendar or GNOME release number.

## Edge Cases

If a component needs to talk to a sibling: use `ComponentSender::output()` to emit upward; parent routes messages downward.

If the view needs to reference a named widget outside the macro: use `#[name = "my_widget"]` annotation inside `view!`.

If GTK panics with "assertion failed: is_main_thread": ensure all GTK calls happen on the GLib main thread — never from `tokio::spawn`.

If an API needs a newer runtime, first decide whether raising the minimum supported platform is in scope. Then update compatible crate features and verify on that minimum runtime.

For deep widget patterns, component composition, and real-world relm4 structure: see the reference files below.

## References

- [Component Patterns](references/component-patterns.md)
- [libadwaita Widget Catalog](references/adwaita-widgets.md)
