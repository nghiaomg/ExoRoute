# ExoRoute v0.1.1

Second source release of ExoRoute, a local-first AI protocol gateway and model router built with Rust, Axum, LMDB, and Svelte 5.

Release date: 2026-09-30

Base for this release: `b7dc754484b35013ce37dda3a284865d34fd9507` (`merge(main): promote npm launcher`).

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

### Features

- Localized documentation surface: Docs home, quickstart, integration guides (Cursor, Continue, Cline, Roo Code, SDKs), and API reference, translated for the dashboard locales.
- Kilo Code account connection via device authorization flow.
- Provider model sources: imported model lists are now distinguished from manually added models.
- Request logs show input and output token usage, including live token usage during streaming, with a compact display.
- Chat Completions request options are translated to Responses equivalents.

### Refactoring and code layout

- Splits long modules into focused ones: streaming translation and Google protocol, chat stream tool-call merge, provider adapter auth (Antigravity, Cline, Codex), security egress and rate limiting, database settings records, and admin provider usage views.
- Dashboard decomposition: session restore, the feature registry, shared types, request formatting, and liveness helpers moved into `web/src/lib/`.
- Test modules split per concern with a shared mock upstream harness.
- Refreshed the agent guide for the new module layout.

### Dashboard redesign (Soft Neo-Brutalism) and fixes

- Introduces a unified Soft Neo-Brutalism design language across the dashboard: 2px ink borders, hard offset ink shadows, lift-on-hover and press-on-active affordances, and the brand orange accent for focus states. Applied to the login screen, primary/secondary/danger buttons, modals, forms, Ark select/combobox/field/password/checkbox controls, provider/combos/API keys/overview/settings surfaces, statistics cards, and the public docs surface.
- Rebuilds the Chat page as a playground: searchable model picker combobox with pill trigger, full-height layout with docked composer, collapsible chat settings, image/document attachments, suggestion cards, reasoning disclosure, and scroll-follow behavior that yields when the user scrolls up. All new strings are translated across the 12 supported locales.
- Fixes dark-mode regressions caused by CSS variables that did not exist in the token set; dark surfaces now use the real `--ink`/`--paper`/`--line` tokens with explicit dark overrides.
- Fixes nested-card rendering on API key cards: inner id codes, date tiles, and revoke controls are flat again inside the framed card; the same treatment is applied to overview chips, combo pipeline rows, and provider key rows.
- Performance: removes the full-height `backdrop-filter` blur layers from the sidebar and topbar (light and dark) and stops remounting the entire route on every dashboard refresh click, which together caused visible scroll jank and FPS drops on the Chat page.

## Security and deployment defaults

Unchanged from v0.1.0: the HTTP listener is loopback-only and rejects non-loopback binds; gateway and dashboard administration require separate authentication; provider credentials are encrypted at rest with the configured master key; egress/SSRF validation runs before outbound requests; the LMDB environment stays single-process with its process lock.

For remote access, place ExoRoute behind a correctly configured TLS reverse proxy and keep the ExoRoute listener bound to loopback.

## Compatibility

- The database format and backup/restore flow are compatible with v0.1.0; exports and imports remain versioned and validated.
- No configuration keys were removed. Existing provider setups, gateway API keys, and combos keep working without migration.
- The command-line surface and default ports are unchanged.

## Build and release targets

The release workflow builds the dashboard before the Rust binary and publishes checksummed archives for these targets:

- Linux x86_64
- Linux aarch64
- macOS x86_64
- macOS aarch64
- Windows x86_64

The release tag must match the package version: `v0.1.1`.

## Verification

The following checks passed on the working tree that this release note describes (includes the UI work listed above):

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features`: 473 passed, 0 failed
- `cargo check --release`
- `node scripts/check_source_boundaries.mjs` (94 test-only/locale exemptions)
- `npm run check`: 0 errors and 0 warnings
- Frontend tests: 49 Node tests and 9 UI tests (4 files) passed
- `npm run build`

The FPS issue was addressed by code inspection and reasoning about paint cost (backdrop-filter layer size and route remounting); no frame-time benchmark was run, so the improvement is not quantified.

## Known limitations

- Antigravity uses upstream Cloud Code APIs that are not documented or guaranteed to remain stable.
- Stream continuity cannot survive a gateway process restart.
- Provider-reported quota data is best-effort and may be unavailable or stale.
- Performance depends on the host, provider, configuration, and workload; use `scripts/benchmark.py` for local measurements rather than treating the benchmark as a production capacity claim.

## License

MIT. See [LICENSE](LICENSE).
