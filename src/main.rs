mod alchemy;
mod alerter;
mod analyzer;
mod cache;
mod config;
mod events;
mod npm;
mod pastebin;
mod poller;
mod validator;

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

type SharedCache = Arc<Mutex<cache::Cache>>;
type SharedValidator = Arc<validator::Validator>;
type SharedAlchemy = Arc<alchemy::OnChainValidator>;
type SharedAlerter = Arc<alerter::Alerter>;
type SharedPoller = Arc<poller::Poller>;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let cfg = config::load("config.toml")?;

    let cache: SharedCache = Arc::new(Mutex::new(cache::Cache::new("scanner.db")?));
    let poller: SharedPoller = Arc::new(poller::Poller::new(cfg.github.token.clone()));
    let validator: SharedValidator = Arc::new(validator::Validator::new(
        cfg.openrouter.api_key.clone(),
        cfg.openrouter.model.clone(),
        cfg.openrouter.fallback_models.clone(),
    ));
    let alchemy: SharedAlchemy = Arc::new(alchemy::OnChainValidator::new());
    let alerter: SharedAlerter = Arc::new(alerter::Alerter::new(
        cfg.telegram.bot_token.clone(),
        cfg.telegram.chat_id.clone(),
        cfg.telegram.message_thread_id,
    ));

    info!(
        "Scanner started. {} keywords, polling every {}s",
        cfg.github.keywords.len(),
        cfg.github.interval_secs
    );

    let startup_msg = format!(
        "✅ *Scanner aktif*\n\n🔍 Keyword search: {} keywords\n⚡ Events stream: aktif\nInterval: {}s",
        cfg.github.keywords.len(),
        cfg.github.interval_secs,
    );
    if let Err(e) = alerter.notify(&startup_msg).await {
        error!("Failed to send startup notification: {}", e);
    }

    let keywords = cfg.github.keywords.clone();
    let interval_secs = cfg.github.interval_secs;
    let token = cfg.github.token.clone();

    let keyword_task = tokio::spawn({
        let (c, p, v, a, al) = (cache.clone(), poller.clone(), validator.clone(), alchemy.clone(), alerter.clone());
        async move { keyword_scan_loop(keywords, interval_secs, c, p, v, a, al).await; }
    });

    let events_task = tokio::spawn({
        let (c, p, v, a, al) = (cache.clone(), poller.clone(), validator.clone(), alchemy.clone(), alerter.clone());
        async move { events_scan_loop(token, c, p, v, a, al).await; }
    });

    let pastebin_task = tokio::spawn({
        let (c, v, a, al) = (cache.clone(), validator.clone(), alchemy.clone(), alerter.clone());
        async move { pastebin_scan_loop(c, v, a, al).await; }
    });

    let npm_task = tokio::spawn({
        let (c, v, a, al) = (cache.clone(), validator.clone(), alchemy.clone(), alerter.clone());
        let kw = cfg.github.keywords.clone();
        async move { npm_scan_loop(kw, c, v, a, al).await; }
    });

    tokio::select! {
        _ = keyword_task => error!("Keyword scan task exited unexpectedly"),
        _ = events_task => error!("Events scan task exited unexpectedly"),
        _ = pastebin_task => error!("Pastebin scan task exited unexpectedly"),
        _ = npm_task => error!("npm scan task exited unexpectedly"),
    }

    Ok(())
}

async fn keyword_scan_loop(
    keywords: Vec<String>,
    interval_secs: u64,
    cache: SharedCache,
    poller: SharedPoller,
    validator: SharedValidator,
    alchemy: SharedAlchemy,
    alerter: SharedAlerter,
) {
    loop {
        for keyword in &keywords {
            info!("[search] Searching: {}", keyword);
            tokio::time::sleep(Duration::from_secs(2)).await;

            let items = match poller.search(keyword).await {
                Ok(items) => items,
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("rate limit") || msg.contains("429") || msg.contains("403") {
                        warn!("[search] Rate limit — pause 60s");
                        tokio::time::sleep(Duration::from_secs(60)).await;
                    } else {
                        warn!("[search] Error for '{}': {}", keyword, e);
                    }
                    continue;
                }
            };

            info!("[search] Found {} results for '{}'", items.len(), keyword);

            for item in items {
                let file_key = format!("file/{}/{}/{}", item.repository.full_name, item.path, item.sha);
                {
                    let c = cache.lock().await;
                    if c.is_seen(&file_key) { continue; }
                }

                let content = match poller.fetch_content(&item).await {
                    Ok(c) => c,
                    Err(e) => { warn!("[search] Fetch error: {}", e); String::new() }
                };

                if !content.is_empty() {
                    let result = analyzer::analyze(&content);
                    if result.found {
                        process_finding(&validator, &alchemy, &alerter, &item.repository.full_name, &item.path, &result.secrets, &content, &item.html_url, "[search]").await;
                    }
                    cache.lock().await.mark_seen(&file_key).ok();
                }

                let repo = &item.repository.full_name;
                let commits = match poller.fetch_recent_commits(repo).await {
                    Ok(c) => c,
                    Err(e) => { warn!("[search] Commits error for {}: {}", repo, e); continue; }
                };

                for commit in commits {
                    scan_commit(repo, &commit.sha, &commit.html_url, &poller, &validator, &alchemy, &alerter, &cache, "[search]").await;
                }
            }
        }

        info!("[search] Cycle complete. Sleeping {}s", interval_secs);
        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
    }
}

async fn events_scan_loop(
    token: Option<String>,
    cache: SharedCache,
    poller: SharedPoller,
    validator: SharedValidator,
    alchemy: SharedAlchemy,
    alerter: SharedAlerter,
) {
    let mut event_poller = events::EventPoller::new(token);

    loop {
        let interval = event_poller.poll_interval();

        match event_poller.poll().await {
            Ok(commits) => {
                for commit in commits {
                    scan_commit(&commit.repo, &commit.sha, &commit.html_url, &poller, &validator, &alchemy, &alerter, &cache, "[events]").await;
                }
            }
            Err(e) => warn!("[events] Poll error: {}", e),
        }

        tokio::time::sleep(interval).await;
    }
}

async fn scan_commit(
    repo: &str,
    sha: &str,
    html_url: &str,
    poller: &SharedPoller,
    validator: &SharedValidator,
    alchemy: &SharedAlchemy,
    alerter: &SharedAlerter,
    cache: &SharedCache,
    tag: &str,
) {
    let commit_key = format!("commit/{}/{}", repo, sha);
    {
        let c = cache.lock().await;
        if c.is_seen(&commit_key) { return; }
    }

    let detail = match poller.fetch_commit_detail(repo, sha).await {
        Ok(d) => d,
        Err(e) => { warn!("{} Commit detail error {}: {}", tag, sha, e); return; }
    };

    let files = detail.files.unwrap_or_default();
    for file in &files {
        let patch = match &file.patch {
            Some(p) => p,
            None => continue,
        };
        let added: String = patch
            .lines()
            .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
            .map(|l| &l[1..])
            .collect::<Vec<_>>()
            .join("\n");

        let result = analyzer::analyze(&added);
        if result.found {
            process_finding(validator, alchemy, alerter, repo, &file.filename, &result.secrets, &added, html_url, tag).await;
        }
    }

    cache.lock().await.mark_seen(&commit_key).ok();
}

async fn process_finding(
    validator: &SharedValidator,
    alchemy: &SharedAlchemy,
    alerter: &SharedAlerter,
    repo: &str,
    path: &str,
    secrets: &[String],
    content: &str,
    link: &str,
    tag: &str,
) {
    match validator.validate(content, secrets).await {
        Ok(v) if v.is_real => {
            info!("{} OpenRouter confirmed {}/{}: {}", tag, repo, path, v.reason);
            send_with_alchemy(alchemy, alerter, repo, path, secrets, content, link).await;
        }
        Ok(v) => info!("{} False positive in {}/{}: {}", tag, repo, path, v.reason),
        Err(e) => warn!("{} Validator error for {}/{}: {}", tag, repo, path, e),
    }
}


async fn npm_scan_loop(
    keywords: Vec<String>,
    cache: SharedCache,
    validator: SharedValidator,
    alchemy: SharedAlchemy,
    alerter: SharedAlerter,
) {
    let scanner = npm::NpmScanner::new();
    loop {
        for keyword in &keywords {
            tokio::time::sleep(Duration::from_secs(5)).await;

            let files = match scanner.scan_recent(keyword).await {
                Ok(f) => f,
                Err(e) => { warn!("[npm] Error for '{}': {}", keyword, e); continue; }
            };

            if !files.is_empty() {
                info!("[npm] Found {} sensitive files for '{}'", files.len(), keyword);
            }

            for file in files {
                let cache_key = format!("npm/{}/{}", file.package, file.filename);
                { if cache.lock().await.is_seen(&cache_key) { continue; } }

                let result = analyzer::analyze(&file.content);
                if result.found {
                    process_finding(&validator, &alchemy, &alerter, &format!("npm:{}", file.package), &file.filename, &result.secrets, &file.content, &file.url, "[npm]").await;
                }
                cache.lock().await.mark_seen(&cache_key).ok();
            }
        }

        info!("[npm] Cycle complete. Sleeping 300s");
        tokio::time::sleep(Duration::from_secs(300)).await;
    }
}

async fn pastebin_scan_loop(
    cache: SharedCache,
    validator: SharedValidator,
    alchemy: SharedAlchemy,
    alerter: SharedAlerter,
) {
    let mut poller = pastebin::PastebinPoller::new();
    loop {
        match poller.poll().await {
            Ok(pastes) => {
                info!("[pastebin] {} new pastes", pastes.len());
                for paste in pastes {
                    let cache_key = format!("pastebin/{}", paste.key);
                    { if cache.lock().await.is_seen(&cache_key) { continue; } }

                    let result = analyzer::analyze(&paste.content);
                    if result.found {
                        process_finding(&validator, &alchemy, &alerter, "pastebin.com", &paste.key, &result.secrets, &paste.content, &paste.url, "[pastebin]").await;
                    }
                    cache.lock().await.mark_seen(&cache_key).ok();
                }
            }
            Err(e) => warn!("[pastebin] Poll error: {}", e),
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
    }
}

async fn send_with_alchemy(
    alchemy: &SharedAlchemy,
    alerter: &SharedAlerter,
    repo: &str,
    path: &str,
    secrets: &[String],
    content: &str,
    link: &str,
) {
    let msg_id = match alerter.send(repo, path, secrets, content, link).await {
        Ok(id) => id,
        Err(e) => { error!("Telegram alert failed: {}", e); return; }
    };

    let chain_results = alchemy.validate(content).await;
    let onchain_status = if chain_results.is_empty() {
        "🔍 On-chain: _tidak ada wallet terdeteksi_".to_string()
    } else {
        let lines: Vec<String> = chain_results.iter().map(|r| {
            let status = if r.is_active { "✅ aktif" } else { "⚪ inactive" };
            info!("On-chain {}: {} — {:.6} {} ({})", r.chain, r.address, r.balance, r.chain, status);
            format!("🔑 {} `{}` — {:.6} {} ({})", r.chain, r.address, r.balance, r.chain, status)
        }).collect();
        format!("🔍 On-chain:\n{}", lines.join("\n"))
    };

    if let Err(e) = alerter.edit_onchain(msg_id, repo, path, secrets, content, link, &onchain_status).await {
        error!("Telegram edit failed: {}", e);
    }
}
