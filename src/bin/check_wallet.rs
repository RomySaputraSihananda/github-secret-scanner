use std::env;

#[tokio::main]
async fn main() {
    let keypair_b58 = "izhECg4wjYvuw1y7TYLuu6qdkb1ruiRPz7LUgfo1Vt55UwLTGyk1pYqgEQt2NGY1nwcukEJ4Q5momZpKrApfGu4";
    let alchemy_key = env::var("ALCHEMY_KEY").unwrap_or_else(|_| "sJvPMnFHOWOoXZ2poHLPj".to_string());

    // Decode base58 keypair
    let bytes = match bs58::decode(keypair_b58).into_vec() {
        Ok(b) => b,
        Err(e) => { eprintln!("Base58 decode error: {}", e); return; }
    };

    println!("Keypair bytes: {} bytes", bytes.len());

    if bytes.len() != 64 {
        eprintln!("Expected 64 bytes, got {}", bytes.len());
        return;
    }

    // Last 32 bytes = public key
    let pubkey = bs58::encode(&bytes[32..]).into_string();
    println!("Public key (wallet address): {}", pubkey);

    let client = reqwest::Client::new();
    let url = format!("https://solana-mainnet.g.alchemy.com/v2/{}", alchemy_key);

    // Check balance
    let bal_resp: serde_json::Value = client
        .post(&url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "getBalance",
            "params": [pubkey]
        }))
        .send().await.unwrap()
        .json().await.unwrap();

    let lamports = bal_resp["result"]["value"].as_u64().unwrap_or(0);
    let sol = lamports as f64 / 1_000_000_000.0;
    println!("Balance (mainnet): {} SOL ({} lamports)", sol, lamports);

    // Check transaction history
    let tx_resp: serde_json::Value = client
        .post(&url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "getSignaturesForAddress",
            "params": [pubkey, {"limit": 5}]
        }))
        .send().await.unwrap()
        .json().await.unwrap();

    let txs = tx_resp["result"].as_array();
    match txs {
        Some(list) if !list.is_empty() => {
            println!("Transactions found: {}", list.len());
            for tx in list {
                println!("  - sig: {}", tx["signature"].as_str().unwrap_or("?"));
            }
        }
        _ => println!("No transactions on mainnet"),
    }
}
