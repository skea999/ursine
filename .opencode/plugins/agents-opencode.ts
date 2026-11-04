import { Plugin } from "@opencode/plugin";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { isBlockedReadPath, selectPackVersion } from "../lib/guards.mjs";

/**
 * The plugin sits in `<pack-root>/plugins/`. Depending on install scope the pack
 * root is either a project's `.opencode/` directory or the global
 * `~/.config/opencode/` directory, so `agents/`, `skills/`, and `commands/` are
 * always one level up (`../`).
 */
const PACK_ROOT = new URL("../", import.meta.url);

type PackInfo = {
  version: string;
  inventory: string;
};

function safeRead(path: URL): string | null {
  try {
    return readFileSync(fileURLToPath(path), "utf8");
  } catch {
    return null;
  }
}

function countFiles(path: URL, extension: string, exclude: string[] = []): number {
  try {
    return readdirSync(fileURLToPath(path)).filter(
      (entry) => entry.endsWith(extension) && !exclude.includes(entry)
    ).length;
  } catch {
    return 0;
  }
}

function countSkills(path: URL): number {
  try {
    return readdirSync(fileURLToPath(path), { withFileTypes: true })
      .filter((entry) => entry.isDirectory())
      .filter((entry) => safeRead(new URL(`${entry.name}/SKILL.md`, path)) !== null)
      .length;
  } catch {
    return 0;
  }
}

/**
 * The installer writes `.opencode-agents-version` at the pack root. Its location
 * relative to this file depends on install scope:
 * - project scope: `<projectRoot>/.opencode/plugins/` -> `../../.opencode-agents-version`
 * - global scope:  `<configDir>/plugins/`              -> `../.opencode-agents-version`
 * The in-repo package.json is only trusted when it actually is this pack, so a
 * consumer's own package.json never leaks in as the pack version.
 */
function readVersion(): string {
  const markers = [
    safeRead(new URL("../../.opencode-agents-version", import.meta.url)),
    safeRead(new URL("../.opencode-agents-version", import.meta.url)),
  ];

  let packageName: unknown;
  let packageVersion: unknown;
  const rawPackage = safeRead(new URL("../../package.json", import.meta.url));
  if (rawPackage) {
    try {
      const parsed = JSON.parse(rawPackage);
      packageName = parsed?.name;
      packageVersion = parsed?.version;
    } catch {
      // selectPackVersion falls back to "unknown" when nothing resolves.
    }
  }

  return selectPackVersion({ markers, packageName, packageVersion });
}

function readPackInfo(): PackInfo {
  const version = readVersion();
  const agents = countFiles(new URL("agents/", PACK_ROOT), ".md");
  const skills = countSkills(new URL("skills/", PACK_ROOT));
  const commands = countFiles(new URL("commands/", PACK_ROOT), ".md", ["README.md"]);

  const parts = [
    agents > 0 ? `${agents} agents` : null,
    skills > 0 ? `${skills} skills` : null,
    commands > 0 ? `${commands} commands` : null,
  ].filter((part): part is string => part !== null);

  return {
    version,
    inventory: parts.length > 0 ? parts.join(", ") : "agent pack",
  };
}

const PACK = readPackInfo();

function compactionContext(): string {
  return `## Agents Opencode Context

You are operating with the agents-opencode v${PACK.version} agent pack.

Available agents:
- Task-delegatable agents (subagent/all): @codebase, @docs, @review, @planner, @brutal-critic, @legal-advisor, @researcher
- Primary agents (user switches with Tab; not Task-invocable): orchestrator, em-advisor, blogger

Active inventory: ${PACK.inventory} available as skills and slash commands.

Memory: state/session-state.json and handoff/latest.md preserve working state.
Context persistence: AGENTS.md tracks project milestones across sessions.`;
}

function sessionStateReminder(): string {
  return `## Session State Reminder
- Current phase: Read state/session-state.json for working memory
- Check handoff/latest.md for continuation context
- Review AGENTS.md for recent milestones and patterns`;
}

export default Plugin.define({
  id: "agents-opencode",
  async setup(ctx) {
    console.log(
      `Agents Opencode v${PACK.version} loaded — ${PACK.inventory} available`
    );

    // Inject pack context into compaction so critical state survives
    // context-window truncation.
    await ctx.session.hook("compaction", (event) => {
      event.system.push({ type: "text", text: compactionContext() });
      event.system.push({ type: "text", text: sessionStateReminder() });
    });

    // Safety hook: block agents from reading sensitive files.
    await ctx.tool.hook("execute.before", (event) => {
      if (event.tool === "read") {
        const input = event.input as { path?: unknown; filePath?: unknown };
        const candidate =
          typeof input.path === "string"
            ? input.path
            : typeof input.filePath === "string"
              ? input.filePath
              : null;
        if (candidate !== null && isBlockedReadPath(candidate)) {
          throw new Error(
            `[agents-opencode] Blocked reading sensitive file: ${candidate}. Do not read credential or secret files.`
          );
        }
      }
    });

    // Inject package version into shell environment for script awareness.
    await ctx.shell.hook("create.before", (event) => {
      event.env["AGENTS_OPENCODE_VERSION"] = PACK.version;
    });

    // Surface session-level failures through the plugin log so they are not
    // silently lost when a run aborts.
    const controller = new AbortController();
    void (async () => {
      try {
        for await (const event of ctx.event.subscribe({
          signal: controller.signal,
        })) {
          if (event.type === "session.error") {
            console.error("agents-opencode: OpenCode session error observed");
          }
        }
      } catch {
        // Subscription aborted during plugin unload.
      }
    })();

    return () => controller.abort();
  },
});
