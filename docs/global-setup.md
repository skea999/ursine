# Global Setup — OpenCode AI Environment

Guide for an AI on how to install and configure the global opencode environment on a new machine.

Last verified: 2026-10-16.

## Version snapshot

| Component | Version |
|-----------|---------|
| OpenCode CLI | 1.15.10 (V1) |
| Node.js | 24.18.1 (≥ 22.5 required by context-mode) |
| caveman + cavecrew | latest (installer re-run, idempotent) |
| agents-opencode (project pkg) | 2.6.0 |
| universal skills (project `.agents/skills/`) | 41 via skills.sh (`npx skills update`) |
| global skills (`~/.agents/skills/`) | caveman suite (22) + ast-grep, ast-grep-outline |
| context-mode | 1.0.169 |
| opencode-rules | @latest (auto-updates) |
| ast-grep | 0.28.1 (Alpine pkg; upstream 0.51.0) |

## Prerequisites

- OpenCode CLI installed (`winget install OpenCode` on Windows, `nix shell nixpkgs#opencode` on Linux, or from opencode.ai)
- Node.js ≥ 22.5 (context-mode requirement; older Node fine for the rest)

## 1. Caveman + Cavecrew

Installer re-run overwrites with latest — idempotent:

```bash
# Step 1 — Plugin + commands + AGENTS.md
npx -y github:JuliusBrussee/caveman -- --force --only opencode

# Step 2 — Skills (caveman + cavecrew + more, 22 total)
npx -y skills add JuliusBrussee/caveman -a opencode -g -y

# Step 3 — Cavecrew agents (subagents; also installed by step 1 — redundant but harmless)
curl -fsSL https://raw.githubusercontent.com/JuliusBrussee/caveman/main/agents/cavecrew-investigator.md -o ~/.config/opencode/agents/cavecrew-investigator.md
curl -fsSL https://raw.githubusercontent.com/JuliusBrussee/caveman/main/agents/cavecrew-builder.md -o ~/.config/opencode/agents/cavecrew-builder.md
curl -fsSL https://raw.githubusercontent.com/JuliusBrussee/caveman/main/agents/cavecrew-reviewer.md -o ~/.config/opencode/agents/cavecrew-reviewer.md
```

This installs:

| What | Path | Files |
|------|------|-------|
| Plugin | `~/.config/opencode/plugins/caveman/` | `caveman-config.cjs`, `plugin.js` |
| Commands | `~/.config/opencode/commands/` | `caveman`, `ultracave`, `megacave`, `caveman-commit`, `caveman-review`, `caveman-compress`, `caveman-stats`, `caveman-help` |
| Skills | `~/.agents/skills/` + `~/.config/opencode/skills/` | 22: caveman suite, cavecrew, investigate-first, lean-build, migration, safe-refactor, surgical-patch, verify-and-stop |
| Agents | `~/.config/opencode/agents/` | `cavecrew-builder`, `cavecrew-investigator`, `cavecrew-reviewer` (mode: subagent) |
| Global AGENTS.md | `~/.config/opencode/AGENTS.md` | appends `<!-- caveman-begin -->` block, patches `opencode.jsonc` |

### Caveat: caveman ≥ current — no manual patching needed

Older cavecrew agent files shipped a `tools:` array (invalid in opencode ≥1.15) and needed `hidden: true`. Current upstream uses `mode: subagent` and no `tools` frontmatter — the installer output loads clean. If `opencode debug config` reports `ConfigInvalidError` for cavecrew agents, re-run the installer before hand-patching.

## 2. Global plugins (`~/.config/opencode/opencode.jsonc`)

Current full global config:

```jsonc
{
  "$schema": "https://opencode.ai/config.json",
  "plugin": [
    "opencode-rules@latest",
    "./plugins/caveman",
    "context-mode"
  ],
  "compaction": {
    "auto": true
  },
  "permission": {
    "*": "allow",
    "bash": {
      "*": "allow",
      "sudo": "deny",
      "sudo *": "deny",
      "doas": "deny",
      "doas *": "deny"
    }
  },
  "mcp": {
    "context7": {
      "type": "remote",
      "url": "https://mcp.context7.com/mcp",
      "enabled": true,
      "headers": {
        "CONTEXT7_API_KEY": "<api-key>"
      }
    }
  }
}
```

Plugins auto-download on first launch. `opencode-rules` uses `@latest` and auto-updates at startup.

## 3. context-mode (v1.0.169)

Context-window saver: registers 11 `ctx_*` tools (in-process TypeScript plugin, no per-session MCP child), routes big raw data (large files, long output, dumps) through its own storage with FTS5 + intent-driven search instead of the live context window. Hooks enforce routing — direct reads of big payloads may be DENIED and redirected.

Install = the `"context-mode"` entry in the global `plugin` array (section 2). Then restart OpenCode.

**Hard rule:** `context-mode` goes in the `plugin` array ONLY. NEVER add a `mcp.context-mode` entry — plugin + legacy MCP entry together register ZERO `ctx_*` tools. Fix: run `context-mode upgrade`.

Verify: in a session, run `ctx stats`.

Routing awareness: global `~/.config/opencode/AGENTS.md` carries a `<!-- context-mode-begin -->` block explaining the `ctx_*` routing and the plugin/MCP conflict rule. Per-project routing files are optional — add only if a project needs custom routing.

## 4. ast-grep (Alpine pkg)

Structural code search/lint/rewrite over tree-sitter ASTs. Binary is a system install (`apk`) because **no musl build exists** upstream: npm ships `gnu`-only platform packages, GitHub releases too. On Alpine, the apk IS the musl build.

```bash
doas apk add ast-grep          # Alpine 3.24: v0.28.1 (upstream is 0.51.0 — apk lags, upgrade via apk upgrade)
npx -y skills add ast-grep/agent-skill -g -y -a opencode   # skills → ~/.agents/skills/ (ast-grep + ast-grep-outline)
```

- opencode loads `~/.agents/skills/` natively — no copy into `~/.config/opencode/skills/` needed.
- `--include=optional` does NOT fix npm on musl — don't try; use the apk.
- Search scope = the path argument: `ast-grep run -p '<pattern>' .` from a repo root searches that repo only.
- Per-project custom rules: `sgconfig.yml` at the project root + `ast-grep scan`.
- Preference rules live in global `AGENTS.md` (`<!-- ast-grep-begin -->` block): structural queries → ast-grep, plain text → grep/rg.

## 5. Global Rule (`rules/general.md`)

`~/.config/opencode/rules/general.md`:

```markdown
- Caveman ON. Full.
- English ONLY.
- Context7 MCP for docs.
```

## 6. Global AGENTS.md

`~/.config/opencode/AGENTS.md` holds header rules plus three marker blocks:

```markdown
- Caveman ON. Full.
- English ONLY.
- Context7 MCP for docs.

<!-- caveman-begin -->
...caveman rules (installer-managed)...
<!-- caveman-end -->

<!-- context-mode-begin -->
...context-mode routing rules (manual, see section 3)...
<!-- context-mode-end -->

<!-- ast-grep-begin -->
...ast-grep preference rules (manual, see section 4)...
<!-- ast-grep-end -->
```

## 7. Context7 MCP

Configured in the global `mcp.context7` section (section 2). Requires a valid API key from https://context7.com.

## 8. Permission Configuration

Global config carries one hard rule: **no privilege escalation** — `sudo` and `doas` (Alpine's sudo-equivalent) are denied in bash patterns; everything else is allowed. Per-project configs mirror the same bash block and may add project-specific restrictions (this project also sets `external_directory: ask`, `doom_loop: deny`).

```jsonc
"permission": {
  "*": "allow",
  "bash": {
    "*": "allow",
    "sudo": "deny",
    "sudo *": "deny",
    "doas": "deny",
    "doas *": "deny"
  }
}
```

Note: this blocks the AGENT's bash calls only — the user still runs `doas apk ...` in their own terminal.

## 9. Parked / evaluated (not installed)

- **agentmemory (rohitg00) + launcher** — rejected for now: launcher v4 requires OpenCode **V2** (we run 1.15.10) and is tested on Windows only. The `@agentmemory/mcp` shim was not evaluated further.
- **opencode-memory-pro** (LanceDB, V1-compatible) — candidate for per-project long-term memory (solutions/decisions/gotchas, semantic recall). Parked pending evaluation. If adopted: global plugin install, per-project `dbPath` override in each project's `opencode.json`.
- **@mathew-cf/opencode-memory** — rejected: global `~/opencode-memory/` store by design; we want per-project memory only.

## Verify Complete Setup

```bash
# Config
cat ~/.config/opencode/opencode.jsonc

# Rules
cat ~/.config/opencode/rules/general.md

# AGENTS.md
cat ~/.config/opencode/AGENTS.md

# Commands / Skills / Agents / Plugin
ls ~/.config/opencode/commands/
ls ~/.config/opencode/skills/
ls ~/.config/opencode/agents/
ls ~/.config/opencode/plugins/caveman/

# Global skills (opencode loads ~/.agents/skills/ natively)
ls ~/.agents/skills/

# ast-grep
ast-grep --version

# Merged config sanity (run inside any project)
opencode debug config

# context-mode live check (inside an opencode session)
# type: ctx stats
```
