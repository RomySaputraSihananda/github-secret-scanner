use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;
use tracing::{debug, info};

#[derive(Deserialize)]
struct Event {
    #[serde(rename = "type")]
    event_type: String,
    repo: EventRepo,
    payload: serde_json::Value,
}

#[derive(Deserialize)]
struct EventRepo {
    name: String,
}

pub struct PushCommit {
    pub repo: String,
    pub sha: String,
    pub html_url: String,
}

pub struct EventPoller {
    client: Client,
    token: Option<String>,
    etag: Option<String>,
    poll_interval_secs: u64,
}

impl EventPoller {
    pub fn new(token: Option<String>) -> Self {
        Self {
            client: Client::new(),
            token,
            etag: None,
            poll_interval_secs: 60,
        }
    }

    pub async fn poll(&mut self) -> Result<Vec<PushCommit>, Box<dyn std::error::Error + Send + Sync>> {
        let mut req = self
            .client
            .get("https://api.github.com/events")
            .query(&[("per_page", "100")])
            .header("Accept", "application/vnd.github.v3+json")
            .header("User-Agent", "github-secret-scanner/1.0");

        if let Some(token) = &self.token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }
        if let Some(etag) = &self.etag {
            req = req.header("If-None-Match", etag.clone());
        }

        let resp = req.send().await?;

        // Ambil poll interval dari header GitHub
        if let Some(interval) = resp.headers().get("X-Poll-Interval") {
            if let Ok(s) = interval.to_str() {
                if let Ok(n) = s.parse::<u64>() {
                    self.poll_interval_secs = n;
                }
            }
        }

        // Simpan ETag untuk request berikutnya
        if let Some(etag) = resp.headers().get("ETag") {
            self.etag = Some(etag.to_str().unwrap_or("").to_string());
        }

        // 304 Not Modified — tidak ada event baru
        if resp.status().as_u16() == 304 {
            debug!("Events: no new events (304)");
            return Ok(vec![]);
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await?;
            return Err(format!("Events API error {}: {}", status, &body[..body.len().min(200)]).into());
        }

        let events: Vec<Event> = resp.json().await?;

        let commits: Vec<PushCommit> = events
            .into_iter()
            .filter(|e| e.event_type == "PushEvent")
            .flat_map(|e| {
                let repo = e.repo.name.clone();
                let commits = e.payload["commits"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                commits.into_iter().filter_map(move |c| {
                    let sha = c["sha"].as_str()?.to_string();
                    let html_url = format!("https://github.com/{}/commit/{}", repo, sha);
                    Some(PushCommit { repo: repo.clone(), sha, html_url })
                })
            })
            .collect();

        info!("Events: {} new push commits", commits.len());
        Ok(commits)
    }

    pub fn poll_interval(&self) -> Duration {
        Duration::from_secs(self.poll_interval_secs)
    }
}
