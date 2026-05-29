use flate2::read::GzDecoder;
use reqwest::Client;
use serde::Deserialize;
use std::io::Read;
use tar::Archive;

const SENSITIVE_PATTERNS: &[&str] = &[
    ".env", ".env.local", ".env.production", ".env.development",
    "config.json", "secrets.json", "credentials.json",
    "keypair.json", "wallet.json", "id.json",
    "private.key", "private.pem",
];

#[derive(Deserialize)]
struct SearchResponse {
    objects: Vec<SearchObject>,
}

#[derive(Deserialize)]
struct SearchObject {
    package: PackageMeta,
}

#[derive(Deserialize)]
struct PackageMeta {
    name: String,
    version: String,
}

#[derive(Deserialize)]
struct PackageManifest {
    dist: Dist,
}

#[derive(Deserialize)]
struct Dist {
    tarball: String,
}

pub struct ScannedFile {
    pub package: String,
    pub filename: String,
    pub content: String,
    pub url: String,
}

pub struct NpmScanner {
    client: Client,
}

impl NpmScanner {
    pub fn new() -> Self {
        Self { client: Client::new() }
    }

    pub async fn scan_recent(
        &self,
        keyword: &str,
    ) -> Result<Vec<ScannedFile>, Box<dyn std::error::Error + Send + Sync>> {
        let resp: SearchResponse = self
            .client
            .get("https://registry.npmjs.org/-/v1/search")
            .query(&[("text", keyword), ("size", "20")])
            .header("User-Agent", "github-secret-scanner/1.0")
            .send()
            .await?
            .json()
            .await?;

        let mut findings = Vec::new();

        for obj in resp.objects {
            let pkg = &obj.package;
            let manifest_url = format!("https://registry.npmjs.org/{}/{}", pkg.name, pkg.version);

            let manifest: PackageManifest = match self
                .client
                .get(&manifest_url)
                .send()
                .await?
                .json()
                .await
            {
                Ok(m) => m,
                Err(_) => continue,
            };

            let tarball_bytes = match self
                .client
                .get(&manifest.dist.tarball)
                .send()
                .await?
                .bytes()
                .await
            {
                Ok(b) => b,
                Err(_) => continue,
            };

            let scanned = extract_sensitive_files(&tarball_bytes, &pkg.name, &pkg.version);
            findings.extend(scanned);
        }

        Ok(findings)
    }
}

fn extract_sensitive_files(tarball: &[u8], pkg_name: &str, version: &str) -> Vec<ScannedFile> {
    let gz = GzDecoder::new(tarball);
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
            filename: path.clone(),
            content,
            url: format!("https://www.npmjs.com/package/{}/v/{}", pkg_name, version),
        });
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sensitive_pattern_match() {
        assert!(SENSITIVE_PATTERNS.iter().any(|p| ".env" == *p));
        assert!(SENSITIVE_PATTERNS.iter().any(|p| "keypair.json" == *p));
    }

    #[test]
    fn test_env_prefix_match() {
        let filename = ".env.production";
        assert!(filename.starts_with(".env"));
    }
}
