# Static analysis policy

Binding. Every rule below runs in CI and locally through
`scripts/static-check.sh` (container `ursine-static:1`). A finding is not
waived without an entry in `docs/static-analysis-exceptions.md` with owner,
reason and expiry (max 90 days).

## Layers and gates

| Layer | Tool | Gate |
|---|---|---|
| Formatting | `cargo fmt --check` | blocking |
| Compiler | `cargo check --all-targets --all-features` | blocking |
| Rust lints | workspace `[lints]` + strict clippy | blocking, `-D warnings` |
| Docs | `cargo doc` with `RUSTDOCFLAGS=-D warnings` | blocking |
| Repo lint | `typos`, `editorconfig-checker` | blocking |
| Scripts | `shellcheck`, `shfmt` | blocking |
| Nix | `statix`, `deadnix`, `alejandra --check` | blocking |
| TOML/Markdown | `taplo`, `markdownlint-cli2` | blocking |
| GitHub Actions | `actionlint` (local image), `zizmor` (official image, CI) | blocking |
| SAST | Semgrep (`p/rust`, `p/secrets`, custom rules) | blocking |
| Secrets | `gitleaks` full history | blocking |
| Unsafe | `unsafe_code = forbid`; `cargo-geiger` optional report (no musl binary) | blocking (forbid) |
| Advisories | `scripts/audit-threshold.sh` + `cargo-deny advisories` | warn 1–9, block ≥10 dependencies |
| Licenses | `cargo-deny licenses` | blocking |
| Sources/bans | `cargo-deny sources`, `bans` | blocking |
| Supply chain | `cargo-deny` + `cargo-audit`; `cargo-vet` optional (no musl binary) | blocking (deny/audit) |
| Unused deps | `cargo-machete` | blocking |
| SBOM | `cargo-cyclonedx` | artifact per release |
| Duplicate code/crates | `jscpd`, `cargo tree -d` | report only |

## Rust lint policy

Workspace lints are versioned in `Cargo.toml`; every crate opts in with
`[lints] workspace = true`. `unsafe_code` is forbidden in Ursine crates.
Clippy runs `all`, `pedantic`, `nursery`, `cargo` and a curated restriction
set (`unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented`, `dbg_macro`,
`print_stdout`, `print_stderr`, `exit`, `mem_forget`, `undocumented_unsafe_blocks`).

Exceptions live in `clippy.toml` (tests may use `unwrap`/`expect`) or in a
local `#[allow(...)]` with a `// reason:` comment. No blanket crate-level
allows without an exception entry.

## Dependency policy

- `Cargo.lock` committed; `--locked` everywhere.
- crates.io only, plus the pinned `github.com/pop-os` git dependencies.
- No wildcard versions, no unknown registries.
- Licenses allowlisted in `deny.toml`.
- `cargo-vet` exemptions are debt: every new exemption needs an owner and
  expiry; imported audits are preferred.
- GitHub Actions pinned by full commit SHA; `zizmor` enforces it.

## Duplicates

Code and crate duplication are measured and reported, never gated:
`artifacts/duplicates/jscpd.json`, `artifacts/duplicates/cargo-tree-duplicates.txt`.
Review quarterly.

## Tool pinning

Tool versions are pinned in `ci/Containerfile.static` (cargo tools, shell
linters) and in `ci.yml` (zizmor action and GitHub Actions SHAs). Upgrade them
in a dedicated PR monthly, never silently. The Rust toolchain is pinned in
`rust-toolchain.toml`; CI and local images must match it. `zizmor` is not built
into the static image (a from-source build costs ~30 minutes); CI runs the
official `zizmorcore/zizmor-action` image instead.

## Container policy

Ursine is not containerized and ships no container image. Container scanning
therefore does not apply. Build/test isolation still uses the dev/static
containers; those images are tooling, not deliverables.

## Advisory threshold

`cargo audit` reports every finding. The gate blocks only when **10 or more
distinct dependencies** have a vulnerability, unmaintained or unsound finding;
1–9 findings produce a warning and a JSON report in
`artifacts/static/cargo-audit.json`. This is deliberate: a small upstream
backlog stays visible, but it cannot grow silently into a large one. Every
finding is triaged in `docs/static-analysis-exceptions.md`; lowering the
threshold is a policy change, not a per-PR decision.
