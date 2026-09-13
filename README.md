<p align="center">
  <img src="assets/logo.svg" alt="ExoRoute — Flying Fish Logo" width="240" />
</p>

<h1 align="center">ExoRoute</h1>

<p align="center">
  <strong>The lightest and fastest AI protocol gateway, engineered in Rust.</strong><br>
  Built for high-throughput workloads with automatic resource reclamation.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/language-Rust%202021-orange.svg" alt="Language" />
  <img src="https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg" alt="License" />
  <img src="https://img.shields.io/badge/platform-Linux%20%7C%20macOS%20%7C%20Windows-lightgrey.svg" alt="Platform" />
  <img src="https://img.shields.io/badge/memory-Zero%20GC%20%7C%20Auto%20Reclaimed-green.svg" alt="Resource Management" />
</p>

<p align="center">
  <img src="assets/cli-demo.webp" alt="ExoRoute CLI startup screen" width="850" />
</p>

---

## Overview

**ExoRoute** is an ultra-lightweight, single-binary AI gateway and model router designed from the ground up in Rust. Engineered specifically to tackle massive request volumes with near-zero latency, ExoRoute eliminates runtime Garbage Collection (GC) pauses and enforces deterministic, immediate resource reclamation.

It provides your applications with a single, highly available loopback endpoint (`127.0.0.1:8686/v1`), seamlessly converting across **OpenAI Chat Completions**, **OpenAI Responses**, and **Anthropic Messages** while routing requests intelligently across providers and models.

---

## User Benefits

Why choose ExoRoute over heavy, multi-service alternatives?

* ⚡ **Blazing-Fast & Sub-Millisecond Overhead**: Native Rust compilation ensures that requests pass through the gateway in fractions of a millisecond (as low as **0.42 ms** added p50 overhead). Your models respond at wire speed without gateway lag.
* 🔄 **Automatic Resource Reclamation**: Say goodbye to memory leaks, ballooning heap allocations, and scheduled daemon reboots. Every buffer, connection permit, and stream chunk is bound and automatically reclaimed the exact microsecond its request completes.
* 🛡️ **Zero-Downtime High Availability (Combo Routing)**: Chain multiple upstream providers into prioritized or round-robin failover combos. If an upstream encounters outages, timeouts, or rate limits, ExoRoute instantly falls through to healthy backup targets.
* 🔑 **Self-Healing API Key Rotation**: Automatically rotate keys across requests. If a provider key hits HTTP 401, 403, or 429, ExoRoute immediately retries alternate keys in-flight without returning errors to your client.
* 🌐 **Universal Protocol Translation**: Build your client against one preferred SDK (OpenAI Chat, OpenAI Responses, or Anthropic Claude) and talk to any backend model without rewriting your application code.
* 📦 **Zero Infrastructure Hassle (Single Native Binary)**: No Docker containers, Python runtimes, or external databases (Redis/PostgreSQL) required. ExoRoute bundles an embedded SQLite database and a modern, high-speed Svelte 5 dashboard directly into a self-contained ~14 MiB executable.
* 🔒 **Local-First Security & Egress Guard**: Runs strictly on loopback (`127.0.0.1`), enforces gateway API token authentication, encrypts stored provider secrets at rest with ChaCha20-Poly1305, and validates egress against SSRF.

---

## Performance Snapshot

*Gateway overhead benchmarked on Windows 11, Intel Core i7-1355U, measuring local HTTP path without mock model delay (median of 4 runs x 600 requests):*

| Concurrency | Direct Mock p50 | Via ExoRoute p50 | Added Gateway Overhead | Throughput via ExoRoute |
| :---: | :---: | :---: | :---: | :---: |
| **1 client** | 0.16 ms | 0.58 ms | **+0.42 ms** | 1,334 req/s |
| **8 clients** | 1.73 ms | 3.28 ms | **+1.55 ms** | 2,317 req/s |
| **32 clients** | 3.51 ms | 12.35 ms | **+8.84 ms** | 2,516 req/s |

> ExoRoute processes persistent and concurrent streams with strictly bounded queues (max 64 in-flight concurrent requests per node and 32 per provider), guaranteeing consistent memory limits under heavy traffic spikes.

---

## Quick Install

### Linux & macOS
```sh
curl -fsSL https://raw.githubusercontent.com/nghiaomg/ExoRoute/main/install.sh | sh
```

### Windows (PowerShell)
```powershell
irm https://raw.githubusercontent.com/nghiaomg/ExoRoute/main/install.ps1 | iex
```

*The installer verifies signed checksum manifests with Sigstore Cosign before placing the binary in your PATH.*

---

## Getting Started

### 1. Launch the Gateway
```sh
exoroute
```
ExoRoute initializes your local directory (`~/.exoroute/`), creates a starter `.env`, prints the temporary admin password, and serves the dashboard immediately at `http://127.0.0.1:8686`.

### 2. Configure Providers & Routes
1. Open `http://127.0.0.1:8686` in your browser and log in.
2. Under **Providers**, add your OpenAI, Anthropic, or OpenAI-compatible credentials (or link OpenAI Codex via browser OAuth).
3. Under **Routes**, create an alias (e.g. `coding-model`) pointing to one or more provider models in order of priority.
4. Under **API Keys**, generate a client API key.

### 3. Send Requests
Use standard OpenAI SDKs or `curl`:

```sh
curl http://127.0.0.1:8686/v1/chat/completions \
  -H "Authorization: Bearer YOUR_EXOROUTE_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "coding-model",
    "messages": [{"role": "user", "content": "Explain Rust ownership in one sentence."}]
  }'
```

Supported endpoints include:
* `/v1/chat/completions` (OpenAI Chat format)
* `/v1/responses` (OpenAI Responses format)
* `/v1/messages` (Anthropic Claude format)
* `/v1/models` (List configured route aliases and models)

---

## Architecture Highlights

```
                      ┌─────────────────────────────────────────┐
                      │              Client (SDK / App)         │
                      └────────────────────┬────────────────────┘
                                           │ HTTP / SSE
                                           ▼
                      ┌─────────────────────────────────────────┐
                      │          ExoRoute Core Gateway          │
                      │  - Loopback listener (127.0.0.1:8686)   │
                      │  - Strict Bounded Memory Queues         │
                      │  - Automatic RAII Resource Reclamation  │
                      │  - Universal Protocol Normalizer        │
                      └──────┬───────────────────┬──────────────┘
                             │                   │
                [Route & Key Selection]  [Health Check & Failover]
                             │                   │
             ┌───────────────┴──────────┐        │
             ▼                          ▼        ▼
┌─────────────────────────┐   ┌─────────────────────────┐
│ Upstream Provider 1     │   │ Upstream Provider 2     │
│ (e.g. OpenAI / Claude)  │   │ (Fallback / Alternate)  │
└─────────────────────────┘   └─────────────────────────┘
```

* **Zero-Allocation Hot Paths**: String allocations and JSON clones are strictly minimized throughout request translation pipelines.
* **Deterministic Resource Teardown**: Async tasks drop acquired permits, socket buffers, and channels as soon as the client disconnects or an upstream stream terminates.
* **Integrated Observability**: Inspect real-time request latencies, HTTP status telemetry, and active providers straight from the built-in Command Center dashboard.

---

## Building from Source

Prerequisites: Rust toolchain (1.80+) and [Bun](https://bun.sh) (or Node.js).

```sh
# 1. Build dashboard assets
cd web
bun install
bun run build
cd ..

# 2. Build and run native release executable
cargo run --release
```

To run test suites:
```sh
cargo test
```

---

## License

Dual-licensed under either of [MIT License](LICENSE-MIT) or [Apache License, Version 2.0](LICENSE-APACHE) at your option.
