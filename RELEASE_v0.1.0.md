# ExoRoute v0.1.0

Initial source release of ExoRoute, a local-first AI protocol gateway and model router built with Rust, Axum, LMDB, and Svelte 5.

Release date: 2026-09-19

## Highlights

- Routes requests through OpenAI Chat Completions, OpenAI Responses, and Anthropic Messages-compatible APIs.
- Translates supported client and upstream provider protocols through one gateway endpoint.
- Provides provider presets and adapter integrations for Kilo AI Gateway, OpenAI Codex, Command Code, OpenCode Go, OpenCode Zen, OpenRouter, Freebuff, Antigravity, Cline, ClinePass, and NVIDIA NIM.
- Supports provider model discovery, manual model catalogs, multiple provider API keys, key rotation, OAuth accounts, model testing, and combo-based priority or round-robin fallback.
- Includes a bundled Svelte dashboard for providers, combos, API keys, request logs, quota, statistics, operational settings, output styles, and stream continuity.
- Uses an embedded LMDB environment with versioned database backup and restore flows; existing SQLite files are not migrated or read.
- Includes bounded request/response processing, configurable concurrency and stream resources, retry handling, request telemetry, and graceful stream cancellation/continuity behavior.

## Security and deployment defaults

- The HTTP listener is loopback-only by default and rejects non-loopback binds because ExoRoute does not provide TLS.
- Gateway and dashboard administration require separate authentication controls.
- Provider credentials are encrypted at rest using the configured master key.
- Provider destinations are checked by the egress/SSRF protection layer before outbound requests.
- Local `.env` files, database directories, backups, build output, dependencies, and credential material are excluded from Git by `.gitignore`.

For remote access, place ExoRoute behind a correctly configured TLS reverse proxy and keep the ExoRoute listener bound to loopback.

## Antigravity OAuth configuration

Release builds include the public Antigravity desktop OAuth client, so end users do not need a `.env` entry for the default flow. Operators can override it in the local, ignored `.env` file when using a different OAuth client:

```dotenv
EXOROUTE_ANTIGRAVITY_OAUTH_CLIENT_ID=your-client-id
EXOROUTE_ANTIGRAVITY_OAUTH_CLIENT_SECRET=your-client-secret
```

The bundled values are public client credentials, not a secure place to store a private application secret. Do not reuse credentials that have been exposed in a rejected or local Git commit; revoke and rotate confidential credentials first.

## Dashboard and storage

On first run, ExoRoute creates its application data under `~/.exoroute` (`%USERPROFILE%\.exoroute` on Windows), including the `.env` file and LMDB environment. The generated bootstrap administrator password must be changed before gateway requests are enabled.

The dashboard supports database export/import with validation, reauthentication, version checks, and atomic restore behavior. Keep the configured `EXOROUTE_MASTER_KEY` stable when encrypted provider credentials or backups need to remain usable.

## Build and release targets

The release workflow builds the dashboard before the Rust binary, runs platform checks, creates checksummed archives, and publishes signed release manifests for these configured targets:

- Linux x86_64
- Linux aarch64
- macOS x86_64
- macOS aarch64
- Windows x86_64

The release tag must match the package version: `v0.1.0`.

## Verification

The following checks passed for the source snapshot used for this release note:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo check --workspace --all-targets --all-features`
- `cargo check --release`
- Antigravity Rust tests: 22 passed
- `npm run check`: 0 errors and 0 warnings
- Frontend tests: 30 Node tests and 2 UI tests passed
- `npm run build`

The complete Rust workspace test command was not marked as passed: it stalled at `admin::database::tests::restore_worker_reconciles_state_after_its_response_handle_is_dropped` during local validation and was stopped. The pinned CI toolchain is Rust 1.88; local formatting and compilation checks used the available toolchain after confirming the source contract.

## Known limitations

- Antigravity uses upstream Cloud Code APIs that are not documented or guaranteed to remain stable.
- Stream continuity cannot survive a gateway process restart.
- Provider-reported quota data is best-effort and may be unavailable or stale.
- Performance depends on the host, provider, configuration, and workload; use `scripts/benchmark.py` for local measurements rather than treating the benchmark as a production capacity claim.

## License

MIT. See [LICENSE](LICENSE).
