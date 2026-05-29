use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct Config {
    pub github: GithubConfig,
    pub openrouter: OpenRouterConfig,
    pub telegram: TelegramConfig,
    pub alchemy: AlchemyConfig,
}

#[derive(Deserialize, Debug)]
pub struct GithubConfig {
    pub token: Option<String>,
    pub keywords: Vec<String>,
    pub interval_secs: u64,
}

#[derive(Deserialize, Debug)]
pub struct OpenRouterConfig {
    pub api_key: String,
    pub model: String,
    #[serde(default)]
    pub fallback_models: Vec<String>,
}

#[derive(Deserialize, Debug)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub chat_id: String,
    pub message_thread_id: Option<i64>,
}

#[derive(Deserialize, Debug)]
pub struct AlchemyConfig {
    pub api_key: String,
}

pub fn load(path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&content)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_config() {
        let toml = r#"
[github]
token = "ghp_test"
keywords = ["solana private key"]
interval_secs = 60

[openrouter]
api_key = "sk-or-test"
model = "meta-llama/llama-3.1-8b-instruct:free"

[telegram]
bot_token = "123:ABC"
chat_id = "456"

[alchemy]
api_key = "test_alchemy_key"
"#;
        let path = "/tmp/test_config.toml";
        std::fs::write(path, toml).unwrap();
        let config = load(path).unwrap();
        assert_eq!(config.github.keywords[0], "solana private key");
        assert_eq!(config.github.interval_secs, 60);
        assert_eq!(config.openrouter.model, "meta-llama/llama-3.1-8b-instruct:free");
        assert_eq!(config.telegram.chat_id, "456");
    }

    #[test]
    fn test_load_config_token_optional() {
        let toml = r#"
[github]
keywords = ["test"]
interval_secs = 120

[openrouter]
api_key = "sk"
model = "model"

[telegram]
bot_token = "tok"
chat_id = "id"

[alchemy]
api_key = "test_key"
"#;
        let path = "/tmp/test_config_no_token.toml";
        std::fs::write(path, toml).unwrap();
        let config = load(path).unwrap();
        assert!(config.github.token.is_none());
    }
}
