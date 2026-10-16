# libadwaita Widget Catalog

## Window Patterns

### AdwApplicationWindow — Box + HeaderBar + ToastOverlay example

This example composes a vertical `gtk::Box` holding a `HeaderBar` and a
`ToastOverlay`. `AdwToolbarView` is another supported composition; preserve
the target application's conventions.

```rust
view! {
    adw::ApplicationWindow {
        set_title: Some("App"),
        set_default_width: 900,
        set_default_height: 600,

        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,

            adw::HeaderBar {
                pack_start = &gtk::Button {
                    set_icon_name: "list-add-symbolic",
                    connect_clicked[sender] => move |_| sender.input(Msg::Add),
                },
                pack_end = &gtk::MenuButton {
                    set_icon_name: "open-menu-symbolic",
                    set_menu_model: Some(&main_menu),
                },
            },

            adw::ToastOverlay {
                #[wrap(Some)]
                set_child = &gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    // primary content
                }
            }
        }
    }
}
```

### AdwToolbarView (alternative — available, not the pattern in use here)

```rust
view! {
    adw::ApplicationWindow {
        adw::ToolbarView {
            add_top_bar = &adw::HeaderBar { },

            #[wrap(Some)]
            set_content = &gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                // primary content
            }
        }
    }
}
```

---

## Preferences Dialog

### AdwPreferencesDialog (current — AdwPreferencesWindow is deprecated since libadwaita 1.6)

```rust
view! {
    adw::PreferencesDialog {
        set_title: "Preferences",

        add = &adw::PreferencesPage {
            set_title: "General",
            set_icon_name: Some("preferences-system-symbolic"),

            add = &adw::PreferencesGroup {
                set_title: "Network",
                set_description: Some("Configure proxy backend"),

                // Text entry row
                add = &adw::EntryRow {
                    set_title: "Server URL",
                    set_text: &model.server_url,
                    connect_changed[sender] => move |row| {
                        sender.input(Msg::SetUrl(row.text().to_string()));
                    }
                },

                // Switch row (replaces AdwActionRow + Switch)
                add = &adw::SwitchRow {
                    set_title: "Enable TLS",
                    set_subtitle: "Encrypt proxy traffic",
                    set_active: model.tls_enabled,
                    connect_active_notify[sender] => move |row| {
                        sender.input(Msg::SetTls(row.is_active()));
                    }
                },

                // Combo row for enums
                add = &adw::ComboRow {
                    set_title: "Protocol",
                    set_model: Some(&protocol_model),
                    set_selected: model.protocol_idx,
                    connect_selected_notify[sender] => move |row| {
                        sender.input(Msg::SetProtocol(row.selected()));
                    }
                },
            },

            add = &adw::PreferencesGroup {
                set_title: "Appearance",

                add = &adw::ActionRow {
                    set_title: "Theme",
                    set_subtitle: "Light, Dark, or system default",

                    add_suffix = &gtk::DropDown {
                        set_model: Some(&theme_model),
                        connect_selected_item_notify[sender] => move |dd| {
                            sender.input(Msg::SetTheme(dd.selected()));
                        }
                    }
                }
            }
        }
    }
}
```

---

## Navigation Patterns

### Split View (sidebar + detail)

```rust
view! {
    adw::NavigationSplitView {
        set_sidebar_width_fraction: 0.35,

        #[wrap(Some)]
        set_sidebar = &adw::NavigationPage {
            set_title: "Items",

            adw::ToolbarView {
                add_top_bar = &adw::HeaderBar {},

                #[wrap(Some)]
                set_content = &gtk::ScrolledWindow {
                    gtk::ListBox {
                        set_selection_mode: gtk::SelectionMode::Single,
                    }
                }
            }
        },

        #[wrap(Some)]
        set_content = &adw::NavigationPage {
            set_title: "Detail",

            adw::ToolbarView {
                add_top_bar = &adw::HeaderBar {},
                // detail view
            }
        }
    }
}
```

---

## Dialogs

### Alert Dialog (destructive action)

```rust
let dialog = adw::AlertDialog::builder()
    .heading("Delete subscription?")
    .body("This cannot be undone.")
    .build();

dialog.add_responses(&[
    ("cancel", "Cancel"),
    ("delete", "Delete"),
]);
dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
dialog.set_default_response("cancel");
dialog.set_close_response("cancel");

let sender = sender.clone();
dialog.connect_response(None, move |_, response| {
    if response == "delete" {
        sender.input(Msg::ConfirmDelete);
    }
});

dialog.present(Some(root));
```

---

## Status & Feedback

### Toast Notifications

```rust
view! {
    adw::ToastOverlay {
        #[name = "toast_overlay"]

        // main content inside the overlay
        set_child = Some(&gtk::ScrolledWindow { })
    }
}

// In update():
self.toast_overlay.add_toast(
    adw::Toast::builder()
        .title("Settings saved")
        .timeout(3)
        .build()
);
```

### Banner (inline alert)

```rust
view! {
    gtk::Box {
        set_orientation: gtk::Orientation::Vertical,

        adw::Banner {
            #[watch]
            set_title: model.error.as_deref().unwrap_or(""),
            #[watch]
            set_revealed: model.error.is_some(),
            set_button_label: Some("Retry"),
            connect_button_clicked[sender] => move |_| sender.input(Msg::Retry),
        },

        // main content
    }
}
```

---

## Common Patterns

### Expandable Row

```rust
add = &adw::ExpanderRow {
    set_title: "Advanced",

    add_row = &adw::ActionRow {
        set_title: "Timeout",
        add_suffix = &gtk::SpinButton {
            set_range: (1.0, 60.0),
            set_increments: (1.0, 5.0),
            set_value: model.timeout as f64,
        }
    }
}
```

### Clamp (responsive width limit)

```rust
adw::Clamp {
    set_maximum_size: 600,
    set_tightening_threshold: 400,

    // content won't exceed 600px
    set_child = Some(&form_box),
}
```

### Spinner in HeaderBar (loading indicator)

```rust
add_top_bar = &adw::HeaderBar {
    pack_end = &gtk::Spinner {
        #[watch]
        set_spinning: model.loading,
        #[watch]
        set_visible: model.loading,
    }
}
```

---

## libadwaita Style Classes

Apply to GTK widgets for semantic styling:

```rust
widget.add_css_class("suggested-action");  // blue primary button
widget.add_css_class("destructive-action"); // red destructive button
widget.add_css_class("flat");               // flat/borderless button
widget.add_css_class("card");               // card background
widget.add_css_class("dim-label");          // secondary text
widget.add_css_class("caption");            // small caption text
widget.add_css_class("title-1");            // large title
widget.add_css_class("error");              // error state (red)
widget.add_css_class("warning");            // warning state (yellow)
widget.add_css_class("success");            // success state (green)
```
