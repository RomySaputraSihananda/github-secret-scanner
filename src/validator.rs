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
    models: Vec<String>,
}

impl Validator {
    pub fn new(api_key: String, model: String, fallback_models: Vec<String>) -> Self {
        let mut models = vec![model];
        models.extend(fallback_models);
        Self {
            client: Client::new(),
            api_key,
            models,
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

        // Rotasi model — skip ke berikutnya kalau 429
        for (i, model) in self.models.iter().enumerate() {
            let request = OpenRouterRequest {
                model: model.clone(),
                messages: vec![Message {
                    role: "user".to_string(),
                    content: prompt.clone(),
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

            if status.as_u16() == 429 {
                tracing::warn!("Model {} rate limited, trying next ({}/{})", model, i + 1, self.models.len());
                continue;
            }

            if !status.is_success() {
                return Err(format!("OpenRouter error {} (model={}): {}", status, model, body).into());
            }

            let or_resp: OpenRouterResponse = serde_json::from_str(&body)
                .map_err(|e| format!("Parse error: {e}\nBody: {body}"))?;

            let raw = &or_resp.choices[0].message.content;
            let json_str = extract_json(raw);
            let result = serde_json::from_str::<ValidationResult>(json_str)
                .unwrap_or_else(|_| parse_fallback(raw));

            tracing::debug!("Validated with model: {}", model);
            return Ok(result);
        }

        Err("All models rate limited or failed".into())
    }
}

fn extract_json(text: &str) -> &str {
    if let (Some(start), Some(end)) = (text.find('{'), text.rfind('}')) {
        return &text[start..=end];
    }
    text
}

// Fallback: if JSON is malformed, look for is_real true/false in raw text
fn parse_fallback(text: &str) -> ValidationResult {
    let lower = text.to_lowercase();
    let is_real = lower.contains("\"is_real\": true")
        || lower.contains("\"is_real\":true")
        || lower.contains("is_real: true");
    ValidationResult {
        is_real,
        reason: "parsed via fallback".to_string(),
    }
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
