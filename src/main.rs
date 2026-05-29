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
