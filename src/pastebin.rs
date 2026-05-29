use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::Client;
use std::collections::HashSet;

static PASTE_KEY_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"href="/([A-Za-z0-9]{8})\?source=archive""#).unwrap()
});

pub struct PastebinPoller {
    client: Client,
    seen_keys: HashSet<String>,
}

pub struct Paste {
    pub key: String,
    pub content: String,
    pub url: String,
}

impl PastebinPoller {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36")
                .build()
                .unwrap(),
            seen_keys: HashSet::new(),
        }
    }

    pub async fn poll(&mut self) -> Result<Vec<Paste>, Box<dyn std::error::Error + Send + Sync>> {
        let html = self
            .client
            .get("https://pastebin.com/archive")
            .send()
            .await?
            .text()
            .await?;

        let keys: Vec<String> = PASTE_KEY_RE
            .captures_iter(&html)
            .map(|c| c[1].to_string())
            .filter(|k| !self.seen_keys.contains(k))
            .collect::<std::collections::LinkedList<_>>()
            .into_iter()
            .collect::<Vec<_>>();

        let mut pastes = Vec::new();
        for key in keys {
            self.seen_keys.insert(key.clone());

            let url = format!("https://pastebin.com/raw/{}", key);
            match self.client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(content) = resp.text().await {
                        pastes.push(Paste {
                            key: key.clone(),
                            content,
                            url: format!("https://pastebin.com/{}", key),
                        });
                    }
                }
                _ => {}
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }

        Ok(pastes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paste_key_regex() {
        let html = r#"<a href="/abc12345?source=archive">title</a><a href="/XYZ98765?source=archive">other</a>"#;
        let keys: Vec<String> = PASTE_KEY_RE
            .captures_iter(html)
            .map(|c| c[1].to_string())
            .collect();
        assert_eq!(keys, vec!["abc12345", "XYZ98765"]);
    }
}
