# ExoRoute v0.1.1

Second source release of ExoRoute, a local-first AI protocol gateway and model router built with Rust, Axum, LMDB, and Svelte 5.

Release date: 2026-10-06

Base for this release: `98c7119a623192f6a9218abcd9a2d94309118847` (`Merge branch 'fix/admin-auth-same-origin' into develop`).

## Highlights

### Protocol and provider fixes

- Fixes HTTP 400 responses from strict proxies such as Command Code: non-empty tool results are now encoded as text parts instead of bare strings in content arrays, and `reasoning_effort` values are normalized for Command Code (`minimal`/`none` → `low`, `ultra` → `max`) case-insensitively.
- Workspace relay errors now follow the gateway error sanitizer format and include the provider id and a sanitized upstream body.
- Translates Chat Completions streamed tool calls to Responses and Messages clients so tool calls work cross-protocol while streaming.
- Forwards `prompt_cache_key` between OpenAI-family protocols.
- Decodes Codex `custom_tool_call` items as tool calls.
- Stops replaying assistant reasoning text into Responses input items.
- Enables per-model protocol routing for OpenCode Go and refreshes its endpoint mappings.
- Treats pre-terminal finish signals as clean stream completion.
- Keeps streaming responses alive past the per-request timeout.
- Prevents stale upstream connection resets on reused connections.
- Reports why an upstream response could not be decoded. Providers routinely answer with HTTP 200 and a body the decoder rejects, and the previous client-visible error was the decoder's own summary, which made an error envelope, a truncated stream, and an unrecognized shape look identical. Undecodable responses now include the provider's own diagnostic, redacted like every other provider error body and truncated to a fixed 300-character budget, so credential material and request content still never reach the client.
- Records a stream that ends without a terminal event as an interrupted request at the moment it stops, instead of when the client body is finally dropped. A gateway shutdown is no longer counted in the totals while missing from request history.

### Reliability and data safety

- Stores request logs as zstd frames built from a dictionary trained on the environment's own request history, which is what makes keeping request history affordable on disk. The dictionary is persisted in the database, loaded when the environment opens, refreshed by the telemetry maintenance tick, and re-derived from a snapshot during backup restore, so restored frames decode without depending on the live environment. Reads accept both the compressed frame and the legacy raw layout, so an existing environment keeps reading every record it already holds and only writes the new format from now on.
- Fixes the `record decompression failed: Src size is incorrect` failures. The read, write, and snapshot paths now report a size or layout mismatch explicitly instead of letting a damaged frame reach the decoder, and the codec's format marker is meaningful only for request-log values, so a raw stored value that happens to begin with the marker byte cannot be misread as a frame.
- Adds `zstd` as a dependency for the request-log codec, with default features disabled and only `zdict_builder` enabled.
- Writes settings secrets to a temporary file and renames it into place instead of truncating the live file first, and removes orphaned temporary files at startup when they are older than any possible live write. A process killed mid-write can no longer strand a half-written `.env` or leave debris behind.
- Bounds the telemetry drain with a budget the writer itself enforces, with the caller passing a slightly larger outer budget, so a stuck writer is reported with its own reason instead of a generic outer-deadline timeout.
- Reports the number of in-flight LMDB operations before the runtime shutdown deadline can abandon them. That count is the only trace of an operation dropped mid-write, and it is the difference between "the database is corrupt" and "the database was interrupted while writing".

### Admin dashboard authentication

- Fixes a 403 that broke every session refresh and logout: `{"error":{"message":"same-origin browser request required"}}`. The same-origin check required a `Sec-Fetch-Site: same-origin` header, which browsers send only to potentially trustworthy URLs and which engines predating Fetch Metadata (Safari before 16.4) never send at all — so a first-party dashboard reached over plain HTTP by a name other than `localhost` was rejected for not describing itself in a header it had no way to send. `Sec-Fetch-Site` is now validated when present, and a cross-site or duplicated value is still rejected, while the authoritative test remains the `Origin`/`Host` comparison, which page script cannot forge. A non-browser client that sends no `Origin` is still rejected, and so is an HTTPS dashboard behind a proxy that has not declared the proxied scheme.
- Logs a rejection's fixed reason label at debug level only. Both endpoints run before their rate limiter, so a warning would let an unauthenticated caller flood the log, and the label is a constant rather than anything derived from the request, so a rejection never echoes client-controlled header content.
- Keeps the browser's `Host` header in the Vite development proxy (`changeOrigin: false`). Rewriting it to the API authority made the forwarded `Host` disagree with the browser's `Origin`, which reproduced the same 403 on every refresh and logout from the dev server.

### Features

- Localized documentation surface: Docs home, quickstart, integration guides (Cursor, Continue, Cline, Roo Code, SDKs), and API reference, translated for the dashboard locales.
- Kilo Code account connection via device authorization flow.
- Provider model sources: imported model lists are now distinguished from manually added models.
- Request logs show input and output token usage, including live token usage during streaming, with a compact display.
- Chat Completions request options are translated to Responses equivalents.
- Gateway API key access scopes: each key can be restricted to a bounded list of provider ids and a bounded list of model rules, where a rule is an exact model name or a trailing-`*` prefix. A missing or empty list means unrestricted, so every existing key keeps working unchanged. Requests outside a key's scope are rejected before any upstream is contacted. Scope entries are validated when written and re-validated on backup import — entry count, entry length, total size, whitespace, control characters, and wildcard placement — and the admin UI gains a scope editor that mirrors the same rules while the backend stays authoritative.
- The chat playground gains a command palette, side-by-side model compare, a prompt library, a session meter, per-turn actions, and markdown rendering, with its state extracted into a feature module. Its transport is a new admin relay that reuses the provider adapters for request preparation, auth, and bounded response reading but stores nothing: no request log, no telemetry, no usage meter, and no credential state. The streaming form converts each upstream frame into at most one canonical delta for the provider's protocol and makes exactly one upstream attempt — once a provider may have accepted or billed an inference the request is neither retried nor failed over, and a mid-stream failure reaches the dashboard as a terminal error event.
- Rebuilds the requests page as a metrics surface: a detail drawer, a filter panel, live follow, an outcome bar, a metrics strip, and skeleton loading states. The telemetry query behind it now reports whether the scan or the output was truncated and how many retained rows it covered, so the dashboard can distinguish a bounded history slice from the unbounded rolling totals instead of presenting one as the other.
- Dashboard shell: dark mode drops the light theme's 2px ink borders and hard offset shadows and relies on per-surface backgrounds for separation; the sidebar's collapsed state is a stored preference read through a guarded browser-storage access, so restricted contexts fall back to the default instead of throwing; and a feature bundle that fails to import renders a retryable error rather than an unexplained loading screen that never resolves, because the browser memoizes a failed dynamic import for the document. The provider-only toast is replaced by a shared toast component.

### Refactoring and code layout

- Splits long modules into focused ones: streaming translation and Google protocol, chat stream tool-call merge, provider adapter auth (Antigravity, Cline, Codex), security egress and rate limiting, database settings records, and admin provider usage views.
- Dashboard decomposition: session restore, the feature registry, shared types, request formatting, and liveness helpers moved into `web/src/lib/`.
- Test modules split per concern with a shared mock upstream harness.
- Refreshed the agent guide for the new module layout.
- Extracts the dashboard shell from the app root, and splits the requests and chat surfaces into their own components and state modules so page components orchestrate instead of owning every detail. Request-log dictionary, streaming relay, and canonical delta extraction each live in their own module.

### Dashboard redesign (Soft Neo-Brutalism) and fixes

- Introduces a unified Soft Neo-Brutalism design language across the dashboard: 2px ink borders, hard offset ink shadows, lift-on-hover and press-on-active affordances, and the brand orange accent for focus states. Applied to the login screen, primary/secondary/danger buttons, modals, forms, Ark select/combobox/field/password/checkbox controls, provider/combos/API keys/overview/settings surfaces, statistics cards, and the public docs surface.
- Rebuilds the Chat page as a playground: searchable model picker combobox with pill trigger, full-height layout with docked composer, collapsible chat settings, image/document attachments, suggestion cards, reasoning disclosure, and scroll-follow behavior that yields when the user scrolls up. All new strings are translated across the 12 supported locales.
- Fixes dark-mode regressions caused by CSS variables that did not exist in the token set; dark surfaces now use the real `--ink`/`--paper`/`--line` tokens with explicit dark overrides.
- Fixes nested-card rendering on API key cards: inner id codes, date tiles, and revoke controls are flat again inside the framed card; the same treatment is applied to overview chips, combo pipeline rows, and provider key rows.
- Performance: removes the full-height `backdrop-filter` blur layers from the sidebar and topbar (light and dark) and stops remounting the entire route on every dashboard refresh click, which together caused visible scroll jank and FPS drops on the Chat page.

## Security and deployment defaults

Unchanged from v0.1.0: the HTTP listener is loopback-only and rejects non-loopback binds; gateway and dashboard administration require separate authentication; provider credentials are encrypted at rest with the configured master key; egress/SSRF validation runs before outbound requests; the LMDB environment stays single-process with its process lock. Admin sessions keep the `/login` redirect, the forced password-change flow, the in-memory-only access token, the HttpOnly `SameSite=Strict` refresh cookie with single-use rotation, replay detection, and 24-hour idle / 7-day absolute expiry, and the one-use step-up proof issued after password reauthentication for database export and import.

Changes in this release that touch those boundaries:

- Gateway API key scopes are an authorization restriction only. An empty or missing scope preserves the previous unrestricted behavior, and scope parsing is shared between the gateway and the admin API so a stored scope cannot mean one thing when written and another when enforced.
- The dashboard chat playground's relay persists nothing — no request log, no telemetry, no usage meter, and no change to credential state — so prompts and responses sent through it are not written to the database. Only the selected model, the operator price table, and the prompt library are kept in browser storage.
- The same-origin check on admin refresh and logout no longer requires Fetch Metadata. The `Origin`/`Host` comparison, which page script cannot forge, is the authoritative test, a cross-site Fetch Metadata value is still rejected, and the check is not relaxed for non-browser clients or for an HTTPS dashboard behind a proxy that has not declared the proxied scheme.

For remote access, place ExoRoute behind a correctly configured TLS reverse proxy and keep the ExoRoute listener bound to loopback.

## Compatibility

- The storage and backup format versions are unchanged (`STORAGE_FORMAT_VERSION` 4, `BACKUP_FORMAT_VERSION` 1), and exports and imports remain versioned and validated. A backup carries the request-log compression dictionary as an ordinary stored record, so restoring a backup written by this release decodes its frames directly; restoring a backup from an earlier release has no dictionary until one is trained from the restored history, which does not prevent those records from being read.
- No configuration keys were removed. Existing provider setups, gateway API keys, and combos keep working without migration; a gateway key with no scope stays unrestricted, and a client that omits the optional scope fields sees the previous behavior.
- The command-line surface and default ports are unchanged.
- Downgrading after this release is not supported for request logs. Records written by this release are compressed frames, and an older build reads only the raw layout, so a database written here and then opened by an older build may fail to decode request logs. To go back to an earlier version, restore a backup taken before the upgrade.

## Build and release targets

The release workflow builds the dashboard before the Rust binary and publishes checksummed archives for these targets:

- Linux x86_64
- Linux aarch64
- macOS x86_64
- macOS aarch64
- Windows x86_64

The release tag must match the package version: `v0.1.1`.

## Verification

The following checks passed on the merged `develop` tree that this release note describes. The Rust checks ran on Rust 1.88.0, the toolchain the release workflow pins, which matters for lint results: a newer local toolchain had already demoted the one lint this release tripped on. The frontend checks ran on Node 24.19.0, not on the Node 22 that the workflow uses, so they are local results rather than workflow reproductions.

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features`: 531 passed, 0 failed
- `cargo check --workspace --all-targets --all-features`
- `cargo check --release`
- `node scripts/check_source_boundaries.mjs` (99 test-only/locale exemptions)
- `npm run check`: 4521 files, 0 errors and 0 warnings
- Frontend tests: 99 Node tests (14 files) and 34 UI tests (12 files) passed, 0 failed
- `npm run build`

Two changes in this release are reasoned, not measured. The FPS issue was addressed by code inspection and reasoning about paint cost (backdrop-filter layer size and route remounting), with no frame-time benchmark run. The request-log compression ratio depends on how repetitive a given workload's logs are; no benchmark was run, so the on-disk saving is not quantified here.

## Known limitations

- Antigravity uses upstream Cloud Code APIs that are not documented or guaranteed to remain stable.
- Stream continuity cannot survive a gateway process restart.
- Provider-reported quota data is best-effort and may be unavailable or stale.
- Request logs written by this release cannot be read by builds older than it; see Compatibility before downgrading.
- The chat playground transcript lives only in the dashboard tab. Its relay writes nothing to the database, and only the selected model, the operator price table, and the prompt library survive a reload.
- Performance depends on the host, provider, configuration, and workload; use `scripts/benchmark.py` for local measurements rather than treating the benchmark as a production capacity claim.

## License

MIT. See [LICENSE](LICENSE).
