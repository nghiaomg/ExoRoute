# ExoRoute v0.1.3

Release Date: 2026-11-28

ExoRoute v0.1.3 turns the gateway into a zero-restart platform: the user-editable provider catalog is now templated, validated, watched, and reloadable from both the dashboard and the terminal. It adds a self-service key portal, automatic OpenCode model discovery, transparent cost fallback, protocol-shaped gateway errors, a compact rewritten README, and release-ready branch builds for Linux, Windows, and macOS.

---

## Highlights

### User-editable provider catalog
- `~/.exoroute/providers.yaml` is created once on startup and never overwritten.
- Valid edits are hot-applied by a background watcher; invalid YAML is rejected and the last known-good snapshot stays active.
- New admin endpoints: `GET /api/v1/admin/provider-presets` now serves the live runtime catalog; `POST /api/v1/admin/provider-presets/reload` applies a validated reload and reports provider/overlay counts.
- Terminal shortcut: press `[r]` + Enter to reload manually; a dedicated hint is printed on interactive terminals.
- New provider creation seeds default models from the live catalog, including user overrides.
- Model overrides support per-model pricing, context/output limits, and are validated before activation.

### OpenCode without manual reconfiguration
- Unknown OpenCode Go/Zen model ids now pass through with the provider's default adapter and endpoint instead of returning 404.
- A bounded background sync queries configured OpenCode providers' `/models` endpoint and registers only missing ids.
- Explicit `providers.yaml` overrides, `remove_models`, and manual/database entries are never overwritten; upstream failures keep the cached catalog.

### Key Portal (self-service statistics)
- `POST /api/v1/key-portal/login` accepts an `exo_...` gateway key and sets a distinct HttpOnly `exoroute_key_session` cookie.
- `GET /session`, `GET /stats`, and `POST /logout` are scoped strictly to the signed-in key.
- Statistics reuse the existing per-key telemetry snapshot; recent requests are filtered by the session key id.
- Security hardening: fail-closed signing secret, key-version binding, bounded logout revocation, dedicated login rate limiter, `Cache-Control: no-store`, cookie path restricted to `/api/v1/key-portal`.

### Gateway reliability and cost visibility
- Fallback `cost_micro_usd` is computed from `providers.yaml` pricing whenever an upstream omits `usage.cost`, in both streaming and non-streaming paths; upstream-reported cost is never overwritten.
- Gateway timeout and final failure responses are protocol-shaped for Anthropic Messages clients.
- Streaming keepalive frames match the client protocol and survive silent upstream pauses.
- Combo resolution handles renamed combos through the combo name index; Devin CLI transport cleanup, retry, text-chunk parsing, and duplicate-auth fixes landed.

### Dashboard and documentation
- Shared dialog primitives replaced raw modal markup; duplicate modal padding removed.
- Provider detail header actions use compact 12px controls; the leaked native toggle checkbox is hidden while keyboard accessibility is preserved.
- Provider presets, API keys, combos, overview charts, themes, and settings received broad consistency and contrast fixes.
- README rewritten as a compact English overview with a prominent cold-start/resource benchmark and provider coverage table (43 presets: 23 cloud APIs, 11 gateways, 9 OAuth subscriptions, plus custom).
- Full Vietnamese/English locale coverage for new UI copy.

### Engineering and CI
- Rust clippy runs with `-D warnings` in CI; all in-scope warnings, dead code, and formatting issues were cleared.
- Source-boundary invariants enforced; test-only modules moved to approved layouts.
- Full suite is green at default concurrency; the timing-sensitive telemetry throughput test was stabilized without weakening assertions.
- `.github/workflows/branch-build.yml` builds Linux, Windows, and macOS (Intel + Apple Silicon) binaries on `develop` and `main`; `release.yml` continues to produce signed `v*` release assets.
- npm launcher version aligned with the Rust package version at 0.1.3.

---

## Compatibility and upgrade notes

- No database schema migration is required.
- Existing `providers.yaml` files are preserved; new installations receive a commented template.
- Admin session behavior and the `/api/v1/admin/*` contract remain unchanged; add the `/api/v1/admin/provider-presets/reload` endpoint to clients that want manual reloads.
- Gateway key format remains `exo_...`; Key Portal is additive.
- Build from source with Rust 1.88+ and Node.js 22+.

---

## Full commit history since v0.1.2

- 35fae21 2026-11-28 docs: rewrite README as compact English overview with benchmark
- 98634fe 2026-11-27 ci: add branch build matrix for Linux, Windows and macOS
- d358d74 2026-11-27 fix: satisfy clippy search_is_some in key portal test
- 32aa7dc 2026-11-26 feat: add key portal, opencode auto-discovery and gateway error hardening
- 0a26f2f 2026-11-26 fix: resolve CI clippy -D warnings and provider detail header controls
- ede32e5 2026-11-26 feat: ship provider catalog hot reload, dialog standardization and final integration fixes
- 7818dc6 2026-11-22 chore: complete theme contrast fixes and remaining clippy cleanups
- b3c45b6 2026-11-22 chore: clear in-scope Rust clippy and dead-code findings
- 307ce2e 2026-11-22 chore: fix Rust CI formatting and source-boundary failures
- 0eb33ad 2026-11-22 chore: integrate wave 1 fixes and dashboard redesign into develop
- 25b0f16 2026-11-16 Fix: models free of opencode
- e047fb6 2026-11-14 feat(devin-cli): delete the per-request CLI session from the transport's store
- 3c680fe 2026-11-14 fix(gateway): resolve renamed combos through the combo name index
- 82855d3 2026-11-14 fix(devin-cli): retry a capacity-refused turn once against a fresh session
- 38c7a29 2026-11-14 feat(combos): unified searchable target picker, popup sizing fixes, drag-drop reorder
- cd01c6d 2026-11-14 fix(devin-cli): strip the summarizer's <summary> envelope before it reaches the client
- 9670b92 2026-11-14 fix(opencode-zen): drop the duplicate free-tier Authorization header Cloudflare rejects
- ef1eaf5 2026-11-12 feat: enhance combo and provider UI with compact controls and improved styling
- dc94ae3 2026-11-12 Style: fix styling
- b4a2386 2026-11-12 fix: devin cli
- 4810938 2026-11-11 components
- d279d20 2026-11-11 feat(web): align shared cards and controls with approved designs
- a40b92b 2026-11-11 Remove unused CSS files for help, overview, providers, quota, requests, and settings pages to streamline the codebase and improve maintainability
- ace26f0 2026-11-11 feat: Refactor input handling across various components
- cb37d15 2026-11-10 fix(providers): hide redundant preset copy
- 50aa1ac 2026-11-10 fix(devin-cli): parse text chunks and reject empty turns
- b9ff120 2026-11-10 feat: enhance dashboard requests page with route alias display and localization updates
- 0e2721a 2026-11-10 fix(web): refresh provider list from section
- 150da4f 2026-11-10 feat(web): show full provider preset catalog
- 5287c9d 2026-11-10 refactor(web): use shared selects across dashboard
- 40d28e7 2026-11-10 feat(web): enrich request usage details
- 0ceeeec 2026-11-10 refactor(web): centralize form control states
- 6886d4f 2026-11-10 feat(web): add API key detail and refine providers
- 4aa2e77 2026-11-10 fix(proxy): scope route stylesheet once
- 3115d57 2026-11-10 i18n(proxy): translate traffic usage cards
- 4aaea6e 2026-11-10 feat(proxy): show runtime usage and throughput stats
- ca4abb7 2026-11-10 fix(gateway): release body permits after request intake
- 5bba160 2026-11-10 feat(web): split settings panels and help content
- c732910 2026-11-10 feat(i18n): translate dashboard additions
- 1a6c439 2026-11-10 feat(providers): add focused account and model panels
- 4fc84a6 2026-11-10 feat(web): refresh dashboard pages and navigation
- 431c7f7 2026-11-10 feat(proxy): rotate proxy endpoints at runtime
- 491d762 2026-11-10 feat(gateway): record route metadata and recover provider keys
- 388213e 2026-11-10 feat(proxy): add nested proxy groups and provider binding
- eb9326d 2026-11-09 feat(web): replace sketch pages with dashboard UI
- 79981c7 2026-11-09 feat(combos): retry exhausted combos until success
- feb9d66 2026-11-09 fix(gateway): preserve keys on non-credential 403 responses
- 422f4b2 2026-11-09 chore(sketch): drop generated page mockups from the tree
- 58aab46 2026-11-09 feat(web): route sketch pages through SketchShell
- 6994815 2026-11-09 feat(web): port help center page to sketch design
- 85310ad 2026-11-09 feat(web): port settings page to sketch design
- 39ba320 2026-11-09 feat(web): port api keys page to sketch design
- 224714c 2026-11-09 feat(web): port chat page to sketch design
- ecef472 2026-11-09 feat(web): port requests page to sketch design
- 2bea1f4 2026-11-09 feat(web): port quota page to sketch design
- 9bc6bdb 2026-11-09 feat(web): port combos page to sketch design
- 8a4294e 2026-11-09 feat(web): port providers page to sketch design
- f84c353 2026-11-09 feat(web): port overview page to sketch design
- 99b47ad 2026-11-09 feat(web): add sketch shell, icon sprite and helpers
- 1686a72 2026-11-09 feat(web): add scoped sketch stylesheets
- 11a7793 2026-11-09 feat(sketch): add static dashboard redesign prototype
- f4c4ae8 2026-11-09 refactor(admin): box callback error responses; deflake telemetry restart test
- f27e5c9 2026-11-09 refactor(chat): split chat.state into per-responsibility modules
- 34356a0 2026-11-09 refactor(antigravity): accumulate event stream in a dedicated parts state
- c0a7afe 2026-11-09 refactor(output-styles): name bypass stages and term tables
- ca1d7e7 2026-11-09 refactor(protocol): split content-part parsing per block kind
- 108672a 2026-11-09 refactor(admin): pipeline the provider API-key callback
- a27f69e 2026-11-09 refactor(app): stage bootstrap into named phases
- 1e75736 2026-11-09 refactor(kiro): split frame_to_chunks per event type
- f6f52e9 2026-11-09 refactor(protocol): split response encoder per wire protocol
- 3445a50 2026-11-09 refactor(admin): group route table into per-resource sub-routers
- 0fae2f2 2026-11-09 refactor(admin): split model probe into staged helpers
- 1fa4d77 2026-11-09 refactor(admin): split backup index rebuild into per-table helpers
- 4ce5190 2026-11-08 refactor(time): centralize unix clock helpers in support::time
- c836785 2026-11-08 refactor(auth): share token helpers across provider auth modules
- b76aeed 2026-11-08 style(themes): drop neo-brutalism from the default classic theme
- cc1d1d1 2026-11-08 feat(providers): add Kiro AI provider with device auth and token import
- 4268730 2026-11-08 feat(providers): import Antigravity CLI (agy) logins
- 69ceb65 2026-11-08 feat(opencode): allow keyless auth for the Zen free tier
- 3620c8e 2026-11-08 feat(api-keys): move the access scope editor into a dialog
- 1bb1974 2026-11-08 feat(web): use the nvidia logo for NVIDIA providers and models
- e2b4e05 2026-11-08 revert(admin): remove the Devin cloud-agent task manager page
- 7ea1924 2026-11-08 chore(assets): track the devinai provider logo
- 4920bff 2026-11-08 test(web): derive release notice versions from the running version
- 41cfb8c 2026-11-08 feat(web): use devinai logo for Devin providers and models
- 78ecdcf 2026-11-08 feat(quota): cache credential states and add 60s auto-refresh toggle
- 34ac3fd 2026-11-08 fix(admin): normalize auth type for credential-less adapters
- 07bc8f1 2026-11-08 fix(admin): keep auth_type none for auth-none-only provider transports
- ec3b071 2026-11-08 fix(providers): drop api_keys capability from the devin_cli preset
- a6c35d5 2026-11-08 chore: bump version to 0.1.3
- f64620f 2026-11-08 feat(admin): default auth_header to x-api-key for header authentication
- a2344aa 2026-11-08 fix(admin): allow adapter-owned transport endpoints in provider validation
- 14dd4a5 2026-11-08 feat(gateway): support -effort suffix on provider-prefixed model aliases
- 5d501c7 2026-11-08 chore: remove accidentally committed tooling artifacts
- 4606ed0 2026-11-08 release 110802026
- 8af1de4 2026-11-08 feat(app): persist error-level logs to a rotated error-logs.txt
- 8ccc819 2026-11-08 feat(providers): add the Devin Desktop (Windsurf) adapter, experimental
- fbbf3e4 2026-11-08 feat(admin): add a Devin cloud-agent task manager
- c0f53cf 2026-11-08 feat(providers): add a Devin CLI local transport adapter
- 42588ca 2026-11-07 feat(web): refresh the traffic summary from the live request feed
- 762cbbc 2026-11-07 feat(web): collapse long combo chains behind a show-all toggle
- 712b090 2026-11-07 feat(web): rework combo card chain into a timeline rail
- dc61767 2026-11-07 style(gateway): cargo fmt the rate-limit header helper
- 92d4052 2026-11-07 feat(web): show a failure-rate alert banner in the shell
- 23cae18 2026-11-07 feat(requests): export all filter matches as CSV
- dc61e4f 2026-11-06 feat(requests): persist the filter set across reloads
- 2c5eae8 2026-11-06 feat(web): add installable PWA manifest and icons
- 06ec6a1 2026-11-06 feat(gateway): emit X-RateLimit headers on every keyed request
- 06804f8 2026-11-06 feat(health): include the package version in health responses
- cd698e3 2026-11-06 feat(web): surface a release update notice with full locale coverage
- 6a056e1 2026-11-06 fix(requests): let the table card grow with its rows in the page scroll
- 543d306 2026-11-06 fix(requests): keep the filter panel from stacking over the table card
- 75dd453 2026-11-05 fix(requests): theme the statistics details panel for dark and palette themes
- b9ceac4 2026-11-05 fix(requests): keep the statistics details panel from collapsing on desktop
- 5b5237b 2026-11-04 docs(i18n): translate the new provider presets and refresh the README matrix
- a77e578 2026-11-04 feat(providers): add a Google Vertex AI adapter
- dd2f890 2026-11-04 feat(providers): add an Azure OpenAI adapter
- 2763c4f 2026-11-04 feat(providers): add local inference server presets
- be88584 2026-11-04 feat(providers): add eight OpenAI-compatible cloud adapters
