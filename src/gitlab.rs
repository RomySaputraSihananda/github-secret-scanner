use reqwest::Client;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct BlobResult {
    pub project_id: u64,
    pub path: String,
    pub ref_field: Option<String>,
    pub data: String,
    pub startline: Option<u64>,
}

impl BlobResult {
    pub fn web_url(&self) -> String {
        format!(
            "https://gitlab.com/api/v4/projects/{}/repository/files/{}/raw",
            self.project_id,
            urlencoding::encode(&self.path)
        )
    }
}

pub struct GitlabPoller {
    client: Client,
    token: Option<String>,
}

impl GitlabPoller {
    pub fn new(token: Option<String>) -> Self {
        Self {
            client: Client::new(),
            token,
        }
    }

    pub async fn search(
        &self,
        keyword: &str,
    ) -> Result<Vec<BlobResult>, Box<dyn std::error::Error + Send + Sync>> {
        let mut req = self
            .client
            .get("https://gitlab.com/api/v4/search")
            .query(&[
                ("scope", "blobs"),
                ("search", keyword),
                ("per_page", "20"),
                ("order_by", "created_at"),
            ])
            .header("User-Agent", "github-secret-scanner/1.0");

        if let Some(token) = &self.token {
            req = req.header("PRIVATE-TOKEN", token.clone());
        }

        let resp = req.send().await?;
        let status = resp.status();
        let body = resp.text().await?;

        if !status.is_success() {
            return Err(format!("GitLab error {}: {}", status, &body[..body.len().min(200)]).into());
        }

        let raw: Vec<serde_json::Value> = serde_json::from_str(&body)?;
        let results = raw
            .into_iter()
            .filter_map(|v| {
                Some(BlobResult {
                    project_id: v["project_id"].as_u64()?,
                    path: v["path"].as_str()?.to_string(),
                    ref_field: v["ref"].as_str().map(|s| s.to_string()),
                    data: v["data"].as_str().unwrap_or("").to_string(),
                    startline: v["startline"].as_u64(),
                })
            })
            .collect();

        Ok(results)
    }

    pub async fn fetch_content(
        &self,
        project_id: u64,
        path: &str,
        ref_name: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!(
            "https://gitlab.com/api/v4/projects/{}/repository/files/{}/raw",
            project_id,
            urlencoding::encode(path)
        );

        let mut req = self
            .client
            .get(&url)
            .query(&[("ref", ref_name)])
            .header("User-Agent", "github-secret-scanner/1.0");

        if let Some(token) = &self.token {
            req = req.header("PRIVATE-TOKEN", token.clone());
        }

        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(format!("GitLab fetch error {}", resp.status()).into());
        }

        Ok(resp.text().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_web_url_encodes_path() {
        let blob = BlobResult {
            project_id: 123,
            path: "src/config/secret.env".to_string(),
            ref_field: Some("main".to_string()),
            data: "PRIVATE_KEY=abc".to_string(),
            startline: Some(1),
        };
        assert!(blob.web_url().contains("123"));
        assert!(blob.web_url().contains("src"));
    }
}
