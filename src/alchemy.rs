use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::Client;
use serde_json::json;

static BYTE_ARRAY_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\[\s*((?:\d{1,3}\s*,\s*){63}\d{1,3})\s*\]").unwrap()
});

static BASE58_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"[1-9A-HJ-NP-Za-km-z]{87,88}").unwrap()
});

#[derive(Debug)]
pub struct OnChainResult {
    pub is_active: bool,
    pub balance_sol: f64,
    pub address: String,
}

pub struct AlchemyValidator {
    client: Client,
    api_key: String,
}

impl AlchemyValidator {
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
        }
    }

    pub async fn validate(&self, content: &str) -> Option<OnChainResult> {
        let pubkey = extract_solana_pubkey(content)?;
        match self.check_wallet(&pubkey).await {
            Ok(result) => Some(result),
            Err(e) => {
                tracing::warn!("Alchemy check failed for {}: {}", pubkey, e);
                None
            }
        }
    }

    async fn check_wallet(
        &self,
        pubkey: &str,
    ) -> Result<OnChainResult, Box<dyn std::error::Error + Send + Sync>> {
        // Cek mainnet dulu, fallback ke devnet kalau kosong
        let mainnet = self.check_rpc(pubkey, &format!("https://solana-mainnet.g.alchemy.com/v2/{}", self.api_key)).await;
        if let Ok(r) = &mainnet {
            if r.is_active {
                return mainnet;
            }
        }
        // Fallback public devnet
        self.check_rpc(pubkey, "https://api.devnet.solana.com").await
    }

    async fn check_rpc(
        &self,
        pubkey: &str,
        rpc_url: &str,
    ) -> Result<OnChainResult, Box<dyn std::error::Error + Send + Sync>> {
        let balance_resp: serde_json::Value = self
            .client
            .post(rpc_url)
            .json(&json!({
                "jsonrpc": "2.0", "id": 1,
                "method": "getBalance",
                "params": [pubkey]
            }))
            .send().await?.json().await?;

        let lamports = balance_resp["result"]["value"].as_u64().unwrap_or(0);
        let balance_sol = lamports as f64 / 1_000_000_000.0;

        let sig_resp: serde_json::Value = self
            .client
            .post(rpc_url)
            .json(&json!({
                "jsonrpc": "2.0", "id": 1,
                "method": "getSignaturesForAddress",
                "params": [pubkey, {"limit": 1}]
            }))
            .send().await?.json().await?;

        let has_tx = sig_resp["result"].as_array().map(|a| !a.is_empty()).unwrap_or(false);
        let network = if rpc_url.contains("devnet") { "devnet" } else { "mainnet" };

        Ok(OnChainResult {
            is_active: balance_sol > 0.0 || has_tx,
            balance_sol,
            address: format!("{} ({})", pubkey, network),
        })
    }
}

fn extract_solana_pubkey(content: &str) -> Option<String> {
    // Try 64-byte array first (last 32 bytes = pubkey)
    if let Some(caps) = BYTE_ARRAY_RE.captures(content) {
        let nums: Vec<u8> = caps[1]
            .split(',')
            .filter_map(|s| s.trim().parse::<u8>().ok())
            .collect();
        if nums.len() == 64 {
            return Some(bs58::encode(&nums[32..]).into_string());
        }
    }

    // Try base58 encoded keypair (87-88 chars)
    if let Some(m) = BASE58_RE.find(content) {
        if let Ok(decoded) = bs58::decode(m.as_str()).into_vec() {
            if decoded.len() == 64 {
                return Some(bs58::encode(&decoded[32..]).into_string());
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_pubkey_from_64_byte_array() {
        // 64 bytes: first 32 = privkey, last 32 = pubkey
        let bytes: Vec<u8> = (0u8..64).collect();
        let content = format!(
            "let keypair = [{}];",
            bytes.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(",")
        );
        let pubkey = extract_solana_pubkey(&content);
        assert!(pubkey.is_some());
        // pubkey should be base58 of bytes 32..64
        let expected = bs58::encode(&bytes[32..]).into_string();
        assert_eq!(pubkey.unwrap(), expected);
    }

    #[test]
    fn test_extract_pubkey_not_found() {
        let content = "fn main() { println!(\"hello\"); }";
        assert!(extract_solana_pubkey(content).is_none());
    }
}
