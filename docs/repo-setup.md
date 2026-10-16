# Repo Setup

How this repo was created from the skea-store template — kept for reference. Setup is DONE; see `docs/guides/getting-started.md` for daily work.

## 1. Clone (done 2026-10-16)

```bash
git clone <template-url> ursine
cd ursine
git remote remove origin
git remote add origin <ursine-repo-url>
```

## 2. Git hooks (done)

`.githooks/pre-commit` recreated for this repo (secrets scan + `cargo fmt --check`).

```bash
git config core.hooksPath .githooks && chmod +x .githooks/pre-commit
```

## 3. Session memory (adapted)

The template's `session-memory` skill is PowerShell-only and not installed on this machine. Replaced by the file-based protocol in `AGENTS.md#Session-protocol`: memory = `handoff/latest.md` + `state/session-state.json`, updated by the agent at session start and end.

## 4. Stack decision (done)

Rust workspace. See `AGENTS.md#Stack`, `docs/architecture/overview.md`.

## Template leftovers still to clean

- template `src/` subdirs (`components/services/types/utils`, `.gitkeep` only) — remove when workspace lands (workspace members go in `src/ursine-*`)
- `docs/{adr,api,ideas,intent}/` — empty placeholders (intent now filled: `docs/intent/ursine.md`), fine to keep

## Harness alignment (2026-10-16)

Hidden harness dirs were missing from the initial copy (`.agents/`, `.opencode/`, `.githooks/`, `.github/`, `.agent-memory/`, `.vscode/`, `.editorconfig`, `.env.example`). Re-aligned from `/home/gab/projects/skea-store`:
- `.agents/skills/` — universal workflow skills (spec-driven-development, interview-me, planning, TDD, security scans…)
- `.opencode/` — 20 agents, commands, instructions (incl. `rust.instructions.md`), local plugin, 5 template rules + `ursine.md` (ours), skills (incl. `rust`)
- `.githooks/pre-commit` — template version + Ursine cargo fmt gate appended
- `.github/` — issue/PR templates; `.vscode/`, `.editorconfig`, `.env.example` — as template
- `.gitignore` — template + `/target` + `/artifacts`
