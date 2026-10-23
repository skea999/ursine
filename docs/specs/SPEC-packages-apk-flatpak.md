# SPEC-packages-apk-flatpak

## Objective

Expose Alpine (`apk`) and Flatpak packages in the same thin-client control
center as the Nix store. Both backends are capability-gated and audited.

## API surface

| Action | Backend |
|---|---|
| `list_apk` | `apk info -v` + `apk info` (name/version pairing) |
| `search_apk` | `apk search -v <query>` |
| `refresh_apk` | `apk update` |
| `install_apk` | `apk add --no-progress <package>` |
| `remove_apk` | `apk del <package>` |
| `upgrade_apk` | `apk upgrade --no-progress` |
| `list_flatpak` | `flatpak list --columns=... --system/--user` |
| `search_flatpak` | `flatpak search --columns=... --system/--user <query>` |
| `install_flatpak` | `flatpak install -y --noninteractive <scope> flathub <app>` |
| `remove_flatpak` | `flatpak uninstall -y --noninteractive <scope> <app>` |
| `update_flatpak` | `flatpak update -y --noninteractive <scope>` |

- APK is system scope: root only.
- Flatpak has system and user scope; user scope runs as the authenticated
  principal.
- The GUI never shells out. It renders typed rows from `ursine-api`.

## Security

- Absolute trusted tool paths, argv only, no shell.
- Package IDs and search queries are validated before execution.
- Output is bounded by the shared exec helper.
- Destructive actions require GUI confirmation.
- Capability gates: `apk` binary; `flatpak` binary.
- Audit uses the normal module/action path; no secrets are logged.

## GUI

Sidebar entries **APK packages** and **Flatpak applications**. Each panel has:

- source dropdown (APK/Flatpak);
- Flatpak system/user dropdown;
- search and direct-install fields;
- installed/search rows with install/remove actions;
- refresh/update controls with confirmations.

The overall shell uses a GNOME Settings-style left sidebar for every logged-in
screen.

## Testing

- Unit: APK package/query validation, Flatpak id validation, scope flags.
- `cargo test --workspace --all-features`.
- Manual: install/remove a harmless package on Alpine and Flatpak; verify
  capability-gated states when binaries are absent.
