# SPEC-web (v3 — stub)

## Status

**Stack undecided.** This stub fixes the binding constraint now so v3 cannot
violate it. Decision deferred to v3; see Open Questions.

## Objective

Web frontend for Ursine (v3, AFTER the desktop GUI): same API, same UX model,
served as static assets by `ursined` itself. Login via PAM today, OIDC later
(see SPEC-daemon-core / SPEC-protocol seam). Web is the LAST frontend.

## Binding constraint: memory-safe frontend

The web GUI must be memory-safe, like the rest of Ursine.

| Option | Status |
|---|---|
| **Rust → WebAssembly** (Leptos / Yew / Dioxus / Sycamore, `wasm-bindgen`) | **preferred**: our code memory-safe by construction, WASM sandbox isolates it from the DOM |
| TypeScript / JavaScript | acceptable fallback: language is memory-safe; browser runtime is outside our control |
| C/C++ compiled to WASM, native npm addons, NAPI, JNI-style bridges | **not allowed** |

No framework decision before v3. Whichever is chosen must justify: bundle size,
build reproducibility, no CDN dependencies, no telemetry.

## UX (binding, same as desktop)

- Capability-gated panels: disabled module shows `reason` + `fix` from the API.
- Same navigation structure and terminology as the desktop GUI (`CONTEXT.md`).
- Explicit confirmation for destructive actions (restore, delete user, prune).
- Keyboard navigable, accessible (WCAG basics), light/dark follows system.
- All actions audited daemon-side (frontend adds no policy).

## Security requirements

- Token kept in memory only (no `localStorage`/cookies for the session token);
  OIDC flow uses PKCE.
- Strict CSP, no third-party scripts/fonts, no inline eval.
- All state changes go through the daemon API; zero server logic in the web layer.
- WebSocket used only for progress streams (long nix operations).

## Project structure (expected)

```text
src/web/        static assets (build output committed or produced in CI)
```

Build toolchain TBD with the stack (e.g. `trunk` for Rust/WASM).

## Testing strategy (expected)

- Unit tests in the chosen language (WASM tests for Rust).
- E2E via the `webapp-testing` skill (Playwright) against a daemon in a container.
- Security: CSP check, dependency scan in CI.

## Boundaries

- Always: memory-safe stack, capability gating, confirmations, CSP.
- Ask first: any JS framework/dependency, any CDN, OIDC provider assumptions.
- Never: server-side rendering logic in the daemon, secrets in frontend code,
  session token on disk/localStorage, third-party trackers.

## Open Questions (v3)

1. Rust→WASM (which framework) vs TypeScript — decide with a spike.
2. Build pipeline: `trunk`/`wasm-pack` in CI vs committed build output.
3. Size budget for the static bundle.
4. Token transport in browser (in-memory + refresh strategy).
5. OIDC client library and provider-agnostic configuration.
