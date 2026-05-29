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
