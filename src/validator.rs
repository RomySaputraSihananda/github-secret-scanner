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
pub struct ValidationResult {
    pub is_real: bool,
    pub reason: String,
}

pub struct Validator {
    client: Client,
    api_key: String,
    model: String,
}

impl Validator {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
        }
    }

    pub async fn validate(
        &self,
        snippet: &str,
        candidates: &[String],
    ) -> Result<ValidationResult, Box<dyn std::error::Error + Send + Sync>> {
        let candidates_list = candidates.join("\n- ");
        let truncated = &snippet[..snippet.len().min(1500)];

        let prompt = format!(
            r#"You are a crypto security analyst. Regex detected potential secrets in this code snippet.

Detected candidates:
- {candidates_list}

Code snippet:
```
{truncated}
```

Determine if these are REAL secrets or false positives. Real secrets include:
- Actual Solana/Ethereum private keys (not example/placeholder values)
- Real API keys (not "your_api_key_here", "xxxx", "test", "example", etc.)
- Real seed phrases (not word lists, documentation examples)

False positives include: test data, example values, documentation, hex colors, random hashes, tutorial code.

Respond ONLY in JSON: {{"is_real": true/false, "reason": "one sentence explanation"}}"#
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

        let status = resp.status();
        let body = resp.text().await?;

        if !status.is_success() {
            return Err(format!("OpenRouter error {}: {}", status, body).into());
        }

        let or_resp: OpenRouterResponse = serde_json::from_str(&body)
            .map_err(|e| format!("Parse error: {e}\nBody: {body}"))?;

        let raw = &or_resp.choices[0].message.content;
        let json_str = extract_json(raw);
        let result: ValidationResult = serde_json::from_str(json_str)
            .map_err(|e| format!("Validation parse error: {e}\nRaw: {raw}"))?;

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
    fn test_extract_json_from_markdown() {
        let text = "```json\n{\"is_real\": false, \"reason\": \"test data\"}\n```";
        let extracted = extract_json(text);
        let result: ValidationResult = serde_json::from_str(extracted).unwrap();
        assert!(!result.is_real);
    }

    #[test]
    fn test_extract_json_with_preamble() {
        let text = r#"Sure: {"is_real": true, "reason": "looks like a real key"}"#;
        let result: ValidationResult = serde_json::from_str(extract_json(text)).unwrap();
        assert!(result.is_real);
    }
}
