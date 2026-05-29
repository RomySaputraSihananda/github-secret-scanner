use k256::ecdsa::SigningKey;
use k256::elliptic_curve::sec1::ToEncodedPoint;
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::Client;
use serde_json::json;
use sha3::{Digest, Keccak256};

static BYTE_ARRAY_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\[\s*((?:\d{1,3}\s*,\s*){63}\d{1,3})\s*\]").unwrap()
});

static BASE58_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"[1-9A-HJ-NP-Za-km-z]{87,88}").unwrap()
});

static ETH_HEX_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"0x([0-9a-fA-F]{64})").unwrap()
});

#[derive(Debug)]
pub struct OnChainResult {
    pub is_active: bool,
    pub balance: f64,
    pub address: String,
    pub chain: Chain,
}

#[derive(Debug)]
pub enum Chain {
    Solana,
    Ethereum,
}

impl std::fmt::Display for Chain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Chain::Solana => write!(f, "SOL"),
            Chain::Ethereum => write!(f, "ETH"),
        }
    }
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

    pub async fn validate(&self, content: &str) -> Vec<OnChainResult> {
        let mut results = Vec::new();

        // Solana check
        if let Some(pubkey) = extract_solana_pubkey(content) {
            match self.check_solana(&pubkey).await {
                Ok(r) => results.push(r),
                Err(e) => tracing::warn!("Solana check failed for {}: {}", pubkey, e),
            }
        }

        // Ethereum check
        if let Some(address) = extract_eth_address(content) {
            match self.check_ethereum(&address).await {
                Ok(r) => results.push(r),
                Err(e) => tracing::warn!("ETH check failed for {}: {}", address, e),
            }
        }

        results
    }

    async fn check_solana(
        &self,
        pubkey: &str,
    ) -> Result<OnChainResult, Box<dyn std::error::Error + Send + Sync>> {
        // Coba Alchemy dulu, fallback ke public RPC kalau 429/error
        let alchemy_url = format!("https://solana-mainnet.g.alchemy.com/v2/{}", self.api_key);
        let endpoints = [
            (alchemy_url.as_str(), "mainnet"),
            ("https://api.mainnet-beta.solana.com", "mainnet"),
            ("https://rpc.ankr.com/solana", "mainnet"),
            ("https://api.devnet.solana.com", "devnet"),
        ];

        for (url, network) in endpoints {
            match self.check_sol_rpc(pubkey, url, network).await {
                Ok(r) if r.is_active => return Ok(r),
                Ok(r) => {
                    // mainnet kosong — cek devnet juga
                    if network == "devnet" { return Ok(r); }
                    continue;
                }
                Err(e) => {
                    tracing::debug!("SOL RPC {} failed: {}", url, e);
                    continue;
                }
            }
        }
        // Semua endpoint dicoba, return hasil terakhir
        self.check_sol_rpc(pubkey, "https://api.mainnet-beta.solana.com", "mainnet").await
    }

    async fn check_sol_rpc(
        &self,
        pubkey: &str,
        rpc_url: &str,
        network: &str,
    ) -> Result<OnChainResult, Box<dyn std::error::Error + Send + Sync>> {
        let bal: serde_json::Value = self.client.post(rpc_url)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"getBalance","params":[pubkey]}))
            .send().await?.json().await?;

        let lamports = bal["result"]["value"].as_u64().unwrap_or(0);
        let balance = lamports as f64 / 1_000_000_000.0;

        let sig: serde_json::Value = self.client.post(rpc_url)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"getSignaturesForAddress","params":[pubkey,{"limit":1}]}))
            .send().await?.json().await?;

        let has_tx = sig["result"].as_array().map(|a| !a.is_empty()).unwrap_or(false);

        Ok(OnChainResult {
            is_active: balance > 0.0 || has_tx,
            balance,
            address: format!("{} ({})", pubkey, network),
            chain: Chain::Solana,
        })
    }

    async fn check_ethereum(
        &self,
        address: &str,
    ) -> Result<OnChainResult, Box<dyn std::error::Error + Send + Sync>> {
        // Coba beberapa public ETH RPC
        let alchemy_url = format!("https://eth-mainnet.g.alchemy.com/v2/{}", self.api_key);
        let urls = [
            alchemy_url.as_str(),
            "https://eth.llamarpc.com",
            "https://rpc.ankr.com/eth",
            "https://cloudflare-eth.com",
        ];

        for url in urls {
            match self.try_check_ethereum(address, url).await {
                Ok(r) => return Ok(r),
                Err(e) => tracing::debug!("ETH RPC {} failed: {}", url, e),
            }
        }
        Err("All ETH RPC endpoints failed".into())
    }

    async fn try_check_ethereum(
        &self,
        address: &str,
        url: &str,
    ) -> Result<OnChainResult, Box<dyn std::error::Error + Send + Sync>> {
        let url = url.to_string();

        let bal: serde_json::Value = self.client.post(&url)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"eth_getBalance","params":[address,"latest"]}))
            .send().await?.json().await?;

        // Balance in hex wei
        let wei_hex = bal["result"].as_str().unwrap_or("0x0");
        let wei = u128::from_str_radix(wei_hex.trim_start_matches("0x"), 16).unwrap_or(0);
        let eth = wei as f64 / 1e18;

        let tx: serde_json::Value = self.client.post(&url)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"eth_getTransactionCount","params":[address,"latest"]}))
            .send().await?.json().await?;

        let nonce_hex = tx["result"].as_str().unwrap_or("0x0");
        let nonce = u64::from_str_radix(nonce_hex.trim_start_matches("0x"), 16).unwrap_or(0);

        Ok(OnChainResult {
            is_active: eth > 0.0 || nonce > 0,
            balance: eth,
            address: address.to_string(),
            chain: Chain::Ethereum,
        })
    }
}

fn extract_solana_pubkey(content: &str) -> Option<String> {
    if let Some(caps) = BYTE_ARRAY_RE.captures(content) {
        let nums: Vec<u8> = caps[1]
            .split(',')
            .filter_map(|s| s.trim().parse::<u8>().ok())
            .collect();
        if nums.len() == 64 {
            return Some(bs58::encode(&nums[32..]).into_string());
        }
    }
    if let Some(m) = BASE58_RE.find(content) {
        if let Ok(decoded) = bs58::decode(m.as_str()).into_vec() {
            if decoded.len() == 64 {
                return Some(bs58::encode(&decoded[32..]).into_string());
            }
        }
    }
    None
}

fn extract_eth_address(content: &str) -> Option<String> {
    let caps = ETH_HEX_RE.captures(content)?;
    let privkey_hex = &caps[1];
    let privkey_bytes = hex::decode(privkey_hex).ok()?;

    let signing_key = SigningKey::from_bytes(privkey_bytes.as_slice().into()).ok()?;
    let verifying_key = signing_key.verifying_key();
    // Uncompressed pubkey: 65 bytes (04 + X + Y), skip first byte
    let pubkey_bytes = verifying_key.to_encoded_point(false);
    let pubkey_uncompressed = pubkey_bytes.as_bytes();
    if pubkey_uncompressed.len() != 65 {
        return None;
    }
    // Keccak256 of X||Y (skip 04 prefix), take last 20 bytes
    let hash = Keccak256::digest(&pubkey_uncompressed[1..]);
    let address = format!("0x{}", hex::encode(&hash[12..]));
    Some(address)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_solana_from_byte_array() {
        let bytes: Vec<u8> = (0u8..64).collect();
        let content = format!(
            "let keypair = [{}];",
            bytes.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(",")
        );
        let pubkey = extract_solana_pubkey(&content);
        assert!(pubkey.is_some());
        assert_eq!(pubkey.unwrap(), bs58::encode(&bytes[32..]).into_string());
    }

    #[test]
    fn test_extract_eth_address_from_hex_key() {
        // Known test vector: privkey -> address
        let content = "PRIVATE_KEY=0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
        let address = extract_eth_address(content);
        assert!(address.is_some());
        // This is the well-known Hardhat/Anvil test account #0
        assert_eq!(
            address.unwrap().to_lowercase(),
            "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266"
        );
    }

    #[test]
    fn test_no_false_positive() {
        let content = "fn main() { let x = 42; }";
        assert!(extract_solana_pubkey(content).is_none());
        assert!(extract_eth_address(content).is_none());
    }
}
