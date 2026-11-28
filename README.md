# ExoRoute

**Local-first AI gateway · single binary · 43 providers · 3 API formats**

`Rust` · `127.0.0.1:8686` · `~/.exoroute` · `MIT`

ExoRoute lets one client talk to many AI providers through one stable API.

---

## ⚡ Port Ready (Cold Start)

**Process start → TCP ready: 1.208s — fastest in the comparison.**

| Metric | **ExoRoute** | 9router | OmniRoute |
|---|---:|---:|---:|
| Ready time | **1.208s** | 7.732s (6.4x slower) | 6.221s (5.1x slower) |
| Working set | **26.4 MiB** | 164.8 MiB | 524.5 MiB |
| Private bytes | **15.0 MiB** | 166.0 MiB | 565.4 MiB |
| Peak startup CPU | **0.134%** | 7.517% | 11.150% |

*Reported cold-start figures; publish harness, machine, and workload for reproducibility.*

---

## Provider coverage

| Type | Count | Examples |
|---|---:|---|
| Cloud APIs | **23** | OpenAI · Anthropic · Gemini · DeepSeek · xAI · Mistral · Perplexity · Groq · Cerebras · DashScope · Together · Fireworks · Moonshot · Zhipu · MiniMax · Azure OpenAI · Vertex AI · GitHub Models · and more |
| Hosted gateways | **11** | OpenRouter · OpenCode Go/Zen · NVIDIA NIM · Kilo Gateway · Command Code · Freebuff · Ollama · LM Studio · vLLM · Devin CLI |
| Subscription / OAuth | **9** | OpenAI Codex · Antigravity · Kilo Code · Qoder · Kiro · Cline · ClinePass · Muse |
| Custom | **+1** | Any OpenAI-compatible endpoint |

**43 built-in presets + custom provider support.**

---

## Why ExoRoute

| For you | In practice |
|---|---|
| One API, every model | Use your existing OpenAI or Anthropic SDK; switch providers without rewriting client code. |
| Fewer interruptions | Friendly aliases try the next model automatically when a provider is rate-limited or fails. |
| New models, no release wait | Edit `~/.exoroute/providers.yaml`; changes apply immediately, or press `[r]` + Enter in the terminal. OpenCode Go/Zen discovers new upstream models on its own. |
| Keys that keep working | Add unlimited keys per provider; failed or exhausted keys are skipped automatically. |
| Costs stay visible | Missing upstream cost is estimated from your own prices in `providers.yaml`. |
| Streams survive disconnects | Live token tracking; optional continuity resumes a stream after a client drops. |
| A usable dashboard | Manage providers, keys, routes, proxies, usage, request history, and themes in one place. |
| Private stats per key | A gateway key holder (`exo_...`) signs in separately and sees only that key's data — never admin routes. |

---

## Quick start

```sh
npm install -g exoroute
exoroute
```

Open <http://127.0.0.1:8686>, sign in, add a provider, create a combo, then generate a gateway key.

```sh
curl http://127.0.0.1:8686/v1/chat/completions \
  -H "Authorization: Bearer EXO_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"coding-model","messages":[{"role":"user","content":"Hello"}]}'
```

Endpoints: `/v1/chat/completions` · `/v1/responses` · `/v1/messages` · `/v1/models`

---

## Model overlay

```yaml
providers:
  opencode_go:
    default_models: [hy4-flash-free]
    remove_models: []
    model_overrides:
      hy4-flash-free:
        pricing: { input_per_million: 0.25, output_per_million: 1.5 }
        limits: { context_window: 200000, max_output_tokens: 32000 }
```

Invalid YAML is rejected; the last valid catalog stays active. The starter file is created once and never overwritten.

Common overrides: `EXOROUTE_HOST`, `EXOROUTE_PORT`, `EXOROUTE_DATA_DIR`, `EXOROUTE_DATABASE_PATH`, `EXOROUTE_MASTER_KEY`, `EXOROUTE_ADMIN_KEY`, `EXOROUTE_ALLOW_PRIVATE_PROVIDER_URLS`, `EXOROUTE_TRUST_PROXY_HEADERS`.

---

## Build & check

```sh
# dashboard first — the binary embeds web/dist
cd web && npm ci && npm run build && cd ..

cargo build --locked --release
cargo test --workspace --all-features
node scripts/check_source_boundaries.mjs
```

CI runs checks on `develop`/`main` and builds Linux, Windows, and macOS binaries. Tags `v*` create signed releases.

**Docs:** <http://127.0.0.1:8686/docs> · **Releases:** <https://github.com/nghiaomg/ExoRoute/releases> · **License:** [MIT](LICENSE)
