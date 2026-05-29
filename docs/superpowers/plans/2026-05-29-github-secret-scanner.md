# GitHub Secret Scanner Bot Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust daemon that polls GitHub Code Search for files matching configurable keywords, uses OpenRouter AI to detect secrets, and sends Telegram alerts.

**Architecture:** A tokio async loop polls GitHub every 2 minutes per keyword, fetches raw file content, deduplicates via SQLite, sends content to OpenRouter for AI analysis, and fires Telegram alerts on findings.

**Tech Stack:** Rust, tokio, reqwest, serde_json, toml, rusqlite, sha2, tracing

---

## File Map

| File | Responsibility |
|------|---------------|
| `Cargo.toml` | Dependencies |
| `config.example.toml` | Config template (committed) |
| `config.toml` | Actual config with API keys (gitignored) |
| `.gitignore` | Ignore config.toml and scanner.db |
| `src/main.rs` | Entry point, main polling loop |
| `src/config.rs` | Load and parse config.toml |
| `src/cache.rs` | SQLite dedup cache |
| `src/poller.rs` | GitHub Code Search API + raw file fetch |
| `src/analyzer.rs` | OpenRouter AI analysis |
| `src/alerter.rs` | Telegram alert sending |

---

## Task 1: Project Scaffold

**Files:**
- Create: `Cargo.toml`
- Create: `.gitignore`
- Create: `config.example.toml`
- Create: `src/main.rs` (stub)

- [ ] **Step 1: Init Rust project**

```bash
cd /home/romy/my-project/mining-sol
cargo init --name github-secret-scanner
```

Expected: `src/main.rs` and `Cargo.toml` created.

- [ ] **Step 2: Replace Cargo.toml with full dependencies**

```toml
[package]
name = "github-secret-scanner"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "scanner"
path = "src/main.rs"

[dependencies]
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", features = ["json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
rusqlite = { version = "0.31", features = ["bundled"] }
sha2 = "0.10"
hex = "0.4"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

[dev-dependencies]
tokio-test = "0.4"
```

- [ ] **Step 3: Create .gitignore**

```
config.toml
scanner.db
target/
```

- [ ] **Step 4: Create config.example.toml**

```toml
[github]
token = ""          # optional — increases rate limit from 10 to 30 req/min
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

- [ ] **Step 5: Verify compile stub**

```bash
cargo build
```

Expected: compiles with `Hello, world!` in main.rs.

- [ ] **Step 6: Commit**

```bash
git init
git add Cargo.toml Cargo.lock .gitignore config.example.toml src/main.rs
git commit -m "feat: scaffold github-secret-scanner project"
```

---

## Task 2: Config Module

**Files:**
- Create: `src/config.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write failing test in src/config.rs**

```rust
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct Config {
    pub github: GithubConfig,
    pub openrouter: OpenRouterConfig,
    pub telegram: TelegramConfig,
}

#[derive(Deserialize, Debug)]
pub struct GithubConfig {
    pub token: Option<String>,
    pub keywords: Vec<String>,
    pub interval_secs: u64,
}

#[derive(Deserialize, Debug)]
pub struct OpenRouterConfig {
    pub api_key: String,
    pub model: String,
}

#[derive(Deserialize, Debug)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub chat_id: String,
}

pub fn load(path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&content)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_load_config() {
        let toml = r#"
[github]
token = "ghp_test"
keywords = ["solana private key"]
interval_secs = 60

[openrouter]
api_key = "sk-or-test"
model = "meta-llama/llama-3.1-8b-instruct:free"

[telegram]
bot_token = "123:ABC"
chat_id = "456"
"#;
        let path = "/tmp/test_config.toml";
        std::fs::write(path, toml).unwrap();
        let config = load(path).unwrap();
        assert_eq!(config.github.keywords[0], "solana private key");
        assert_eq!(config.github.interval_secs, 60);
        assert_eq!(config.openrouter.model, "meta-llama/llama-3.1-8b-instruct:free");
        assert_eq!(config.telegram.chat_id, "456");
    }

    #[test]
    fn test_load_config_token_optional() {
        let toml = r#"
[github]
keywords = ["test"]
interval_secs = 120

[openrouter]
api_key = "sk"
model = "model"

[telegram]
bot_token = "tok"
chat_id = "id"
"#;
        let path = "/tmp/test_config_no_token.toml";
        std::fs::write(path, toml).unwrap();
        let config = load(path).unwrap();
        assert!(config.github.token.is_none());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test config
```

Expected: compile error — module `config` not declared in `main.rs`.

- [ ] **Step 3: Add module declaration to src/main.rs**

```rust
mod config;

fn main() {
    println!("Hello, world!");
}
```

- [ ] **Step 4: Run tests and verify they pass**

```bash
cargo test config
```

Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs src/main.rs
git commit -m "feat: add config module with toml loading"
```

---

## Task 3: Cache Module

**Files:**
- Create: `src/cache.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write src/cache.rs with tests**

```rust
use rusqlite::{Connection, Result};
use sha2::{Digest, Sha256};

pub struct Cache {
    conn: Connection,
}

impl Cache {
    pub fn new(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS scanned_files (
                hash TEXT PRIMARY KEY,
                scanned_at INTEGER NOT NULL
            )",
        )?;
        Ok(Self { conn })
    }

    pub fn is_seen(&self, key: &str) -> bool {
        let hash = hash_key(key);
        self.conn
            .query_row(
                "SELECT 1 FROM scanned_files WHERE hash = ?1",
                [&hash],
                |_| Ok(()),
            )
            .is_ok()
    }

    pub fn mark_seen(&self, key: &str) -> Result<()> {
        let hash = hash_key(key);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        self.conn.execute(
            "INSERT OR IGNORE INTO scanned_files (hash, scanned_at) VALUES (?1, ?2)",
            rusqlite::params![hash, now],
        )?;
        Ok(())
    }
}

fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_seen_returns_false_for_new_key() {
        let cache = Cache::new(":memory:").unwrap();
        assert!(!cache.is_seen("owner/repo/path/abc123"));
    }

    #[test]
    fn test_mark_seen_then_is_seen_returns_true() {
        let cache = Cache::new(":memory:").unwrap();
        let key = "owner/repo/path/abc123";
        cache.mark_seen(key).unwrap();
        assert!(cache.is_seen(key));
    }

    #[test]
    fn test_mark_seen_is_idempotent() {
        let cache = Cache::new(":memory:").unwrap();
        let key = "owner/repo/path/abc123";
        cache.mark_seen(key).unwrap();
        cache.mark_seen(key).unwrap(); // second call must not error
        assert!(cache.is_seen(key));
    }

    #[test]
    fn test_different_keys_are_independent() {
        let cache = Cache::new(":memory:").unwrap();
        cache.mark_seen("key1").unwrap();
        assert!(cache.is_seen("key1"));
        assert!(!cache.is_seen("key2"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test cache
```

Expected: compile error — module not declared.

- [ ] **Step 3: Add module to src/main.rs**

```rust
mod config;
mod cache;

fn main() {
    println!("Hello, world!");
}
```

- [ ] **Step 4: Run tests and verify they pass**

```bash
cargo test cache
```

Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/cache.rs src/main.rs
git commit -m "feat: add SQLite dedup cache module"
```

---

## Task 4: Poller Module

**Files:**
- Create: `src/poller.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write src/poller.rs**

```rust
use reqwest::Client;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct SearchResponse {
    pub items: Vec<SearchItem>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SearchItem {
    pub name: String,
    pub path: String,
    pub sha: String,
    pub html_url: String,
    pub repository: Repository,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Repository {
    pub full_name: String,
}

pub struct Poller {
    client: Client,
    token: Option<String>,
}

impl Poller {
    pub fn new(token: Option<String>) -> Self {
        Self {
            client: Client::new(),
            token,
        }
    }

    pub async fn search(&self, keyword: &str) -> Result<Vec<SearchItem>, reqwest::Error> {
        let mut req = self
            .client
            .get("https://api.github.com/search/code")
            .query(&[
                ("q", keyword),
                ("sort", "indexed"),
                ("order", "desc"),
                ("per_page", "10"),
            ])
            .header("Accept", "application/vnd.github.v3+json")
            .header("User-Agent", "github-secret-scanner/1.0");

        if let Some(token) = &self.token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        let resp = req.send().await?;
        let search_resp: SearchResponse = resp.json().await?;
        Ok(search_resp.items)
    }

    pub async fn fetch_content(&self, item: &SearchItem) -> Result<String, reqwest::Error> {
        let raw_url = item
            .html_url
            .replace("github.com", "raw.githubusercontent.com")
            .replace("/blob/", "/");

        let mut req = self
            .client
            .get(&raw_url)
            .header("User-Agent", "github-secret-scanner/1.0");

        if let Some(token) = &self.token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        req.send().await?.text().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raw_url_conversion() {
        let item = SearchItem {
            name: "file.rs".to_string(),
            path: "src/file.rs".to_string(),
            sha: "abc".to_string(),
            html_url: "https://github.com/owner/repo/blob/main/src/file.rs".to_string(),
            repository: Repository {
                full_name: "owner/repo".to_string(),
            },
        };
        let raw_url = item
            .html_url
            .replace("github.com", "raw.githubusercontent.com")
            .replace("/blob/", "/");
        assert_eq!(
            raw_url,
            "https://raw.githubusercontent.com/owner/repo/main/src/file.rs"
        );
    }
}
```

- [ ] **Step 2: Add module to src/main.rs**

```rust
mod config;
mod cache;
mod poller;

fn main() {
    println!("Hello, world!");
}
```

- [ ] **Step 3: Run test**

```bash
cargo test poller
```

Expected: 1 test passes.

- [ ] **Step 4: Commit**

```bash
git add src/poller.rs src/main.rs
git commit -m "feat: add GitHub search poller module"
```

---

## Task 5: Analyzer Module

**Files:**
- Create: `src/analyzer.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write src/analyzer.rs**

```rust
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct OpenRouterRequest {
    model: String,
    messages: Vec<Message>,
}

#[derive(Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct OpenRouterResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: MessageContent,
}

#[derive(Deserialize)]
struct MessageContent {
    content: String,
}

#[derive(Deserialize, Debug)]
pub struct AnalysisResult {
    pub found: bool,
    pub secrets: Vec<String>,
}

pub struct Analyzer {
    client: Client,
    api_key: String,
    model: String,
}

impl Analyzer {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
        }
    }

    pub async fn analyze(&self, content: &str) -> Result<AnalysisResult, Box<dyn std::error::Error + Send + Sync>> {
        let truncated = &content[..content.len().min(2000)];

        let prompt = format!(
            r#"Analyze this code snippet. Does it contain any secrets such as:
- Private keys (Solana, Ethereum, or other crypto)
- Seed phrases or mnemonics
- API keys (AWS, GitHub tokens, etc.)
- .env variable assignments with sensitive values

Code:
```
{}
```

Respond ONLY in JSON format with no extra text: {{"found": true, "secrets": ["description"]}} or {{"found": false, "secrets": []}}"#,
            truncated
        );

        let request = OpenRouterRequest {
            model: self.model.clone(),
            messages: vec![Message {
                role: "user".to_string(),
                content: prompt,
            }],
        };

        let resp = self
            .client
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        let or_resp: OpenRouterResponse = resp.json().await?;
        let raw = &or_resp.choices[0].message.content;
        let json_str = extract_json(raw);
        let result: AnalysisResult = serde_json::from_str(json_str)?;
        Ok(result)
    }
}

fn extract_json(text: &str) -> &str {
    if let (Some(start), Some(end)) = (text.find('{'), text.rfind('}')) {
        return &text[start..=end];
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_clean() {
        let text = r#"{"found": true, "secrets": ["API key"]}"#;
        assert_eq!(extract_json(text), text);
    }

    #[test]
    fn test_extract_json_with_markdown_wrapper() {
        let text = "```json\n{\"found\": false, \"secrets\": []}\n```";
        let extracted = extract_json(text);
        let result: AnalysisResult = serde_json::from_str(extracted).unwrap();
        assert!(!result.found);
    }

    #[test]
    fn test_extract_json_with_preamble() {
        let text = r#"Sure! Here is the result: {"found": true, "secrets": ["private key found"]}"#;
        let extracted = extract_json(text);
        let result: AnalysisResult = serde_json::from_str(extracted).unwrap();
        assert!(result.found);
        assert_eq!(result.secrets[0], "private key found");
    }
}
```

- [ ] **Step 2: Add module to src/main.rs**

```rust
mod config;
mod cache;
mod poller;
mod analyzer;

fn main() {
    println!("Hello, world!");
}
```

- [ ] **Step 3: Run tests**

```bash
cargo test analyzer
```

Expected: 3 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/analyzer.rs src/main.rs
git commit -m "feat: add OpenRouter AI analyzer module"
```

---

## Task 6: Alerter Module

**Files:**
- Create: `src/alerter.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write src/alerter.rs**

```rust
use reqwest::Client;
use serde::Serialize;

#[derive(Serialize)]
struct TelegramMessage {
    chat_id: String,
    text: String,
    parse_mode: String,
    disable_web_page_preview: bool,
}

pub struct Alerter {
    client: Client,
    bot_token: String,
    chat_id: String,
}

impl Alerter {
    pub fn new(bot_token: String, chat_id: String) -> Self {
        Self {
            client: Client::new(),
            bot_token,
            chat_id,
        }
    }

    pub async fn send(
        &self,
        repo: &str,
        path: &str,
        secrets: &[String],
        snippet: &str,
        link: &str,
    ) -> Result<(), reqwest::Error> {
        let secrets_list = secrets
            .iter()
            .map(|s| format!("• {}", s))
            .collect::<Vec<_>>()
            .join("\n");

        let snippet_preview = &snippet[..snippet.len().min(300)];
        let text = format!(
            "🚨 *Secret Detected*\n\nRepo: `{}`\nFile: `{}`\n\nSecrets Found:\n{}\n\nSnippet:\n```\n{}\n```\n\n[View File]({})",
            repo, path, secrets_list, snippet_preview, link
        );

        let message = TelegramMessage {
            chat_id: self.chat_id.clone(),
            text,
            parse_mode: "Markdown".to_string(),
            disable_web_page_preview: true,
        };

        let url = format!(
            "https://api.telegram.org/bot{}/sendMessage",
            self.bot_token
        );
        self.client.post(&url).json(&message).send().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_format_contains_repo() {
        let secrets = vec!["Solana private key".to_string()];
        let snippet = "let key = [1,2,3];";
        let repo = "owner/repo";
        let path = "src/main.rs";
        let link = "https://github.com/owner/repo/blob/main/src/main.rs";

        let secrets_list = secrets
            .iter()
            .map(|s| format!("• {}", s))
            .collect::<Vec<_>>()
            .join("\n");

        let text = format!(
            "🚨 *Secret Detected*\n\nRepo: `{}`\nFile: `{}`\n\nSecrets Found:\n{}\n\nSnippet:\n```\n{}\n```\n\n[View File]({})",
            repo, path, secrets_list, snippet, link
        );

        assert!(text.contains("owner/repo"));
        assert!(text.contains("Solana private key"));
        assert!(text.contains("https://github.com"));
    }
}
```

- [ ] **Step 2: Add module to src/main.rs**

```rust
mod config;
mod cache;
mod poller;
mod analyzer;
mod alerter;

fn main() {
    println!("Hello, world!");
}
```

- [ ] **Step 3: Run test**

```bash
cargo test alerter
```

Expected: 1 test passes.

- [ ] **Step 4: Commit**

```bash
git add src/alerter.rs src/main.rs
git commit -m "feat: add Telegram alerter module"
```

---

## Task 7: Main Loop

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Replace src/main.rs with full implementation**

```rust
mod alerter;
mod analyzer;
mod cache;
mod config;
mod poller;

use std::time::Duration;
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let cfg = config::load("config.toml")?;
    let cache = cache::Cache::new("scanner.db")?;
    let poller = poller::Poller::new(cfg.github.token.clone());
    let analyzer = analyzer::Analyzer::new(
        cfg.openrouter.api_key.clone(),
        cfg.openrouter.model.clone(),
    );
    let alerter = alerter::Alerter::new(
        cfg.telegram.bot_token.clone(),
        cfg.telegram.chat_id.clone(),
    );

    info!(
        "Scanner started. {} keywords, polling every {}s",
        cfg.github.keywords.len(),
        cfg.github.interval_secs
    );

    loop {
        for keyword in &cfg.github.keywords {
            info!("Searching keyword: {}", keyword);

            let items = match poller.search(keyword).await {
                Ok(items) => items,
                Err(e) => {
                    warn!("GitHub search error for '{}': {}", keyword, e);
                    continue;
                }
            };

            info!("Found {} results for '{}'", items.len(), keyword);

            for item in items {
                let cache_key = format!(
                    "{}/{}/{}",
                    item.repository.full_name, item.path, item.sha
                );

                if cache.is_seen(&cache_key) {
                    continue;
                }

                let content = match poller.fetch_content(&item).await {
                    Ok(c) => c,
                    Err(e) => {
                        warn!("Fetch error for {}: {}", item.html_url, e);
                        continue;
                    }
                };

                match analyzer.analyze(&content).await {
                    Err(e) => {
                        error!(
                            "Analyzer error for {}/{}: {}",
                            item.repository.full_name, item.path, e
                        );
                        // Do NOT mark as seen — retry next cycle
                        continue;
                    }
                    Ok(result) if result.found => {
                        info!(
                            "Secret found in {}/{}",
                            item.repository.full_name, item.path
                        );
                        if let Err(e) = alerter
                            .send(
                                &item.repository.full_name,
                                &item.path,
                                &result.secrets,
                                &content,
                                &item.html_url,
                            )
                            .await
                        {
                            error!("Telegram alert failed: {}", e);
                        }
                        cache.mark_seen(&cache_key)?;
                    }
                    Ok(_) => {
                        info!("Clean: {}/{}", item.repository.full_name, item.path);
                        cache.mark_seen(&cache_key)?;
                    }
                }
            }
        }

        info!(
            "Cycle complete. Sleeping {}s",
            cfg.github.interval_secs
        );
        tokio::time::sleep(Duration::from_secs(cfg.github.interval_secs)).await;
    }
}
```

- [ ] **Step 2: Build and verify compilation**

```bash
cargo build --release
```

Expected: compiles cleanly with no errors. Binary at `target/release/scanner`.

- [ ] **Step 3: Run all tests**

```bash
cargo test
```

Expected: all tests pass (config ×2, cache ×4, poller ×1, analyzer ×3, alerter ×1 = 11 tests).

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat: implement main polling loop"
```

---

## Task 8: First Run

**Files:**
- Create: `config.toml` (from example, fill in API keys)

- [ ] **Step 1: Copy config example**

```bash
cp config.example.toml config.toml
```

- [ ] **Step 2: Fill in config.toml**

Open `config.toml` and set:
- `github.token` — create at https://github.com/settings/tokens (no scope needed for public search)
- `openrouter.api_key` — from https://openrouter.ai/keys
- `telegram.bot_token` — create via @BotFather on Telegram
- `telegram.chat_id` — get by messaging @userinfobot on Telegram

- [ ] **Step 3: Run the scanner**

```bash
./target/release/scanner
```

Expected output:
```
INFO scanner: Scanner started. 5 keywords, polling every 120s
INFO scanner: Searching keyword: solana private key
INFO scanner: Found N results for 'solana private key'
...
```

- [ ] **Step 4: Verify Telegram alert arrives**

If any secret is detected in the first cycle, a Telegram message should appear in your chat within 30 seconds.

- [ ] **Step 5: Final commit**

```bash
git add config.example.toml
git commit -m "chore: finalize project setup and config template"
```

---

## Running in Production (VPS)

```bash
# Build release binary
cargo build --release

# Run as background process with logging
nohup ./target/release/scanner > scanner.log 2>&1 &

# Or use systemd service
# /etc/systemd/system/secret-scanner.service
```

```ini
[Unit]
Description=GitHub Secret Scanner
After=network.target

[Service]
WorkingDirectory=/home/romy/my-project/mining-sol
ExecStart=/home/romy/my-project/mining-sol/target/release/scanner
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```
