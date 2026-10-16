# Ursine project invariants (binding for every agent)

Read `docs/architecture/overview.md` before any code change. Never violate these:

1. **Memory-safe Rust only.** No `unsafe` blocks unless discussed and approved explicitly.
2. **No polkit.** Auth = PAM handshake (feature `pam`) once per session + RAM-only session tokens. Never write tokens to disk, never store secrets in frontends.
3. **No database. Ever.** The daemon is stateless; system state is read live from apk/flatpak/nix/shadow/btrfs. Audit = JSONL append-only. Config = `/etc/ursine/config.toml` (tri-state `"auto"|true|false`).
4. **Capabilities are runtime-detected**, never persisted. `ensure_available()` before every module action.
5. **musl everywhere**: local builds via Docker with Alpine Linux images; CI builds musl; the nix flake must use musl (pkgsMusl/pkgsStatic), never glibc.
6. **`asUser` discipline**: `flatpak --user` and `nix profile` run as the target user; `apk` and flatpak `--system` are always root. Never mix scopes.
7. **Frontends are thin**: no logic in GUI (`ursine`) or web (`web/`). All logic lives in `ursined` / `ursine-core`. Shared types only via `ursine-api`.
8. **Protocol**: newline-delimited JSON over unix socket; Bearer token on every request after handshake.
9. **CI requirements**: GitHub Actions must build + test every cargo feature combination (musl) and build + publish the nix flake.
10. **Vocabulary**: use `CONTEXT.md` terms exactly (module vs feature vs capability, principal, session token, asUser).
11. **Session protocol**: at session start read `handoff/latest.md` + `state/session-state.json`; at session end update them + `docs/TASKS.md`.
12. **HOST ISOLATION (absolute)**: never modify the host machine — no installs, no nix gc/profile changes, no global tooling, no system commands. ALL builds, tests, dedup, security checks, dependency checks and maintenance run inside Docker containers (alpine for cargo, nixos/nix for flake). Container volumes: project root mounted READ-ONLY, only `artifacts/` writable. Temp files NEVER in host `/tmp` — always `artifacts/<tool-or-task>/` (gitignored). Keep only the last 3 artifact versions per subfolder. Host nix store GC only via the nixos/nix container, never with `-d`.
13. **Language**: everything written in this repo (code, docs, scripts, comments, commit messages) is ENGLISH only.
14. **Conventions**: change logs go to `CHANGELOG.md` (never AGENTS.md/handoff). Agents stay FLAT in `.opencode/agents/` (nested pack dirs do not load — after any installer run, re-promote new agents to flat).
