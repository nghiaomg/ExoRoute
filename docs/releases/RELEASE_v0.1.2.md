# ExoRoute v0.1.2

Release Date: 2026-11-04

ExoRoute v0.1.2 is a major release introducing official Cloud API provider subscriptions, an overhaul of the dashboard theme design system, live streaming cache token tracking, and 100% translation key parity across all 11 supported locales.

---

## Highlights

### 1. Official Cloud API Provider Integrations (`Cloud API`)
ExoRoute v0.1.2 adds first-class support for **12 official AI provider platforms and subscription APIs**, providing dedicated endpoint routing, credential validation, automated protocol normalization, and crisp vector branding:

1. **OpenAI Direct (`openai`)**
   - Connects directly to `https://api.openai.com/v1`.
   - Dual protocol support: **Chat Completions** (`/v1/chat/completions`) and **Responses** (`/v1/responses`).
   - Dynamic model discovery via `/v1/models`.
   - Default models: `gpt-4o`, `gpt-4o-mini`, `o1`, `o3-mini`, `gpt-4.5-preview`.

2. **Anthropic Direct (`anthropic`)**
   - Connects directly to `https://api.anthropic.com/v1`.
   - Native Anthropic Messages protocol (`/v1/messages`).
   - Automated injection of required `anthropic-version: 2023-06-01` header and `x-api-key` authentication.
   - Dynamic model discovery via Anthropic's `/v1/models` endpoint.
   - Default models: `claude-3-7-sonnet-20250219`, `claude-3-5-sonnet-20241022`, `claude-3-5-haiku-20241022`.

3. **Google Gemini (`gemini`)**
   - Connects to Google AI Studio's OpenAI-compatible endpoint at `https://generativelanguage.googleapis.com/v1beta/openai`.
   - Bearer authentication and Chat Completions protocol.
   - Default models: `gemini-2.5-pro`, `gemini-2.5-flash`, `gemini-2.0-flash`.

4. **DeepSeek Platform (`deepseek`)**
   - Direct connection to `https://api.deepseek.com/v1`.
   - Bearer authentication and Chat Completions protocol.
   - Default models: `deepseek-chat`, `deepseek-reasoner` (R1 reasoning models).

5. **xAI Grok (`xai`)**
   - Direct connection to `https://api.x.ai/v1`.
   - Bearer authentication and Chat Completions protocol.
   - Default models: `grok-2-1212`, `grok-2-vision-1212`, `grok-beta`.

6. **Mistral AI (`mistral`)**
   - Connects to `https://api.mistral.ai/v1`.
   - Bearer authentication and Chat Completions protocol.
   - Default models: `mistral-large-latest`, `codestral-latest`, `pixtral-large-latest`.

7. **Perplexity AI (`perplexity`)**
   - Connects to `https://api.perplexity.ai`.
   - Bearer authentication and Chat Completions protocol.
   - Built-in graceful fallback probe handling for Perplexity's non-standard model endpoint.
   - Default models: `sonar`, `sonar-pro`, `sonar-reasoning`.

8. **Groq Cloud (`groq`)**
   - High-speed LPU inference via `https://api.groq.com/openai/v1`.
   - Bearer authentication and Chat Completions protocol.
   - Default models: `llama-3.3-70b-versatile`, `deepseek-r1-distill-llama-70b`, `qwen-2.5-coder-32b`.

9. **Cerebras Cloud (`cerebras`)**
   - Wafer-scale CS-3 ultra-fast inference via `https://api.cerebras.ai/v1`.
   - Bearer authentication and Chat Completions protocol.
   - Default models: `llama3.3-70b`, `llama3.1-8b`.

10. **Alibaba DashScope / Qwen (`dashscope`)**
    - Connects to `https://dashscope-intl.aliyuncs.com/compatible-mode/v1`.
    - Bearer authentication and Chat Completions protocol.
    - Default models: `qwen-2.5-coder-32b-instruct`, `qwen-plus`, `qwen-max`.

11. **Together AI (`together`)**
    - Connects to `https://api.together.xyz/v1`.
    - Bearer authentication and Chat Completions protocol.
    - Default models: `meta-llama/Llama-3.3-70B-Instruct-Turbo`, `deepseek-ai/DeepSeek-R1`.

12. **Fireworks AI (`fireworks`)**
    - Production-grade low-latency inference via `https://api.fireworks.ai/inference/v1`.
    - Bearer authentication and Chat Completions protocol.
    - Default models: `accounts/fireworks/models/deepseek-r1`, `accounts/fireworks/models/llama-v3p3-70b-instruct`.

---

### 2. Comprehensive Theme Design System Overhaul
- **7 Distinct Visual Archetypes**:
  - `classic-light`: Clean, warm Soft Neo-Brutalism with 2px ink borders, hard offset shadows, and signature brand orange accents.
  - `classic-dark`: Sleek obsidian dark surfaces with warm orange glow and subtle elevation.
  - `tokyo-night`: Cyberpunk aesthetic inspired by Tokyo nightlife, featuring deep navy slate, vibrant violet, and neon cyan accents.
  - `catppuccin-mocha`: Soothing pastel-dark palette (Mocha) designed to minimize eye strain during extended coding sessions.
  - `linear`: High-precision, ultra-clean monochrome minimalist dark interface with subtle indigo/violet highlights.
  - `cyber-neon`: High-contrast cyberpunk space black with high-voltage neon yellow, electric green, and hot pink accents.
  - `minimal-light`: High-legibility editorial light theme designed for bright workspaces, documentation, and clean screenshots.
- **True Style Customization**: Themes now redefine border radiuses, shadows, surface elevations, and typographic hierarchies rather than just changing primary colors.
- **Instant Preview & Zero-Flicker Bootstrap**: Setting changes apply instantly with zero layout shift; the HTML bootstrap sets both `data-theme` and `data-palette` before first paint.

---

### 3. Protocol & Gateway Streaming Improvements
- **Live Cache Token Tracking**: Resolved an issue where prompt cache metrics were only visible after a full page reload; streaming SSE requests now decode and display cached tokens in real-time.
- **Anthropic Messages Versioning**: Guaranteed injection of `anthropic-version: 2023-06-01` on all outbound Messages protocol requests, model tests, and discovery probes.
- **Provider Egress & Header Safety**: Enforced strict case-insensitive header validation and egress protections across all new cloud adapters.

---

### 4. Multilingual (i18n) Key Parity & Polish
- Added full translations in Vietnamese (`vi.ts`), English (`en.ts`), and Chinese (`zh.ts`) for all newly integrated providers and theme settings.
- Enforced 100% key parity and placeholder integrity across all 11 supported locales (`en`, `vi`, `zh`, `es`, `fr`, `de`, `ja`, `ko`, `pt`, `id`, `ar`, `hi`), verified by automated test suites.

---

### 5. Cross-Platform Binaries & Build System
- **Linux Release**: Standalone statically-linked Linux x86_64 release binary at `target/release/exoroute`.
- **Windows Release**: Cross-compiled standalone Windows x86_64 PE executable at `target/x86_64-pc-windows-gnu/release/exoroute.exe`.
- **Embedded Frontend**: Both release executables embed the full Svelte 5 single-page application and all vector assets via `RustEmbed`.

---

## Verification & Test Results
- **Rust Unit & Integration Tests**: 655/655 passed (`cargo test --workspace`).
- **Web Frontend Tests**: 127/127 unit tests passed (`node --test tests/*.test.ts`).
- **Vitest UI Test Suites**: 16/16 test files passed, 57/57 tests passed (`vitest run`).
- **Source Layout & Boundary Budget**: Passed (`node scripts/check_source_boundaries.mjs`).
