use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct OpenRouterRequest {
    model: String,
    messages: Vec<Message>,
}

#[derive(Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct OpenRouterResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: MessageContent,
}

#[derive(Deserialize)]
struct MessageContent {
    content: String,
}

#[derive(Deserialize, Debug)]
pub struct AnalysisResult {
    pub found: bool,
    pub secrets: Vec<String>,
}

pub struct Analyzer {
    client: Client,
    api_key: String,
    model: String,
}

impl Analyzer {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
        }
    }

    pub async fn analyze(&self, content: &str) -> Result<AnalysisResult, Box<dyn std::error::Error + Send + Sync>> {
        let truncated = &content[..content.len().min(2000)];

        let prompt = format!(
            r#"Analyze this code snippet. Does it contain any secrets such as:
- Private keys (Solana, Ethereum, or other crypto)
- Seed phrases or mnemonics
- API keys (AWS, GitHub tokens, etc.)
- .env variable assignments with sensitive values

Code:
```
{}
```

Respond ONLY in JSON format with no extra text: {{"found": true, "secrets": ["description"]}} or {{"found": false, "secrets": []}}"#,
            truncated
        );

        let request = OpenRouterRequest {
            model: self.model.clone(),
            messages: vec![Message {
                role: "user".to_string(),
                content: prompt,
            }],
        };

        let resp = self
            .client
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        let or_resp: OpenRouterResponse = resp.json().await?;
        let raw = &or_resp.choices[0].message.content;
        let json_str = extract_json(raw);
        let result: AnalysisResult = serde_json::from_str(json_str)?;
        Ok(result)
    }
}

fn extract_json(text: &str) -> &str {
    if let (Some(start), Some(end)) = (text.find('{'), text.rfind('}')) {
        return &text[start..=end];
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_clean() {
        let text = r#"{"found": true, "secrets": ["API key"]}"#;
        assert_eq!(extract_json(text), text);
    }

    #[test]
    fn test_extract_json_with_markdown_wrapper() {
        let text = "```json\n{\"found\": false, \"secrets\": []}\n```";
        let extracted = extract_json(text);
        let result: AnalysisResult = serde_json::from_str(extracted).unwrap();
        assert!(!result.found);
    }

    #[test]
    fn test_extract_json_with_preamble() {
        let text = r#"Sure! Here is the result: {"found": true, "secrets": ["private key found"]}"#;
        let extracted = extract_json(text);
        let result: AnalysisResult = serde_json::from_str(extracted).unwrap();
        assert!(result.found);
        assert_eq!(result.secrets[0], "private key found");
    }
}
