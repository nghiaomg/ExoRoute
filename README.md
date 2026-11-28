# ExoRoute

**A local-first AI gateway in a single Rust binary.**  
Route OpenAI, Anthropic, and Responses API traffic across **43 providers** with automatic failover, `providers.yaml` hot-reload, and a built-in dashboard — no Docker, no Postgres, no Redis.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Built%20with-Rust-orange.svg)](#build-from-source)
[![Releases](https://img.shields.io/github/v/release/nghiaomg/ExoRoute)](https://github.com/nghiaomg/ExoRoute/releases)

```text
Claude Code / Cursor / OpenAI SDK / Anthropic SDK
                      │
                      ▼  (127.0.0.1:8686)
              ┌───────────────┐
              │   ExoRoute    │  ← Single Rust binary (~15 MiB RAM)
              │  • Failover   │  ← Embedded LMDB + Web UI
              │  • Key Portal │  ← Live providers.yaml hot-reload
              └───────┬───────┘
                      │
     ┌────────────────┼────────────────┐
     ▼                ▼                ▼
 Cloud APIs    Hosted Gateways   OAuth / Local
(OpenAI, ...)  (OpenRouter, ...) (Ollama, Codex, ...)
Why ExoRoute?One endpoint, 3 API formats: Speak /v1/chat/completions, /v1/responses, or /v1/messages — ExoRoute translates protocols and streaming SSE frames automatically.Resilient failover & multi-key rotation: Group models into aliases (combos) and attach unlimited API keys per provider. Rate-limited or failing keys/upstreams are skipped automatically.Zero-downtime model updates: Edit ~/.exoroute/providers.yaml and changes apply immediately (or press r + Enter in the terminal). OpenCode Go/Zen models are auto-discovered upstream without manual config.Built for long coding sessions: Protocol-aware SSE keep-alive (event: ping for Anthropic), live token/cache accounting, fallback cost estimation, and optional stream continuity across client disconnects.Scoped Key Portal: Issue exo_... gateway keys so teammates or apps can sign in with their key and inspect only their own usage, cost, and request logs — strictly isolated from admin routes.
Cold-Start FootprintMeasured from process launch to TCP port ready on the same machine:MetricExoRoute9routerOmniRoutePort ready (cold start)1.208 s7.732 s (6.4× slower)6.221 s (5.1× slower)Working set (RAM)26.4 MiB164.8 MiB524.5 MiBPrivate bytes15.0 MiB166.0 MiB565.4 MiBPeak startup CPU0.134%7.517%11.150%
Quick Start1. Install & RunBashnpm install -g exoroute
exoroute
(Prebuilt binaries for Linux, macOS, and Windows are also available on GitHub Releases.)Open http://127.0.0.1:8686, sign in, add a provider, create a model combo, and generate a gateway key (exo_...).2. Connect Your ToolsClaude Code (Anthropic /v1/messages):Bashexport ANTHROPIC_BASE_URL="http://127.0.0.1:8686"
export ANTHROPIC_AUTH_TOKEN="exo_your_gateway_key"
claude
OpenAI SDK / curl (/v1/chat/completions):Bashcurl http://127.0.0.1:8686/v1/chat/completions \
  -H "Authorization: Bearer exo_your_gateway_key" \
  -H "Content-Type: application/json" \
  -d '{"model":"coding-model","messages":[{"role":"user","content":"Hello"}]}'
Supported Endpoints: POST /v1/chat/completions · POST /v1/responses · POST /v1/messages · GET /v1/modelsProvider Coverage (43 Built-in + Custom)CategoryCountHighlightsCloud APIs23OpenAI, Anthropic, Gemini, DeepSeek, xAI, Mistral, Groq, Cerebras, Perplexity, DashScope, Together, Fireworks, Moonshot, Zhipu, MiniMax, Azure OpenAI, Vertex AI, GitHub Models, and moreGateways & Local11OpenRouter, OpenCode Go/Zen, NVIDIA NIM, Kilo Gateway, Command Code, Freebuff, Ollama, LM Studio, vLLM, Devin CLIOAuth / Subscription9OpenAI Codex, Antigravity, Kilo Code, Qoder, Kiro, Cline, ClinePass, MuseCustom+∞Any OpenAI- or Anthropic-compatible HTTP endpointLive Model & Pricing Overlay (providers.yaml)ExoRoute generates a starter ~/.exoroute/providers.yaml on first boot (never overwritten). Use it to add models, hide defaults, or override pricing/limits without waiting for a release:YAMLproviders:
  opencode_go:
    default_models: [hy4-flash-free]
    remove_models: []
    model_overrides:
      hy4-flash-free:
        pricing: { input_per_million: 0.25, output_per_million: 1.5 }
        limits: { context_window: 200000, max_output_tokens: 32000 }
Invalid YAML is rejected safely while keeping the last known-good catalog active.ConfigurationEnvironment VariableDefaultDescriptionEXOROUTE_HOST127.0.0.1Bind addressEXOROUTE_PORT8686Listen portEXOROUTE_DATA_DIR~/.exorouteData directory (config & database)EXOROUTE_DATABASE_PATH<data_dir>/dbCustom LMDB database pathEXOROUTE_MASTER_KEY(auto-generated)Master encryption key for stored credentialsEXOROUTE_ADMIN_KEY(auto-generated)Admin authentication secretEXOROUTE_ALLOW_PRIVATE_PROVIDER_URLSfalseAllow routing to localhost/private-IP upstreamsEXOROUTE_TRUST_PROXY_HEADERSfalseTrust X-Forwarded-For from loopback reverse proxiesBuild from SourceThe Rust binary embeds the compiled dashboard (web/dist), so build the frontend first:Bashcd web && npm ci && npm run build && cd ..
cargo build --locked --release
Run verification checks:Bashcargo test --workspace --all-features
cd web && npm test && cd ..
node scripts/check_source_boundaries.mjs
Links & LicenseAPI Docs (local): http://127.0.0.1:8686/docsReleases: https://github.com/nghiaomg/ExoRoute/releasesLicense: MIT
