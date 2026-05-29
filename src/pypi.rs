use flate2::read::GzDecoder;
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::Client;
use serde::Deserialize;
use std::io::Read;
use tar::Archive;

static TITLE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"<title>([^<]+?) ([0-9][^<\s]*)</title>").unwrap()
});

const SENSITIVE_PATTERNS: &[&str] = &[
    ".env", ".env.local", ".env.production", ".env.development",
    "config.json", "secrets.json", "credentials.json",
    "keypair.json", "wallet.json", "id.json",
    "settings.py", "local_settings.py", "config.py",
    "private.key", "private.pem",
];

#[derive(Deserialize)]
struct PypiInfo {
    urls: Vec<DistFile>,
}

#[derive(Deserialize)]
struct DistFile {
    url: String,
    packagetype: String,
    filename: String,
}

pub struct ScannedFile {
    pub package: String,
    pub filename: String,
    pub content: String,
    pub url: String,
}

pub struct PypiScanner {
    client: Client,
}

impl PypiScanner {
    pub fn new() -> Self {
        Self { client: Client::new() }
    }

    pub async fn scan_recent(&self) -> Result<Vec<ScannedFile>, Box<dyn std::error::Error + Send + Sync>> {
        let rss = self
            .client
            .get("https://pypi.org/rss/updates.xml")
            .header("User-Agent", "github-secret-scanner/1.0")
            .send()
            .await?
            .text()
            .await?;

        let packages: Vec<(String, String)> = TITLE_RE
            .captures_iter(&rss)
            .filter_map(|c| {
                let name = c[1].trim().to_string();
                let version = c[2].trim().to_string();
                if name.to_lowercase() == "pypi recent updates" { None } else { Some((name, version)) }
            })
            .take(30)
            .collect();

        let mut findings = Vec::new();

        for (name, version) in packages {
            let info_url = format!("https://pypi.org/pypi/{}/{}/json", name, version);
            let info: PypiInfo = match self.client.get(&info_url).send().await?.json().await {
                Ok(i) => i,
                Err(_) => continue,
            };

            let sdist = info.urls.iter().find(|f| f.packagetype == "sdist");
            if let Some(dist) = sdist {
                let bytes = match self.client.get(&dist.url).send().await?.bytes().await {
                    Ok(b) => b,
                    Err(_) => continue,
                };

                if dist.filename.ends_with(".tar.gz") {
                    let scanned = extract_from_targz(&bytes, &name, &version);
                    findings.extend(scanned);
                }
            }
        }

        Ok(findings)
    }
}

fn extract_from_targz(data: &[u8], pkg_name: &str, version: &str) -> Vec<ScannedFile> {
    let gz = GzDecoder::new(data);
    let mut archive = Archive::new(gz);
    let mut results = Vec::new();

    let entries = match archive.entries() {
        Ok(e) => e,
        Err(_) => return results,
    };

    for entry in entries.flatten() {
        let path = match entry.path() {
            Ok(p) => p.to_string_lossy().to_string(),
            Err(_) => continue,
        };

        let filename = path.split('/').last().unwrap_or("").to_lowercase();
        let is_sensitive = SENSITIVE_PATTERNS.iter().any(|p| filename == *p || filename.starts_with(".env"));

        if !is_sensitive {
            continue;
        }

        let mut content = String::new();
        let mut entry = entry;
        if entry.read_to_string(&mut content).is_err() {
            continue;
        }

        if content.len() > 50_000 {
            continue;
        }

        results.push(ScannedFile {
            package: pkg_name.to_string(),
            filename: path,
            content,
            url: format!("https://pypi.org/project/{}/{}/", pkg_name, version),
        });
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_title_regex_extracts_package() {
        let rss = "<title>my-package 1.2.3</title>";
        let caps: Vec<_> = TITLE_RE.captures_iter(rss).collect();
        assert_eq!(caps[0][1].trim(), "my-package");
        assert_eq!(caps[0][2].trim(), "1.2.3");
    }

    #[test]
    fn test_filters_header_title() {
        let rss = "<title>PyPI recent updates 1.0</title><title>real-package 2.0.0</title>";
        let packages: Vec<_> = TITLE_RE
            .captures_iter(rss)
            .filter_map(|c| {
                let name = c[1].trim().to_string();
                if name.to_lowercase() == "pypi recent updates" { None } else { Some(name) }
            })
            .collect();
        assert_eq!(packages, vec!["real-package"]);
    }
}
