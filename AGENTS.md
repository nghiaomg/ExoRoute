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
- Keep module boundaries cohesive. Split a module when it owns independent responsibilities or lifecycle/state boundaries, not to hit an arbitrary line count. Do not create wrapper, pass-through, generic, or “base” abstractions that only move code or forward arguments without reducing coupling or complexity.
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
