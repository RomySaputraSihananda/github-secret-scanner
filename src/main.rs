mod alchemy;
mod alerter;
mod analyzer;
mod cache;
mod config;
mod poller;
mod validator;

use std::time::Duration;
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let cfg = config::load("config.toml")?;
    let cache = cache::Cache::new("scanner.db")?;
    let poller = poller::Poller::new(cfg.github.token.clone());
    let validator = validator::Validator::new(
        cfg.openrouter.api_key.clone(),
        cfg.openrouter.model.clone(),
    );
    let alchemy = alchemy::AlchemyValidator::new(cfg.alchemy.api_key.clone());
    let alerter = alerter::Alerter::new(
        cfg.telegram.bot_token.clone(),
        cfg.telegram.chat_id.clone(),
        cfg.telegram.message_thread_id,
    );

    info!(
        "Scanner started. {} keywords, polling every {}s",
        cfg.github.keywords.len(),
        cfg.github.interval_secs
    );

    let startup_msg = format!(
        "✅ *Scanner aktif*\n\nKeywords: {}\nInterval: {}s\nMode: regex",
        cfg.github.keywords.len(),
        cfg.github.interval_secs,
    );
    if let Err(e) = alerter.notify(&startup_msg).await {
        error!("Failed to send startup notification: {}", e);
    }

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
                // --- scan file content ---
                let file_key = format!(
                    "file/{}/{}/{}",
                    item.repository.full_name, item.path, item.sha
                );
                if !cache.is_seen(&file_key) {
                    let content = match poller.fetch_content(&item).await {
                        Ok(c) => c,
                        Err(e) => {
                            warn!("Fetch error for {}: {}", item.html_url, e);
                            String::new()
                        }
                    };
                    if !content.is_empty() {
                        let result = analyzer::analyze(&content);
                        if result.found {
                            match validator.validate(&content, &result.secrets).await {
                                Ok(v) if v.is_real => {
                                    info!("OpenRouter confirmed {}/{}: {}", item.repository.full_name, item.path, v.reason);
                                    send_with_alchemy(&alchemy, &alerter, &item.repository.full_name, &item.path, &result.secrets, &content, &item.html_url).await;
                                }
                                Ok(v) => {
                                    info!("False positive in {}/{}: {}", item.repository.full_name, item.path, v.reason);
                                }
                                Err(e) => {
                                    warn!("Validator error for {}/{}: {}", item.repository.full_name, item.path, e);
                                }
                            }
                        }
                        cache.mark_seen(&file_key)?;
                    }
                }

                // --- scan recent commits of this repo ---
                let repo = &item.repository.full_name;
                let commits = match poller.fetch_recent_commits(repo).await {
                    Ok(c) => c,
                    Err(e) => {
                        warn!("Commits fetch error for {}: {}", repo, e);
                        continue;
                    }
                };

                for commit in commits {
                    let commit_key = format!("commit/{}/{}", repo, commit.sha);
                    if cache.is_seen(&commit_key) {
                        continue;
                    }

                    let detail = match poller.fetch_commit_detail(repo, &commit.sha).await {
                        Ok(d) => d,
                        Err(e) => {
                            warn!("Commit detail error {}: {}", commit.sha, e);
                            continue;
                        }
                    };

                    let files = detail.files.unwrap_or_default();
                    for file in &files {
                        let patch = match &file.patch {
                            Some(p) => p,
                            None => continue,
                        };
                        // hanya scan baris yang ditambahkan
                        let added: String = patch
                            .lines()
                            .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
                            .map(|l| &l[1..])
                            .collect::<Vec<_>>()
                            .join("\n");

                        let result = analyzer::analyze(&added);
                        if result.found {
                            match validator.validate(&added, &result.secrets).await {
                                Ok(v) if v.is_real => {
                                    info!("OpenRouter confirmed commit {}/{} ({}): {}", repo, file.filename, &commit.sha[..8], v.reason);
                                    send_with_alchemy(&alchemy, &alerter, repo, &file.filename, &result.secrets, &added, &commit.html_url).await;
                                }
                                Ok(v) => {
                                    info!("False positive in commit {}/{}: {}", repo, file.filename, v.reason);
                                }
                                Err(e) => {
                                    warn!("Validator error for commit {}: {}", commit.sha, e);
                                }
                            }
                        }
                    }
                    cache.mark_seen(&commit_key)?;
                }
            }
        }

        info!("Cycle complete. Sleeping {}s", cfg.github.interval_secs);
        tokio::time::sleep(Duration::from_secs(cfg.github.interval_secs)).await;
    }
}

async fn send_with_alchemy(
    alchemy: &alchemy::AlchemyValidator,
    alerter: &alerter::Alerter,
    repo: &str,
    path: &str,
    secrets: &[String],
    content: &str,
    link: &str,
) {
    // Alchemy hanya enrichment — selalu alert, tambah info wallet kalau ada
    let mut enriched = secrets.to_vec();
    match alchemy.validate(content).await {
        Some(chain) if chain.is_active => {
            info!("Alchemy: wallet {} aktif ({:.4} SOL)", chain.address, chain.balance_sol);
            enriched.push(format!("🔑 Wallet: `{}` — *{:.4} SOL* (mainnet aktif)", chain.address, chain.balance_sol));
        }
        Some(chain) => {
            info!("Alchemy: wallet {} tidak aktif di mainnet (mungkin devnet)", chain.address);
            enriched.push(format!("🔑 Wallet: `{}` — 0 SOL (devnet/inactive)", chain.address));
        }
        None => {
            info!("Alchemy: tidak ada Solana wallet di snippet");
        }
    }
    if let Err(e) = alerter.send(repo, path, &enriched, content, link).await {
        error!("Telegram alert failed: {}", e);
    }
}
