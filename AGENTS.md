# ExoRoute Agent Guide

This file is the working agreement for agents making changes in ExoRoute. Follow the user's request and these project rules. Complete the pre-change gate below before editing, keep changes focused, preserve existing behavior unless the task calls for changing it, and report checks that could not be run.

## Project at a glance

ExoRoute is a Rust AI protocol gateway with an Axum HTTP server, a Svelte 5 / TypeScript single-page dashboard, and an embedded LMDB environment. The Rust binary embeds the built dashboard from `web/dist` with `RustEmbed`.

- `src/main.rs`: the module roots and process entry point.
- `src/app/`: bootstrap, router assembly, startup banner, and background workers.
- `src/config/`: environment and file configuration, resolved limits, and `.env` secret handling.
- `src/state/`: shared application state, runtime gates, and initialized services.
- `src/infra/db/`: LMDB initialization, process lock, and typed domain storage operations.
- `src/infra/storage/`: the only module that opens and operates on the LMDB environment.
- `src/infra/telemetry/`: request analytics and statistics rollups.
- `src/admin/`: authenticated dashboard API, split per resource (`providers/`, `combos/`, `database/`, `settings/`, `sessions/`, ...).
- `src/gateway/`: public model API, provider selection, retries, streaming, and request logs. `continuity/` owns stream retry, `streaming/` the SSE translation, `execution/` target preparation.
- `src/security/`: authentication and request-safety controls. `egress/` splits provider URL policy, header-name policy, and the pinned client cache; `rate_limit/` splits policy types from the stateful engine.
- `src/protocol/`: protocol conversion between supported client and provider formats.
- `src/provider_adapters/`: one directory module per upstream provider, plus the adapter registry, auth flows, and model testing.
- `src/support/`: cross-cutting helpers. `test_support.rs` provides the isolated LMDB fixture every Rust test uses, and `mock_upstream.rs` the shared mock HTTP upstreams (axum router servers, scripted raw-socket responses, silent servers, SSE responses).
- `src/static_assets.rs`: serving the embedded frontend.
- `web/src/`: Svelte application. `lib/` holds the shared API client, types, i18n, formatting, session flow, and the feature registry; `features/` holds per-page feature modules and their components.
- `scripts/`: maintenance and benchmark tools. `scripts/check_source_boundaries.mjs` enforces the source layout in CI.
- `.github/workflows/release.yml`, `build.bat`: release and Windows build workflows.

The application stores its `.env` file and LMDB environment under the current user's `~/.exoroute` directory (on Windows, `%USERPROFILE%\.exoroute`); the default environment path is `exoroute.lmdb`. `EXOROUTE_DATABASE_PATH`, when set, names the LMDB environment directory. Environment variables override values from the app `.env` file. Relative data paths follow the resolution rules in `src/config/`.

The database is local-filesystem-only and is owned by one ExoRoute process at a time. Never put it on a network filesystem or open it from another process. Existing SQLite files are intentionally ignored: ExoRoute does not migrate SQLite databases or accept SQLite backups. Do not delete, inspect, or repurpose an old SQLite file as part of an LMDB change.

Use `README.md`, the current code, and existing tests as the source of truth when details differ from this guide. Do not assume that a planned or documented feature is implemented without checking it.

## Working in the codebase

- Inspect neighboring code and reuse its established patterns before adding abstractions, dependencies, or parallel implementations.
- Keep test modules in one `tests.rs` per feature, or split them into a `tests/` directory holding one file per concern plus a shared `support` module for harness code. Feature-specific fixtures belong in that feature's `support` module; mock upstream servers come from `src/support/mock_upstream.rs` instead of being rebuilt per test. `scripts/check_source_boundaries.mjs` rejects the `_tests.rs` naming and a module that has both `<name>.rs` and `<name>/mod.rs`.
- Keep Rust handlers, database operations, and protocol conversion in their existing modules. Keep frontend HTTP calls and shared types in `web/src/lib/` where appropriate.
- Make API changes consistently across the Rust handler, frontend client/types, and tests. Keep user-facing text translated in both English and Vietnamese in `web/src/lib/i18n.ts`.
- Match the current Svelte and CSS conventions. The project uses Svelte 5 but existing components may use legacy reactive syntax; do not migrate components to runes as part of unrelated work.
- Avoid adding a UI framework or a large dependency for a small feature. If a dependency is necessary, use a maintained, compatible package and explain why it is needed.
- Preserve existing error handling and return useful, non-secret diagnostics. Do not silently discard unsupported user input when the API can report that it is unsupported.
- Do not make production-throughput claims from unit tests or the benchmark script alone. State the test environment and measured limits.

## Mandatory pre-change gate

Before writing code, an agent must establish what it is changing and what behavior must remain true.

1. Read the relevant request and this guide. Inspect `git status` and the existing diff first. Treat modified and untracked files as user work: do not overwrite, stage, revert, or reformat unrelated changes.
2. Trace the affected behavior through its callers, handlers, persistence, UI/API types, configuration, and tests. Read neighboring code and use existing patterns before proposing a new abstraction or dependency.
3. For Rust work, check the active toolchain, `Cargo.toml`, `Cargo.lock`, build scripts, features, CI/release workflow, and migrations when applicable. For UI work, inspect the feature boundary, shared types, translation dictionaries, and existing Svelte/CSS conventions.
4. Before editing, identify the intended behavior, input and output contracts, security/data invariants, ownership/state boundary, failure and cancellation paths, and how existing installations or clients remain compatible. For non-trivial work, communicate a short implementation plan and important risks before editing, then proceed without waiting for approval unless the request itself requires approval.
5. Choose verification before implementation. Include the relevant success, invalid-input, failure, concurrency/cancellation, and compatibility cases. Do not add tests that merely mirror implementation details; prioritize observable behavior and regressions.
6. If the current tree already fails to build or has failing checks, capture the exact baseline. Do not attribute pre-existing failures to the change or claim a check passed against a different snapshot.

For a small, localized change, keep this gate lightweight, but never skip checking the working tree, ownership boundary, relevant callers, or a meaningful verification path. Do not use an unrelated task as an excuse for broad cleanup or architecture rewrites.

## Rust production quality bar

These rules apply to production Rust code, including handlers, workers, migrations, and protocol adapters.

- Prefer explicit, typed failure paths. Do not use `unwrap`, `expect`, `panic!`, `todo!`, or `unimplemented!` for runtime input, I/O, database, network, or configuration failures. An `expect` is acceptable only for a narrowly defined invariant that cannot be caused by external input, with the invariant documented beside it. Do not turn corrupt or unsupported data into success with `unwrap_or_default`, ignored errors, or silent fallbacks; fail safely and retain useful, non-secret diagnostics.
- Audit every `clone`, allocation, and collection on request paths. Avoid cloning large `String`, `Vec`, JSON values, response bodies, or credential lists to work around ownership. Clone cheap handles such as `Arc` or `Bytes` when appropriate. Before keeping an expensive clone, explain why ownership transfer, borrowing, or a bounded/lazy design is unsuitable; measure hot-path optimization claims.
- Bound memory and work before allocation or expensive processing: request and response bytes, parsed collections, caches, queues, spawned tasks, retries, and per-request fan-out. Use pagination or lazy iteration for large datasets. Any cache or map keyed by deletable entities needs an explicit cleanup or capacity policy. Preserve the user requirement that a provider may have an unlimited number of API keys; do not add an arbitrary key cap—make routing and administration lazy/paginated instead.
- Async code must not perform blocking file, network, CPU-heavy, or synchronous database work on Tokio workers. Use `spawn_blocking` only for bounded blocking work and protect it with concurrency limits. Avoid `std::sync::Mutex` in contended async request paths; do not hold locks across `.await`, database calls, or network calls unless serialization is required and the reason, contention behavior, and cancellation behavior are documented. Never create an unbounded channel or unbounded task fan-out.
- Review cancellation and backpressure as normal control flow. Every acquired permit, half-open probe, temporary file, queue item, and background task must be released, drained, retried, or safely abandoned when its future is cancelled, a client disconnects, a timeout fires, or shutdown begins. Use RAII where possible and test the cancellation path for stateful streaming or retry logic.
- Keep LMDB reads and writes behind `src/infra/storage/` and domain operations in `src/infra/db/` or the owning feature module. Use typed records, bounded prefix scans and secondary indexes; avoid per-request full-table reads and eager materialization of user-growable data. Update secondary indexes atomically with primary records, preserve transaction/rollback behavior, and validate backup/import records before replacing live data. Never use a live user database as a test fixture.
- Network calls need explicit connect, request/idle, and overall time limits, bounded response consumption, safe retry semantics, and useful redacted errors. Preserve egress validation, DNS pinning, redirect policy, and local-provider exceptions. Do not retry an inference when the upstream may have accepted or billed it unless the protocol makes retry safe.
- Avoid `unsafe`. If FFI or unsafe code is unavoidable, isolate the smallest block, document the safety invariants at that block, and add focused tests for assumptions that can be tested. Do not use unsafe for speculative micro-optimization.
- Add a dependency only when the standard library and existing dependencies cannot meet the need cleanly. Review default features, duplicate runtimes, compile impact, license/maintenance, and the lockfile; keep `Cargo.lock` with this binary project and update it intentionally.
- Keep module boundaries cohesive. Split a module when it owns independent responsibilities or lifecycle/state boundaries, not to hit an arbitrary line count. Do not create wrapper, pass-through, generic, or “base” abstractions that only move code or forward arguments without reducing coupling or complexity. A production budget of 800 lines per file *is* enforced by `scripts/check_source_boundaries.mjs`. Treat it as a review trigger, not a target: either split along real responsibilities, or record why the file owns one large responsibility in that script's `oversizedAllowlist`. Never satisfy it with a pass-through module, which is the outcome both this rule and the budget exist to prevent.
- Do not claim throughput, low latency, memory efficiency, or production scale without a representative benchmark or load test. Include machine/configuration, workload, sample size, and measured limits; distinguish code-inspection hypotheses from measured bottlenecks.

## Production code review mode

When the user asks for a production/security/performance audit or code review:

- Review the current worktree, not memory or an earlier snapshot. Inspect the diff and the complete affected call path before assigning severity. Do not make edits during a review-only request.
- Every finding must include concrete evidence (file and location), trigger/precondition, practical impact, and a viable remediation. Separate confirmed defects from risks that need a test or benchmark. Do not invent issues to fill a checklist; explicitly say when an area was inspected and no material issue was found.
- Prioritize findings as P0 (blocks build/release or creates immediate severe risk), P1 (fix before production exposure or significant traffic), P2 (planned reliability/security/maintainability fix), or P3 (low-risk debt). Explain the user-visible failure mode, not just the code smell.
- Cover correctness, ownership/cloning, async/blocking, locks and lock order, cancellation, channel capacity/backpressure, memory and CPU bounds, database queries/transactions, panic and unsafe boundaries, dependency/features, security, observability, and test gaps as relevant to the change.
- For performance review, identify hot-path full scans, `fetch_all`, serialization copies, allocations, contention, task fan-out, and database write patterns. Do not turn inspection hypotheses into measured claims. Require a representative benchmark/load test before stating capacity or latency.
- Give exact verification results for the reviewed snapshot, including failures and unavailable tools. If a build fails, distinguish compile blockers from findings that require runtime validation. If the user requests a scored audit, rate architecture, correctness, idiomatic Rust, ownership, async, concurrency, memory, CPU, errors, security, observability, testing, maintainability, and production readiness, with a short reason for each score; finish with no more than ten prioritized actions.

## Strict Svelte component boundaries

Apply these rules throughout `web/src/`. Responsibility, cohesion, ownership, and comprehension matter more than file size.

- A component needs a clear UI/domain responsibility, behavior, local state, lifecycle, reuse, independent change boundary, or meaningful standalone test boundary. Before extracting one, state which of these improves and why the parent becomes easier to understand. Do not split only to reduce lines, wrap a `div`, isolate a few repeated markup lines, or increase component count.
- Use LOC only as a review signal: 0–150 is ordinary; 150–250 merits a responsibility review; 250–400 strongly suggests considering a split; over 400 requires a clear reason to keep the component intact. These are not automatic split rules. A smaller component with several unrelated responsibilities still needs a better boundary.
- Review components with more than 8 props; more than 12 is a strong warning, not a hard limit. Avoid many boolean props that change a component's identity. Prefer a small explicit variant or composition. Do not build speculative generic business components or force superficially similar domains into one abstraction; small, clear duplication is better than a wrong abstraction.
- Keep state close to the UI that owns it. A page should orchestrate route-level data and compose feature components; tables, rows, forms, dialogs, menus, and filters should own their details when those details have independent behavior. Avoid prop drilling through components that only forward values; use composition or feature-scoped context only when it simplifies ownership, not as a default global store.
- Fetch data at a route/page/feature-container or dedicated data layer unless the component truly owns that data lifecycle. Keep leaf components presentational and free of hidden network, storage, global-state, document, or WebSocket side effects unless that is their explicit responsibility. Keep business logic independent of UI when it can be a function, utility, service, or feature state module.
- Co-locate feature-specific components and utilities with their feature. Put a component in shared UI only when it is genuinely generic, feature-independent, and has a stable API. Keep import direction from pages to features to shared UI/utilities; do not introduce circular dependencies or make UI primitives depend on domain features.
- Modals and complex menus/popovers with their own state, keyboard handling, positioning, or lifecycle normally deserve clear boundaries. Do not extract trivial list rows or form fields without independent behavior, reuse, or test value. Preserve Svelte 5 conventions already used by the surrounding code; do not migrate unrelated components to runes.
- A component with three or more independent warning signs (large LOC, many props/states/handlers/effects/API calls, unrelated conditional UI, multiple dialogs/forms/domain concepts) must receive an explicit boundary review. Keep it only with a reason tied to cohesion and comprehension, not convenience.

Every new or changed user-visible string must use the shared English and Vietnamese dictionaries. Continue to use the common dashboard API client, preserve centralized authentication/redirect behavior, and keep backend validation authoritative.

## Output style invariants

- Output styles are a global, default-off gateway setting persisted in the `OutputStyles` LMDB table. The only supported v1 IDs are `terse-prose`, `less-code`, and `ponytail`; each has `lite`, `full`, and `ultra` levels. Keep selections bounded, unique, canonicalized in catalog order, and protected by revision compare-and-swap.
- Inject deterministic English instructions exactly once into the canonical system/developer context after request decoding. Do not rewrite provider responses, alter tools/media/metadata/order, add per-request/header overrides, infer language, or let a client-supplied marker suppress injection. Failover must reuse the same canonical request without duplicate instructions.
- Conservatively bypass every enabled style when the bounded tail of messages contains security/credential or vulnerability warnings, irreversible actions, explicit detail/step-by-step clarification requests, or ordered backup/migration/deploy/release work. Bound bytes scanned per message and in total; do not read an unbounded request body to classify it.
- Save/reset/import must validate before a bounded LMDB writer transaction, compare `expected_revision` inside that transaction, commit atomically, and publish the runtime snapshot only after commit. A lost response must not cause an automatic PUT retry; the client verifies with GET. Corrupt records, unknown IDs, duplicate selections, unsupported versions, revision overflow, storage busy/map-full/I/O, or failed imports retain the current runtime state and return a sanitized error.
- Keep output-style metrics bounded to fixed style/reason labels and counters. Never log or expose instruction text, request content, credentials, or other secrets.

## Security and data invariants

Treat these as security boundaries. Changes that affect them need focused tests and a clear justification.

- The HTTP listener defaults to loopback. It must reject non-loopback binds because the service itself does not provide TLS. Remote deployment belongs behind a correctly configured TLS reverse proxy; do not weaken the bind check to make remote access appear to work.
- Gateway model requests require an ExoRoute API key. Admin endpoints require an authenticated, short-lived admin session. Preserve the `/login` redirect and forced password-change flow; frontend checks are not a substitute for backend authorization.
- Keep admin access tokens only in JavaScript memory; never persist them in `sessionStorage`, `localStorage`, IndexedDB, or URLs. The refresh token is an HttpOnly, SameSite=Strict cookie with single-use rotation, replay detection, 24-hour idle expiry, and 7-day absolute expiry. Reuse the common client in `web/src/lib/api.ts`; preserve cross-tab refresh coordination and transient-error behavior.
- Database export and import require a one-use, session-bound proof issued after password reauthentication. Never send the raw password with backup data or store/log the proof; keep the proof scoped to its operation and short-lived.
- Provider credentials are secrets and must remain encrypted at rest. Preserve the configured master-key behavior and ensure exports/imports keep security-relevant password-change state intact.
- `EXOROUTE_MASTER_KEY` must remain stable for an installation and is not a substitute for database backup. Do not put it in database exports or release artifacts. Preserve strict permissions and symlink protections around the user's data directory and secret files.
- Never log, print, return in an error, or commit passwords, provider keys, gateway keys, session tokens, encryption keys, or `.env` contents. Do not inspect or modify a user's live database or credentials unless the task specifically requires it.
- Provider destinations are untrusted input. Preserve HTTPS requirements, DNS/IP validation and pinning, redirect restrictions, and protections against loopback, private, link-local, and other non-public destinations. Preserve any explicit, narrowly-scoped local-provider override policy already implemented. Application checks complement deployment egress controls; they do not replace them.
- Preserve request size, timeout, concurrency, rate-limit, and database bounds. Validate limits before allocation or expensive work, and avoid unbounded buffering—especially for upstream responses and streaming events.
- Assume one ExoRoute process owns the LMDB environment. Preserve its process lock, private directory/file permissions, and symlink checks; do not add multi-process access or place it on a network filesystem. Do not claim process-local sessions or rate limits are shared across replicas.

## Frontend and API conventions

- Build the frontend before building or running the Rust binary when dashboard assets may have changed; the binary embeds `web/dist` at compile time.
- Use `web/src/lib/api/` for dashboard API calls and `web/src/lib/types/` for shared request/response types. Preserve the backend as the authority for authorization and validation.
- Keep login and redirect handling centralized. Validate any post-login return path so it cannot redirect to an untrusted external URL.
- Follow existing loading, empty, error, and success-state patterns. Make forms keyboard-accessible and expose validation or provider-key test warnings clearly.
- Add or update both English and Vietnamese strings when changing visible text. Do not hard-code new UI copy in a component if it belongs in the shared dictionaries.
- Keep the Rust API and frontend compatible in the same change. Test response shapes and error cases, not just successful requests.
- Provider configuration supports multiple API keys per provider, and a failed key test can be saved with a warning. Preserve key rotation/failover when a key becomes unusable; do not add an arbitrary total-key cap. Changes to routing or conversion should cover all affected protocols (Chat Completions, Responses, and Anthropic Messages) and streaming where applicable.
- Static browser routes should continue to fall back to the dashboard entry point; unknown `/api*` and `/v1*` paths must remain API 404 responses.

## Build and verification

Run checks relevant to the files changed. For a broad Rust change, run the full applicable set:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo check --workspace --all-targets --all-features
cargo check --release
```

Also run the source-layout check, which CI enforces:

```powershell
node scripts/check_source_boundaries.mjs
```

If a baseline check already fails, record that baseline and ensure the change adds no new failures. Do not hide warnings with broad `allow` attributes or claim the full verification passed when only one command passed. Run platform-specific checks for touched `cfg`/FFI code, migrations for schema changes, and relevant integration or cancellation tests for network/streaming changes. A load test or benchmark is required before making a performance claim, not for every routine change.

From `web/`:

```powershell
npm ci
npm run check
npm run build
```

To create the Windows release binary, run `build.bat` from the repository root. It builds the dashboard and then runs `cargo build --release`; the binary is `target\release\exoroute.exe`.

For development, run `npm run dev` from `web/` and use the Rust server for the API. For a release or packaged-binary change, inspect `.github/workflows/release.yml` and verify that the dashboard build precedes the Rust build for every target.

Do not report a check as passing unless it completed successfully. If a check is unavailable or unrelated, say so briefly and give the reason. Avoid running commands that start a persistent server, change a user's profile data, or initialize/modify a live database unless needed for the task.

For dependency changes, inspect the resolved feature tree and lockfile. Run a dependency vulnerability audit when the tool is available; report clearly if it is unavailable rather than implying the dependencies were audited.

## Changes to releases and persistent data

- Keep release assets and installer behavior aligned with `.github/workflows/release.yml`, `build.bat`, and the documented install flow. Validate archive names, executable names, and supported target triples against the actual workflow.
- Treat LMDB storage format changes as versioned internal migrations. Keep import/export versioned and atomic; validate records and relationships before replacing live data. Do not add SQLite migration or fallback support. Update backup/restore behavior when persisted security or configuration state changes.
- Database import/export handles sensitive state. Preserve size limits, validation, safe/atomic restore, serialization, and required reauthentication behavior.
- Keep generated output (`target/`, `web/node_modules/`, built assets, local databases, and environment files) out of source changes unless the task explicitly requests an artifact.
- Never commit secrets, local `.env` files, database files, API keys, or user-specific absolute paths.

## Completion report

When finishing code work, give a concise, reviewable report that states:

- What behavior changed and which files or boundaries changed.
- Why the design preserves correctness, ownership, security, failure handling, and compatibility.
- Exact checks run and whether each passed or failed. Mark checks that were not run and why; distinguish current results from an earlier baseline.
- Any remaining risks, migrations, platform gaps, or unmeasured performance assumptions.

Never say “production-ready,” “handles millions of requests,” “no vulnerabilities,” or similar without evidence that matches the claim. Do not claim a refactor, test, benchmark, security scan, or build that was not actually completed.

<!-- dsh-openwolf:start -->
# Code Map
Generated 2026-11-10T02:49:51.581Z · 835 files · 170507 lines · 3.49s

## ./
- `Cargo.lock` — 0 lines · [file too large]
- `Cargo.toml` — 69 lines · [package]
- `LICENSE` — 21 lines · MIT License
- `README.md` — 318 lines · <p align="center">
- `RELEASE_v0.1.1.md` — 127 lines · Second source release of ExoRoute, a local-first AI protocol gateway and model router built with Rust, Axum, LMDB, and Svelte 5.
- `build.bat` — 48 lines · @echo off
- `build.rs` — 488 lines · valid_id, valid_model_prefix, valid_default_model, validate_provider_presets, rust_string, rust_string_slice · env, fs,
- `install.ps1` — 296 lines · $ErrorActionPreference = 'Stop'
- `install.sh` — 280 lines · set -eu

## assets
- `assets/cli-demo.webp` — 0 lines · [file too large]
- `assets/favicon.svg` — 62 lines · <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" fill="none">
- `assets/hero-banner.jpg` — 0 lines · [file too large]
- `assets/logo-mark.svg` — 68 lines · <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" fill="none">
- `assets/logo.svg` — 70 lines · <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 260 56" fill="none">

## assets/models
- `assets/models/cerebras.svg` — 6 lines · <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" fill="none">
- `assets/models/claude.svg` — 7 lines · <?xml version="1.0" encoding="UTF-8"?>
- `assets/models/deepseek.svg` — 13 lines · <?xml version="1.0" encoding="UTF-8"?>
- `assets/models/devinai.webp` — 0 lines · [binary]
- `assets/models/fireworks.svg` — 4 lines · <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" fill="none">
- `assets/models/gemini.svg` — 0 lines · <svg height="1em" style="flex:none;line-height:1" viewBox="0 0 24 24" width="1em" xmlns="http://www.w3.org/2000/svg"><title>Gemini</title><p…
- `assets/models/grok.svg` — 0 lines · <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0.36 0.5 33.33 32"><path d="M13.2371 21.0407L24.3186 12.8506C24.8619 12.4491 25…
- `assets/models/groq.svg` — 4 lines · <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" fill="none">
- `assets/models/inclusion.svg` — 0 lines · <svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64" version="1.1"><path d="M 26.882 17.184 C 26.816 17.908, 2…
- `assets/models/kimi.svg` — 18 lines · <svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:xodm="http://www.corel.com/coreldraw/odm/2003" xml:…
- `assets/models/meta.svg` — 0 lines · <svg xmlns="http://www.w3.org/2000/svg" width="1567" height="1041" version="1.2"><defs><linearGradient id="a" x1="332.6" x2="1411" y1="637.7…
- `assets/models/minimax.svg` — 0 lines · <svg fill="#E73562" role="img" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><title>MiniMax</title><path d="M11.43 3.92a.86.86 0 1 …
- `assets/models/mistral.svg` — 10 lines · <svg xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape" xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd" xmlns=…
- `assets/models/nvidia.svg` — 0 lines · <svg height="1em" style="flex:none;line-height:1" viewBox="0 0 24 24" width="1em" xmlns="http://www.w3.org/2000/svg"><title>Nvidia</title><p…
- `assets/models/openai.svg` — 9 lines · <svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 2406 2406">
- `assets/models/perplexity.svg` — 5 lines · <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" fill="none">
- `assets/models/poolside.svg` — 3 lines · <svg width="128" height="128" viewBox="0 0 128 128" fill="none" xmlns="http://www.w3.org/2000/svg">
- `assets/models/qwen.svg` — 14 lines · <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="27.55 17.52 147.28 145.51">
- `assets/models/sakana.svg` — 0 lines · <svg height="1em" style="flex:none;line-height:1" viewBox="0 0 24 24" width="1em" xmlns="http://www.w3.org/2000/svg"><title>sakana.ai</title…
- `assets/models/tencenthy.svg` — 0 lines · <svg fill="#0052D9" role="img" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><title>Tencent Hy</title><path d="M12 0a1 1 0 0 1 0 24…
- `assets/models/together.svg` — 5 lines · <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" fill="none">
- `assets/models/xiaomi.svg` — 29 lines · <?xml version="1.0" encoding="utf-8"?>
- `assets/models/z-ai.svg` — 214 lines · <svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" version="1.1" x="0px" y="0px" style="enable-background:ne…

## assets/providers
- `assets/providers/antigravity.webp` — 0 lines · [binary]
- `assets/providers/cline.webp` — 0 lines · [binary]
- `assets/providers/codex.webp` — 0 lines · [binary]
- `assets/providers/commandcode.webp` — 0 lines · [binary]
- `assets/providers/devinai.webp` — 0 lines · [binary]
- `assets/providers/freebuff.webp` — 0 lines · [binary]
- `assets/providers/kilocode.webp` — 0 lines · [binary]
- `assets/providers/kiro.webp` — 0 lines · [binary]
- `assets/providers/minimax.webp` — 0 lines · [binary]
- `assets/providers/nvidia.webp` — 0 lines · [binary]
- `assets/providers/opencode.webp` — 0 lines · [binary]
- `assets/providers/openrouter.webp` — 0 lines · [binary]

## data
- `data/provider-presets.yaml` — 1529 lines · providers:

## docs/design
- `docs/design/README.md` — 88 lines · This directory is version-controlled on purpose. The rest of `docs/` holds
- `docs/design/muse-integration-report.md` — 874 lines · Nguồn tham chiếu đã đọc trực tiếp:

## docs/releases
- `docs/releases/RELEASE_v0.1.2.md` — 118 lines · Release Date: 2026-11-04

## npm
- `npm/LICENSE` — 21 lines · MIT License
- `npm/README.md` — 268 lines · [![npm version](https://img.shields.io/npm/v/exoroute.svg)](https://www.npmjs.com/package/exoroute)
- `npm/package.json` — 25 lines · ExoRoute AI protocol gateway launcher

## npm/bin
- `npm/bin/exoroute.js` — 40 lines · fs, path, executableName, executablePath, metadata, child · 'use strict';

## npm/scripts
- `npm/scripts/install.cjs` — 342 lines · crypto, fs, fsp, os, path, execFileAsync · 'use strict';

## scripts
- `scripts/benchmark.py` — 438 lines · MockProvider, free_port, request_json, admin_request, wait_for_gateway, percentile · Measure local ExoRoute request-path overhead against a deterministic mock.
- `scripts/check_source_boundaries.mjs` — 149 lines · root, budget, oversizedAllowlist, sourceRoots, extensions, collect · const root = process.cwd();
- `scripts/split_inventory.py` — 94 lines · name_of, inventory · Inventory a Rust file into top-level chunks (header + items).

## sketch
- `sketch/build-css.mjs` — 115 lines · require, postcss, here, webStyles, PAGE_IDS, PAGE_ROUTE_IDS · Class names
- `sketch/build-pages.mjs` — 50 lines · root, read, head, shellTop, shellBottom, pages · const root = dirname(fileURLToPath(import.meta.url));
- `sketch/main.html` — 0 lines · [file too large]

## sketch/pages
- `sketch/pages/apikeys.css` — 283 lines · course-row key cards, budget meters, modal fields). */
- `sketch/pages/apikeys.html` — 388 lines · <div class="page-heading">
- `sketch/pages/cards.css` — 264 lines · .variants-grid {
- `sketch/pages/cards.html` — 195 lines · <div class="page-heading">
- `sketch/pages/chat.css` — 228 lines · .chat-surface { display: flex; flex-direction: column; min-height: 0; }
- `sketch/pages/chat.html` — 268 lines · <div class="page-heading">
- `sketch/pages/combos.css` — 144 lines · .combo-board { display: flex; flex-direction: column; }
- `sketch/pages/combos.html` — 504 lines · <div class="page-heading">
- `sketch/pages/docs.css` — 257 lines · .docs-layout {
- `sketch/pages/docs.html` — 203 lines · <div class="page-heading">
- `sketch/pages/help.css` — 538 lines · reference, and the guide sidebar. */
- `sketch/pages/help.html` — 170 lines · <div class="page-heading">
- `sketch/pages/overview.css` — 48 lines · .chart-tooltip {
- `sketch/pages/overview.html` — 350 lines · <div class="page-heading">
- `sketch/pages/provider.css` — 265 lines · .back-link {
- `sketch/pages/provider.html` — 634 lines · <div class="page-heading">
- `sketch/pages/providers.css` — 217 lines · .provider-section { display: flex; flex-direction: column; }
- `sketch/pages/providers.html` — 593 lines · <div class="page-heading">
- `sketch/pages/proxies.css` — 45 lines · .dashboard-app.dashboard-page-proxies .proxy-toolbar { margin: 12px 0 16px; }
- `sketch/pages/quota.css` — 45 lines · .quota-board { display: flex; flex-direction: column; }
- `sketch/pages/quota.html` — 316 lines · <div class="page-heading">
- `sketch/pages/requests.css` — 161 lines · .req-board { display: flex; flex-direction: column; }
- `sketch/pages/requests.html` — 422 lines · <div class="page-heading">
- `sketch/pages/settings.css` — 83 lines · .dirty-badge {
- `sketch/pages/settings.html` — 327 lines · <div class="page-heading">
- `sketch/pages/stats-cards.css` — 241 lines · .variants-grid {
- `sketch/pages/stats-cards.html` — 169 lines · <div class="page-heading">

## sketch/parts
- `sketch/parts/head.html` — 0 lines · [file too large]
- `sketch/parts/shell-bottom.html` — 108 lines · </div>
- `sketch/parts/shell-top.html` — 184 lines · <body data-page="{{PAGE_ID}}">

## src
- `src/main.rs` — 22 lines · admin, app, config, gateway, infra, protocol · 11 modules
- `src/static_assets.rs` — 68 lines · serve · body::Body,

## src/admin
- `src/admin/api_keys.rs` — 723 lines · list_api_keys, api_key_json, api_key_request_count, validated_scope, api_key_index_key, api_key_token_index_key · 5 public items, 1 modules
- `src/admin/auth.rs` — 419 lines · admin_login, parse_auth_json, hash_admin_password, verify_admin_password, is_supported_admin_password_hash, hash_admin_password_async · 6 public items
- `src/admin/cookie_policy.rs` — 246 lines · cookie_name, refresh_cookie, is_valid_token, cookie_is_secure, trusted_forwarded_scheme, same_origin_rejection · 7 public items
- `src/admin/database.rs` — 733 lines · RestoredRuntimeSettings, BackupExportError, normalize_provider_defaults, index_provider_record, index_provider_api_key, index_provider_model · 3 public items, 9 modules
- `src/admin/errors.rs` — 192 lines · fail, internal, internal_message, storage_error_response, resource_id, admin_rate_limited_response · 6 public items, 1 modules
- `src/admin/gates.rs` — 125 lines · rate_limit_admin_login, require_admin, unauthorized_response · Json,
- `src/admin/metrics.rs` — 65 lines · prometheus_metrics · pub(crate) async fn prometheus_metrics(State(state): State<AppState>) -> axum::response::Response {
- `src/admin/mod.rs` — 93 lines · cleanup_stale_database_temp_dirs · 1 public items, 21 modules
- `src/admin/model_probes.rs` — 763 lines · ModelTestResult, test_models, parse_probe_target, acquire_probe_permit, load_probe_target, ProbeConnectionConfig · 2 public items, 1 modules
- `src/admin/overview.rs` — 85 lines · overview, overview_snapshot · 1 modules
- `src/admin/providers.rs` — 97 lines · budgets, crud_create, crud_update, custom_headers, deletion, indexing · 1 modules
- `src/admin/request_logs.rs` — 807 lines · RequestLogCursor, list_requests, RequestLiveKeyNames, lookup_request_api_key_name, request_events, request_live_snapshot_event · 1 public items, 1 modules
- `src/admin/router.rs` — 288 lines · provider_auth_callback_routes, provider_routes, combo_routes, proxy_routes, workspace_routes, operations_routes · 1 public items
- `src/admin/sessions.rs` — 78 lines · IssuedAdminSession, RefreshResult, RotationCommit, token_digest, new_token, db_now_unix_seconds · 6 public items, 1 modules
- `src/admin/statistics.rs` — 87 lines · default_range, range_window, get_statistics, get_api_key_statistics · 1 public items
- `src/admin/tests.rs` — 373 lines · provider_preset_catalog_exposes_categories_labels_and_adapter_ids, codex_provider_requires_its_fixed_responses_endpoint_without_api_keys, devin_cli_provider_accepts_the_local_process_endpoint, header_auth_defaults_the_api_key_header_name, provider_model_prefixes_are_normalized_and_bounded, models_url_preserves_existing_v1_or_models_path · AppState, ProviderInput, list_provider_presets, normalized_provider_model_prefix,
- `src/admin/update_check.rs` — 321 lines · check_for_updates, update_check_unavailable, parse_release_version, parse_release_number, valid_release_identifier, compare_release_versions · 1 modules

## src/admin/combos
- `src/admin/combos/mod.rs` — 553 lines · yes, priority, all_protocols, list_routes, list_combos, list_provider_options · 4 public items, 4 modules
- `src/admin/combos/references.rs` — 499 lines · ComboStore, ComboReferences, node_from_targets, ComboNodeReader, ComboNodeReader, ComboNodeReader · 12 public items
- `src/admin/combos/storage.rs` — 221 lines · route_record, TargetRecordWrite, route_target_records, route_target_prefix, route_target_key, route_name_index_key · 11 public items
- `src/admin/combos/tests.rs` — 823 lines · combo_with_target_count, reference_target, combo_with_references, provider_target, error_message, assert_ok · ComboReferenceError, ComboReferenceIssue, PendingCombo, validate_pending_combo,
- `src/admin/combos/validation.rs` — 188 lines · apply_path_id_for_update, validate_saved_combo_models, validate_combo · 2 public items

## src/admin/database
- `src/admin/database/backup_format.rs` — 273 lines · write_backup_archive, decode_backup_file · 4 public items
- `src/admin/database/export.rs` — 152 lines · export_database, export_database_inner · Json,
- `src/admin/database/import.rs` — 229 lines · import_database, receive_import_upload · Json,
- `src/admin/database/preflight.rs` — 225 lines · preflight_backup_reader · 1 public items
- `src/admin/database/restore.rs` — 197 lines · spawn_restore_reconciliation_task, restore_application_data_with_runtime, restore_application_data_inner · 1 public items
- `src/admin/database/temp_dir.rs` — 94 lines · TempDirectory, temp_directory_cleanup_sender, Drop, cleanup_stale_temp_directories, create_private_temp_dir · 3 public items
- `src/admin/database/validate.rs` — 751 lines · validate_backup_payload, BackupComboGraph, super, null_text_field, null_integer_field, route_target_key_components · 2 public items

## src/admin/database/tests
- `src/admin/database/tests/format.rs` — 191 lines · backup_header_checks_format_size_and_checksum, streamed_backup_writer_matches_the_in_memory_wire_format, backup_preflight_rejects_forged_lengths_before_deserialization, production_backup_decoder_preflights_forged_record_field_counts_and_lengths, malformed_backup_is_rejected_before_database_changes · fn backup_header_checks_format_size_and_checksum() {
- `src/admin/database/tests/mod.rs` — 15 lines · format, restore_atomicity, settings, support, tempdir, validation · 6 modules
- `src/admin/database/tests/restore_atomicity.rs` — 418 lines · backup_restore_rebuilds_secondary_indexes_from_primary_records, backup_restore_rebuilds_pending_provider_deletion_marker, backup_restore_is_atomic_and_excludes_sessions_and_stream_state, restore_worker_reconciles_state_after_its_response_handle_is_dropped · fn backup_restore_rebuilds_secondary_indexes_from_primary_records() {
- `src/admin/database/tests/settings.rs` — 198 lines · corrupt_output_styles_backup_is_rejected_without_changing_live_state, legacy_backup_without_output_styles_preserves_current_value_and_advances_revision, request_log_snapshot_option_and_key_usage_redaction_are_applied · async fn corrupt_output_styles_backup_is_rejected_without_changing_live_state() {
- `src/admin/database/tests/support.rs` — 136 lines · temporary_backup_path, test_route_target_key, write_raw_backup_archive, raw_backup_payload, route_backup_fixture · 4 public items
- `src/admin/database/tests/tempdir.rs` — 49 lines · stale_temp_cleanup_removes_only_owned_backup_directories, temp_directory_drop_queues_cleanup_off_runtime_workers · fn stale_temp_cleanup_removes_only_owned_backup_directories() {
- `src/admin/database/tests/validation.rs` — 730 lines · backup_route_entry, backup_nested_target_entry · fn backup_route_target_ordinals_respect_the_configured_target_limit() {

## src/admin/provider_api_key_auth
- `src/admin/provider_api_key_auth/callback.rs` — 278 lines · read_callback_payload, resolve_callback_flow, parse_callback_payload, encrypt_callback_key, callback, preflight · 1 public items
- `src/admin/provider_api_key_auth/flow.rs` — 154 lines · ApiKeyAuthApplyClaim, claim_api_key_auth_apply, ApiKeyCallbackAcceptance, acce…
(rendered map truncated at 16384 bytes)
<!-- dsh-openwolf:end -->
