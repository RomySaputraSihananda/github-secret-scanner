# GitHub Secret Scanner Bot — Design Spec

**Date:** 2026-05-29  
**Stack:** Rust  
**Purpose:** Monitor public GitHub repos by keyword, detect accidentally pushed secrets, alert via Telegram.

---

## Overview

A lightweight Rust daemon that polls GitHub Code Search API every 2 minutes for files matching configurable keywords. Each new file is sent to an AI model (OpenRouter) for secret detection. If secrets are found, an alert is sent to Telegram with repo name, file path, code snippet, and direct link.

---

## Architecture

```
config.toml
    │
    ▼
┌─────────────────────────────────────────────┐
│               Scanner Bot                   │
│                                             │
│  Poller ──▶ Dedup Cache (SQLite)            │
│     │            │ (skip if seen)           │
│     ▼            ▼                          │
│  Fetch raw file content                     │
│     │                                       │
│     ▼                                       │
│  AI Analyzer (OpenRouter)                   │
│     │                                       │
│     ▼ (if secret found)                     │
│  Telegram Alerter                           │
└─────────────────────────────────────────────┘
```

---

## Components

### 1. Poller
- Hits GitHub Code Search API: `GET /search/code?q=<keyword>&sort=indexed&order=desc`
- Runs every `interval_secs` (default: 120)
- Iterates through all keywords in config
- Fetches raw file content via `raw.githubusercontent.com`
- Passes file content + metadata to Dedup Cache check

### 2. Dedup Cache (SQLite)
- Table: `scanned_files(hash TEXT PRIMARY KEY, scanned_at INTEGER)`
- Hash = SHA-256 of `{repo_full_name}/{file_path}/{file_sha}`
- If hash exists → skip
- If not → proceed to AI Analyzer, then insert hash

### 3. AI Analyzer
- Sends file content (truncated to 2000 chars) to OpenRouter
- Model: `meta-llama/llama-3.1-8b-instruct:free` (configurable)
- Prompt:
  ```
  Analyze this code snippet. Does it contain any secrets such as:
  - Private keys (Solana, Ethereum, or other crypto)
  - Seed phrases or mnemonics
  - API keys (AWS, GitHub tokens, etc.)
  - .env variable assignments with sensitive values

  Respond in JSON: {"found": true/false, "secrets": ["description1", ...]}
  ```
- Parses JSON response
- If `found: true` → forward to Telegram Alerter

### 4. Telegram Alerter
- Sends message via Bot API: `POST /bot{token}/sendMessage`
- Message format:
  ```
  🚨 Secret Detected

  Repo: owner/repo-name
  File: path/to/file.rs
  Secrets: [list from AI]

  Snippet:
  <first 300 chars of file>

  Link: https://github.com/owner/repo/blob/main/path/to/file.rs
  ```

### 5. Config (`config.toml`)
```toml
[github]
token = ""          # optional but increases rate limit to 30 req/min
keywords = [
  "solana private key",
  "PRIVATE_KEY=",
  "phantom wallet seed",
  "secret_key solana",
  "id_keypair",
]
interval_secs = 120

[openrouter]
api_key = ""
model = "meta-llama/llama-3.1-8b-instruct:free"

[telegram]
bot_token = ""
chat_id = ""
```

---

## Rust Crates

| Crate | Version | Purpose |
|-------|---------|---------|
| `tokio` | 1.x | Async runtime |
| `reqwest` | 0.12 | HTTP client (GitHub, OpenRouter, Telegram) |
| `serde` + `serde_json` | 1.x | JSON parsing |
| `toml` | 0.8 | Config file parsing |
| `rusqlite` | 0.31 | SQLite dedup cache |
| `sha2` | 0.10 | SHA-256 hashing |
| `tracing` + `tracing-subscriber` | 0.1 | Structured logging |
| `tokio-interval` (via tokio) | — | Polling loop |

---

## File Structure

```
mining-sol/
├── Cargo.toml
├── config.toml          # user config (gitignored)
├── config.example.toml  # template committed to repo
├── src/
│   ├── main.rs          # entry point, init, main loop
│   ├── config.rs        # config loading
│   ├── poller.rs        # GitHub Search API polling
│   ├── cache.rs         # SQLite dedup cache
│   ├── analyzer.rs      # OpenRouter AI analysis
│   └── alerter.rs       # Telegram alert sending
└── scanner.db           # SQLite DB (gitignored)
```

---

## Error Handling

- GitHub rate limit (403/429) → log warning, skip iteration, wait for next interval
- OpenRouter error → log error, skip file (do NOT mark as scanned so it retries)
- Telegram error → log error, continue (alert failure should not stop scanning)
- Network timeout → retry once, then skip

---

## Constraints & Limits

- GitHub Code Search: 30 req/min authenticated, 10 unauthenticated
- OpenRouter free tier: ~20 req/min (sufficient for 2-minute polling interval)
- File content truncated to 2000 chars before sending to AI (cost/token control)
- Each polling cycle processes max 10 results per keyword (GitHub API default page size)
