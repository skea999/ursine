# Agents — Quick Reference

## How to start a task

In opencode CLI press `Tab` and select an agent. These are the ones you drive directly:

| Agent | Mode | What it does |
|-------|------|-------------|
| `plan` | primary (built-in) | Plan and analyze. Standard opencode. |
| `build` | primary (built-in) | Implement code. Standard opencode. |
| `codebase` | all | Like `build` but auto-detects project stack and loads matching skills. |
| `planner` | all | Like `plan` but produces detailed breakdowns with deps, risks, files. |
| `legal-advisor` | all | Legal research, license compliance, GDPR. |
| `researcher` | all | Research APIs/libs via Context7 docs, cross-refs ≥2 sources. (User-customized — protected, see session state.) |
| `blogger` | primary | Blog posts, podcast ideas, YouTube scripts. Calls `@brutal-critic` internally. |
| `em-advisor` | primary | Engineering management advice. Uses `em-advisor-meetings.instructions.md`. |
| `orchestrator` | primary | Complex multi-phase projects. Plans, then dispatches the specialists below via `task`. |

---

## Daily workflow

```
Small task:    select build or @codebase
Need analysis: select plan or @planner first
Legal check:   select @legal-advisor
Everything else: @name when really needed
```

### Example

```
1. Tab → select @planner
   "plan a checkout feature"

2. Tab → select @codebase
   "implement step 1 from the plan"

3. Tab → select build
   "fix the button alignment"
```

---

## Subagents (not user-callable)

Dispatched automatically via the `task` tool by the agents above — mostly by `@orchestrator`, `@codebase`, and `@review`. You don't `@` them in chat; ask a primary agent to delegate instead.

| Subagent | Dispatched by | What it does |
|----------|--------------|-------------|
| `@review` | `@orchestrator` | PR/quality review. Takes lens briefs (requirements, code, security, ux-responsive — Pattern 8 in `orchestrator-reference.instructions.md`). Security lens loads `security-audit`. |
| `@code-reviewer` | `@orchestrator`, `@review` chain | Code correctness, readability, architecture review. 3 axes per diff. Read-only. |
| `@debugger` | `@orchestrator` | Root-cause debugging. Reproduce → hypothesize → isolate → minimal fix → regression test. |
| `@docs` | `@orchestrator` | Writes README, API docs, ADRs, guides. |
| `@test-automator` | `@orchestrator` | Implements test suites, coverage, CI test integration. |
| `@performance-engineer` | `@orchestrator` | Profiling, bottleneck analysis, latency/throughput optimization. |
| `@refactoring-specialist` | `@orchestrator` | Safe cross-module refactoring. Preserves behavior. |
| `@legacy-modernizer` | `@orchestrator` | Migrates legacy frameworks/languages to modern stacks. Incremental, milestone-based. |
| `@security-auditor` | `@orchestrator` | Security assessment, threat modeling, compliance review. |
| `@penetration-tester` | `@orchestrator` | Simulated attacks, API security testing. |
| `@compliance-auditor` | `@orchestrator` | GDPR, SOC 2, HIPAA, PCI-DSS audits and gap analysis. |
| `@smart-contract-auditor` | `@orchestrator` | Solidity/Web3 security review, reentrancy/overflow/oracle checks. |
| `@documentation-engineer` | `@orchestrator` | Doc infrastructure: SSG, search, versioning, CI/CD for docs. |
| `@information-scan` | pre-commit gate | Pre-commit secrets/PII scanner. Also runs via git hook. |
| `@vulnerability-scan` | `@orchestrator` | Tool-based vulnerability scanning (Docker, CI, app code). |
| `@brutal-critic` | `@blogger` | Content review. Hidden from agent list. |

---

## Internal tools

Used by other agents internally. You don't call them.

| Tool | Called by | What it does |
|------|-----------|-------------|
| `@explore` | `@orchestrator`, `@codebase`, others | Fast file/content search. Read-only. |
| `@general` | `@orchestrator` | Generic multi-step research tasks. |
