# Ursine

Domain vocabulary for the Ursine project. Use these terms consistently across code, docs, and conversations.

**Ursine** is the control center of FeralOS: a root daemon with pluggable frontends (native GUI, web), in the spirit of YaST/Cockpit/Webmin but memory-safe (Rust) and desktop-first.

## Terms

**Daemon** (`ursined`):
The root process, dinit service, owner of ALL logic. Frontends are thin clients.
_Avoid_: server, backend service, helper

**Frontend**:
GUI (`ursine`, GTK4/libadwaita) or web (`ursine-web`, static assets). No logic, no policy decisions.
_Avoid_: client app, app (ambiguous)

**Module**:
A functional area inside the daemon (e.g. `packages.apk`, `snapshots`, `users`, `niri`).
_Avoid_: feature (reserved for compile-time cargo features), plugin

**Capability**:
A runtime-detected fact about the machine (e.g. btrbk installed, btrfs filesystem present). Never persisted, re-checked per request.
_Avoid_: feature flag

**Feature**:
Compile-time cargo feature controlling what is in the binary (`web`, `oidc`).
_Avoid_: capability (that means runtime detection)

**Tri-state config**:
The per-module setting `"auto" | true | false` in `/etc/ursine/config.toml`. `auto` = detection decides.
_Avoid_: enabled/disabled boolean

**Principal**:
The authenticated identity of a requester: local PAM user or future OIDC subject. Every audit entry records it.
_Avoid_: user (ambiguous with system users), session

**Session token**:
RAM-only Bearer token issued by the daemon after successful auth (PAM handshake or, later, OIDC). Never on disk.
_Avoid_: API key, cookie

**PAM handshake**:
The one-time credential check at session start, done by the daemon (service `ursine`), not by frontends.
_Avoid_: login (reserved for the web form flow)

**asUser**:
Optional request field selecting the target unix user for per-user operations (`flatpak --user`, `nix profile`). Defaults to the principal. `apk` and `--system` operations ignore it.
_Avoid_: runAs, sudo

**Snapshot**:
A btrfs snapshot managed by the snapshots module (btrbk/btrfs). Owned exclusively by this module (manual + automatic pre-update). 
_Avoid_: backup

**Audit log**:
Append-only JSONL recording every action with principal, timestamp, request. 
_Avoid_: history, log file (generic)
