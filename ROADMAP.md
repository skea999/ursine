# Roadmap

**Status**: Intent confirmed — see `docs/intent/ursine.md` (source of truth for versions)
**Distribution**: FeralOS (Alpine/musl/dinit/apk)
**Updated**: 2026-10-16

## Milestones

| Ver | Target | Status | What |
|-----|--------|--------|------|
| v0.1 | Skeleton | Planned | Workspace, ursine-api types, ursined skeleton (socket, PAM handshake, capabilities, audit), flake.nix musl, CI (test matrix per feature + flake build/publish) |
| v0.2 | nix module | Planned | install/remove/search/list via nix profiles per asUser |
| v0.3 | Store GUI | Planned | ursine GUI: nix store + snapshots panel |
| v0.4 | Users + niri | Planned | accountsservice panel, niri config.kdl editor with `niri validate` |
| **v1.0** | **v1 release** | — | **backend + GUI complete (nix, users, snapshots, niri), packaged in feralos-aports** |
| v2 | Full coverage | Planned | apk + flatpak; printers (CUPS), scanners (SANE), bluetooth (BlueZ), network (NetworkManager), firewall (firewalld), disks (UDisks2) — all via D-Bus/CLI, capability-gated |
| v3 | OIDC + web | Planned | OIDC auth, web interface (LAST frontend), same UX model; **memory-safe stack required** (Rust→WASM preferred; see SPEC-web) |
| v4 | Incus UI | Planned | Incus management UI in the same product |
| v5 | ZFS + Pirateus | Planned | ZFS + LINSTOR/Pirateus storage management (FeralOS hyperconverged stack) |
| v6 | Multi-distro | Planned | package-manager/init abstraction for other distros; glibc revisited |

## Pipeline (v0.1 sprint)

| Feature | Prio | Status | Notes |
|---------|------|--------|-------|
| Workspace + ursine-api | P0 | Next | serde JSON-lines protocol, Capability, tri-state config |
| ursined skeleton | P0 | Next | unix socket, peer creds, PAM (feature-gated), capabilities scan, JSONL audit |
| flake.nix (musl static) | P0 | Planned | pkgsMusl/pkgsStatic, never glibc |
| CI: test matrix per feature | P0 | Planned | Alpine container, cargo test × {default, pam, web, oidc, full}; clippy+fmt gate |
| CI: build+publish nix flake | P0 | Planned | nix flake check + nix build per push; Release artifacts on tag |
| Local build via Docker Alpine | P0 | Planned | rust:alpine container |
| UI consistency baseline | P1 | Planned | one design language for all UIs (see docs/intent/ursine.md#ui-principles) |

## Timeline

*Schedule TBD — driven by contribution velocity.*

---

**Next review**: at v0.1 (skeleton) completion
