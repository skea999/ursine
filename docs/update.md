# Update Reference

Run these from the project root.

## Rust toolchain (project)

```bash
rustup update                # stable toolchain
cargo update                 # dependency lockfile (Cargo.lock)
cargo install cargo-audit    # once; then:
cargo audit                  # dependency vulnerability scan
```

## OpenCode CLI

```bash
# Windows
winget upgrade OpenCode

# Linux (nix)
nix profile upgrade 'nixpkgs#opencode'

# macOS / any (npm)
npm install -g @opencode/cli@latest
```

## Caveman + Cavecrew (global)

Re-run the same 3-step install commands to overwrite with latest:

```bash
# Step 1 — Plugin + commands + AGENTS.md
npx -y github:JuliusBrussee/caveman -- --force --only opencode

# Step 2 — Skills (caveman + cavecrew)
npx skills add JuliusBrussee/caveman -a opencode

# Step 3 — Cavecrew agents
curl -fsSL https://raw.githubusercontent.com/JuliusBrussee/caveman/main/agents/cavecrew-investigator.md -o ~/.config/opencode/agents/cavecrew-investigator.md
curl -fsSL https://raw.githubusercontent.com/JuliusBrussee/caveman/main/agents/cavecrew-builder.md -o ~/.config/opencode/agents/cavecrew-builder.md
curl -fsSL https://raw.githubusercontent.com/JuliusBrussee/caveman/main/agents/cavecrew-reviewer.md -o ~/.config/opencode/agents/cavecrew-reviewer.md
```

## Universal skills (`.agents/skills/`)

Tracked in `skills-lock.json` via `skills.sh` CLI (Vercel Labs). Auto-installs on first run.

```bash
npx skills update
```

As needed, restore all skills from the lock file:

```bash
npx skills experimental_install
```

## agents-opencode package (`.opencode/`)

Updates agents, skills, plugin, commands managed by the package.

```bash
npx agents-opencode --update
```

## Project plugins (opencode.json)

- `opencode-rules` — uses `@latest` tag in `opencode.json`, auto-updates on every OpenCode startup
- `agents-opencode` — updated via `npx agents-opencode --update` above

## Template agents

```bash
npx github:dmicheneau/opencode-template-agent install --pack quality --force
npx github:dmicheneau/opencode-template-agent install --pack security --force
```

## Global rules / AGENTS.md

`~/.config/opencode/rules/general.md` and `~/.config/opencode/AGENTS.md` are static files. Review and edit manually if the global policy changes.

## Context7 MCP API key

If the key in `~/.config/opencode/opencode.json` expires, replace it from https://context7.com.

## Notes

- User-created agents are safe from overwrites (no manifest entry).
- The agents-opencode harness (`.opencode/`) and universal skills (`.agents/skills/`) are present in this repo — `npx skills update` and `npx agents-opencode --update` work from here.
- Caveman re-run steps are idempotent — safe to run anytime.
