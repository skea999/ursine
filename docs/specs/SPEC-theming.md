# SPEC — Theming (uniform desktop)

Status: **planned** (user decision 2026-10-27). Application top bar: planned,
specs pending from the user.

## Goal

One visual identity across **all** applications, regardless of toolkit: GTK3,
GTK4, legacy GTK2, Qt5/Qt6 (qt5ct/qt6ct), KDE (kdeglobals/color-schemes),
Flatpak sandboxes, libcosmic/COSMIC apps, and session-level environment
(cursors, icon theme, platform theme). The user must not be able to tell the
toolkit apart. The theme source is the user's shell config; Ursine is the
propagator, never the source of truth.

## Source selection

1. Detect configured shells by config presence (target user's home):
   - Wayle: `~/.config/wayle/config.toml` (`[styling]`)
   - DankMaterialShell: `~/.config/DankMaterialShell/settings.json`
2. If both exist: the shell **running as the same uid** that opened the Ursine
   session (principal) wins. If neither runs, DMS wins (deterministic default).
3. If neither config exists: capability `theming` is unavailable — the GUI
   section stays hidden/inactive. No writes, ever.

## Snapshot

`theming.get` normalizes both sources into one `ThemeSnapshot`:

- `source`: `wayle` | `dms`
- colors: background, foreground, primary/accent, secondary, surface,
  error/destructive, success, warning, outline (hex)
- `dark`: bool (from palette luminance / source flag)
- fonts: ui family, mono family, weight, scale
- corner radius, icon theme, cursor theme + size

Source extraction:

- **DMS**: `settings.json` (fonts, icon, cursor, radius, theme name) + palette
  from the generated `@define-color` rules in
  `~/.config/gtk-3.0/dank-colors.css` (DMS/matugen own that file).
- **Wayle**: `config.toml` `[styling]` palette tokens and providers.

## Targets

All `theming.apply` writes are user-scoped via `asUser`, atomic, audit-logged,
and destructive parts sit behind GUI confirmation.

| # | Technology | Artifact | Gate |
|---|---|---|---|
| 1 | GTK3 | `~/.config/gtk-3.0/settings.ini` (theme/font/icon/cursor); CSS import only when DMS does not already own `gtk.css` | always |
| 2 | GTK4 | `~/.config/gtk-4.0/settings.ini` + `gtk.css` import (same rule as GTK3) | always |
| 3 | GTK2 | `~/.gtkrc-2.0` (theme/font/icon/cursor) | always |
| 4 | Qt5 | `~/.config/qt5ct/colors/ursine-<source>.conf` + `qt5ct.conf` `colorScheme=` | qt5ct config dir/tool present |
| 5 | Qt6 | same under `~/.config/qt6ct/` | qt6ct present |
| 6 | KDE | `~/.local/share/color-schemes/ursine-<source>.colors` + `kwriteconfig5` `kdeglobals` (ColorScheme, cursor, font) | `kwriteconfig5` present |
| 7 | Flatpak | merge `filesystems=…:ro` entries into `~/.local/share/flatpak/overrides/global` for `xdg-config/gtk-3.0`, `gtk-4.0`, `qt5ct`, `qt6ct` (read-only mounts so sandboxed apps see host theme) | flatpak present |
| 8 | libcosmic / COSMIC apps | write custom entry `com.system76.CosmicTheme.*` + set `com.system76.CosmicTheme.Mode` active theme (systemic: all COSMIC windows follow) | always (cosmic-config present with COSMIC apps) |
| 9 | Session env | `~/.config/environment.d/95-ursine-theme.conf`: `XCURSOR_THEME`, `XCURSOR_SIZE`, `ICON_THEME`, `QT_QPA_PLATFORMTHEME` (qt6ct/qt5ct/KDE per gate), `GTK_THEME` (only when Ursine owns the CSS path) | always (best-effort: session restart needed; note in GUI) |
| 10 | Portal | appearance/prefer-dark via portal config where supported | portal present |

**DMS and Wayle themselves are excluded**: their own config files are the
source we read; never write them.

## Application themes (discovery 2026-10-27 on the test VM)

The shell does NOT theme these; Ursine must. Per-app targets, each gated on
the app's config/profile actually existing (write only what is there or
directly creatable for an installed binary):

| App class | Mechanism | Gate |
|---|---|---|
| Firefox | profile `chrome/userChrome.css` (import/embed the palette) + prefs `toolkit.legacyUserProfileCustomizations.stylesheets=true`, `ui.systemUsesDarkTheme`, ui font — via `user.js` in the profile | a Firefox profile exists (`profiles.ini`) |
| VSCodium / VSCode family | merge into `User/settings.json`: `workbench.colorCustomizations`, `terminal.integrated.*` palette; title bar styling deferred to the top-bar spec | `~/.config/VSCodium` (or Code) exists |
| Terminals | **alacritty**: `alacritty.toml` `import = ["dank-theme.toml"]` (DMS file exists but is never imported — create the import; for wayle source write `[colors]` directly); **kitty** `include`; **ghostty** `theme`/colors; **foot** `include`; **wezterm** — same pattern | that terminal's config file/dir exists |
| cava | `~/.config/cava/config` `[color]` / theme section | config exists |
| Generic Electron | best-effort `electron-flags.conf` (`--gtk-version=4`) so Electron follows GTK; colors only via per-app settings when available | flags file writable / app present |
| Everything else | covered by the toolkit layers above (GTK/Qt/KDE/portal/env) — nautilus, LibreOffice, KeepassXC, missioncenter, filezilla, remmina, eog, zim, galculator, simple-scan … | — |

**Out of scope (declared)**: DBeaver/Eclipse SWT workbench themes (proprietary
XML theme system) — optional candidate only if requested later.

All per-app merges are idempotent (JSON/TOML round-trip merge, Ursine-owned
keys marked), atomic, and reported per-target in the apply result.

## Application top bar (planned)

A uniform application title bar across all applications (specs pending from
the user). Same goal — the bar must look identical regardless of toolkit.
Block until specs arrive; keep the section in the GUI plan.

## Rules

- Thin frontend: GUI only shows snapshot + status + triggers `detect/get/apply`.
- `ensure_available()` capability per action; every action audited.
- Never read or write shell secrets; only theme configuration files.
- Writes are atomic (tmp + rename) and idempotent; `apply` reports per-target
  results (`written` / `skipped (tool absent)` / `failed`).
- Preview first (snapshot shown in the GUI), apply behind confirmation.
- Parity: backend and GUI ship in the same task.

## Verification (this VM, user-approved)

DMS 1.4.6 is present and running here; Wayle absent. Live probe order:
`detect` → `get` (compare palette against `dank-colors.css`) → `apply` →
assert written files (settings.ini, environment.d, overrides/global, cosmic
entry) and per-target status. All writes reversible; stop on any mismatch.

## Tests

- Unit: DMS settings + `@define-color` parser, Wayle toml parser, snapshot
  normalization (dark detection), flatpak override merge, source precedence
  (running-shell wins by uid).
- Unit: apply writers against temp dirs (idempotency, atomicity, gates).
- Unit: application writers — firefox profile detection + user.js prefs,
  codium settings.json merge, alacritty import creation, cava config merge.
- GUI: section hidden when capability off; apply flow shows per-target results.
