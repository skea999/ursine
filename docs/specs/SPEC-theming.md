# SPEC — Theming (uniform desktop)

Status: **implemented 2026-10-27** (backend + GUI; live probe against a real
DMS config green; real FeralOS/KDE/Qt/Wayle smoke pending). Application top
bar: planned, specs pending from the user.

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

Capability probe: ANY login user (uid 1000..60000, same range as the module's
identity check) has a shell config; `theme_detect`/`theme_get`/`theme_apply`
resolve the session principal (or `asUser` equal to it — cross-user theming is
rejected on the wire).

## Snapshot

`theming.get` normalizes both sources into one `ThemeSnapshot`:

- `source`: `wayle` | `dms`
- colors: background, foreground, primary/accent, secondary, surface,
  error/destructive, success, warning, outline (hex)
- `dark`: bool (from palette luminance / source flag)
- fonts: ui family, mono family, weight, scale (Wayle exposes none: empty
  families mean "leave the configured fonts alone" — writers skip font keys)
- corner radius, icon theme, cursor theme + size

Source extraction:

- **DMS**: `settings.json` (fonts, icon, cursor, radius, theme name) + palette
  from the generated `@define-color` rules in
  `~/.config/gtk-3.0/dank-colors.css` (DMS/matugen own that file).
- **Wayle**: `config.toml` `[styling]` palette tokens and providers.

## Targets

All `theming.apply` writes are user-scoped via `asUser`, atomic, audit-logged,
and sit behind an explicit GUI confirmation step. Every propagated label
(font/icon/cursor) is validated (no control bytes, no `=`) before entering an
INI or env file.

| # | Technology | Artifact | Gate |
|---|---|---|---|
| 1 | GTK3 | `~/.config/gtk-3.0/settings.ini` (theme/font/icon/cursor); `gtk.css` written only when absent or marker-owned (shell/hand-written CSS never clobbered, regardless of source) | always |
| 2 | GTK4 | same as GTK3 under `~/.config/gtk-4.0/` | always |
| 3 | GTK2 | `~/.gtkrc-2.0` (theme/font/icon/cursor) | always |
| 4 | Qt5 | `~/.config/qt5ct/colors/ursine-<source>.conf` + `qt5ct.conf` `colorScheme=` | qt5ct config dir/tool present |
| 5 | Qt6 | same under `~/.config/qt6ct/` | qt6ct present |
| 6 | KDE | `~/.local/share/color-schemes/ursine-<source>.colors` + direct merged writes into `kdeglobals` (ColorScheme, Icons, cursor, Font) | kwriteconfig5/6 binary, `kdeglobals`, or color-schemes dir present |
| 7 | Flatpak | merge `filesystems=…:ro` entries into `~/.local/share/flatpak/overrides/global` for `xdg-config/gtk-3.0`, `gtk-4.0`, `qt5ct`, `qt6ct` (read-only mounts so sandboxed apps see host theme) | flatpak present |
| 8 | libcosmic / COSMIC apps | patch a full `cosmic_theme::Theme` from the snapshot (8-digit hex for ColorRepr) and `write_entry` into `com.system76.CosmicTheme.Dark`/`.Light` + `Mode.is_dark` via `Config::with_custom_path` (systemic: all COSMIC windows follow after restart) | always (self-sufficient: creates its config tree) |
| 9 | Session env | `~/.config/environment.d/95-ursine-theme.conf`: `XCURSOR_THEME`, `XCURSOR_SIZE`, `ICON_THEME`, `QT_QPA_PLATFORMTHEME` (qt6ct/qt5ct when those tools exist; KDE apps read kdeglobals themselves) | always (best-effort: session restart needed; note in GUI) |
| 10 | Portal | no direct file: the settings portal derives color/font from the GTK/Qt/KDE layers above (reported as `skipped` with that reason) | portal present |

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

## Native three-button baseline (permanent, all desktops — decided 2026-10-28)

Every application — GNOME/libadwaita included — must show the classic
trio **minimize, maximize, close**, and the three must look as similar as
each toolkit allows (colours/sizes follow the shell palette; pixel-identical
shapes are NOT required, per the binding UI rule). This baseline stays in
effect permanently, including when `ursine-titlebar` exists (that app is
niri-only and overlays its four buttons there; the configured baseline keeps
every other desktop and every non-covered window correct).

| Stack | Mechanism |
|---|---|
| GTK3 | `gtk-decoration-layout=:minimize,maximize,close` in our `settings.ini` merge + restyle rules for `.titlebutton` in the CSS we own |
| GTK4 + libadwaita (GNOME apps) | same `gtk-decoration-layout` key + `windowcontrols` restyle rules in the CSS we own; libadwaita follows the system layout |
| KDE/kwin | `kdeglobals [WM] ButtonsOnLeft=` (empty) + `ButtonsOnRight=IAX` (KDecoration letters: I=minimize, A=maximize, X=close) |
| Qt CSD / Electron / libcosmic | already draw the trio by default → no functional key; colours follow their own palette (Breeze/chromium/cosmic — partial styling only) |
| Terminals without decorations (alacritty …) | nothing to configure; covered later by `ursine-titlebar` |

Notes:

- **niri caveat**: niri has no minimized state — the native minimize click is
  inert until `ursine-titlebar` replaces it (see below). The baseline config
  is still written so KDE/other desktops and future coverage stay correct.
- **GUI toggle**: the Theming section shows "Show minimize button"
  (checkbox); `theme_detect` reads the current state back from
  `settings.ini`, `theme_apply { show_minimize }` writes it into the GTK
  layout, the live `button-layout` key and the kwin letters (`IAX`/`AX`).
- **Grafica**: when we own `gtk.css` (Wayle source / no shell CSS) the
  window-control buttons get our uniform restyle (size, radius, palette
  alpha); when the shell owns it (DMS), the buttons still follow the shell
  palette through the shell CSS. Rest of the stacks: palette-level similarity
  only.

## Application title bar — `ursine-titlebar` (planned, separate repo)

Specs from the user 2026-10-28; reference behaviour:
`github.com/skea999/dms-window-buttons` (actions only — NOT a bar plugin).
Status: **planned only** — do not scaffold the repo yet. Ursine configures
it; the drawing lives in its own repository/app.

### Requirement

Keep every application's own title bar (headerbar, GNOME menus, Qt titlebar)
visible. Remove the native buttons and make the buttons identical for every
toolkit (GTK3/4, Qt, libcosmic, Electron):

- **niri (v1)**: four buttons on the title bar's top-right corner of **every
  window, focused or not**, right-to-left: close (rightmost), maximize,
  move-workspace-down, move-workspace-up. Behaviour: `niri msg action`
  `close-window` / `maximize-column` / `move-column-to-workspace-down` /
  `move-column-to-workspace-up`, socket self-resolved
  (`$XDG_RUNTIME_DIR/niri*.sock`), exactly like the reference plugin.
  The move-workspace-down button IS the working minimize: it occupies the
  classic minimize role and moves the app to the workspace below (niri has
  no minimized state and native `set_minimized` cannot be remapped by any
  config); move-workspace-up restores it.
- The native three-button baseline above stays configured at all times,
  also when `ursine-titlebar` runs (the strip overlays the native area on
  niri; the baseline governs every other desktop).
- **Other compositors (at most COSMIC, KDE, GNOME — decided later)**: the
  classic three (minimize, maximize, close). Not in v1.

### Mechanism (analysis 2026-10-28)

Wayland forbids drawing into another client's window, so no code can be
injected into apps; per-toolkit config can only *remove* buttons, never add
move-workspace buttons. The only viable mechanism is **layer-shell overlay
surfaces from one user-session process**:

- `ursine-titlebar` = one process (started with the user session via
  autostart/dinit user unit — never inside root `ursined`), **one overlay
  surface per window**, anchored with top/left margins to that window's
  title-bar geometry, opaque background in the title-bar colour covering the
  native button area (this also "removes" buttons that no config can hide,
  e.g. libcosmic header buttons), while the rest of the title bar and all
  menus stay untouched and clickable underneath.
- Window list, geometry, focus and open/close come from the niri event
  stream; actions via `niri msg action` (reference plugin's approach).
- Rust + libcosmic/iced layer-shell (verified present in the pinned fork).
- Its own TOML config with hot reload, its own CI, v1 targets niri only;
  future backends limited to cosmic/kde/gnome (user decision).

### Division of labour

| Component | Owns |
|---|---|
| `ursine-titlebar` (new repo) | surfaces per window, geometry tracking, drawing, niri IPC, autostart, its config format, hot reload |
| Ursine `theming` module | remove native buttons where config allows (GTK3/4 CSS marker block, KDE `[WM]`, Electron); detect the binary (capability); write `~/.config/ursine-titlebar/config.toml` from the shell snapshot (`enabled`, `buttons` per compositor — 4 on niri, 3 elsewhere —, `[style] background/foreground/accent/radius/height`); GUI toggle + status in the Theming section |

Contract (Ursine is the only writer of style/button keys):

```toml
# ~/.config/ursine-titlebar/config.toml
enabled = true
buttons = ["move-up", "move-down", "maximize", "close"]   # niri; others: ["minimize", "maximize", "close"]
[style]
background = "#rrggbb"
foreground = "#rrggbb"
accent = "#rrggbb"
radius = 12
height = 28
```

### Sequencing note

Removing native min/max via config takes effect before the title-bar app
exists: until `ursine-titlebar` ships, minimized/maximized clicks are gone
(keyboard binds still work). Ship the config removal behind the same Apply
step so it moves with the feature.

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
