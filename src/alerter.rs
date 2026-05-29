use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct TelegramMessage {
    chat_id: String,
    text: String,
    parse_mode: String,
    disable_web_page_preview: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_thread_id: Option<i64>,
}

#[derive(Serialize)]
struct TelegramEdit {
    chat_id: String,
    message_id: i64,
    text: String,
    parse_mode: String,
    disable_web_page_preview: bool,
}

#[derive(Deserialize)]
struct SendResponse {
    result: Option<MessageResult>,
}

#[derive(Deserialize)]
struct MessageResult {
    message_id: i64,
}

pub struct Alerter {
    client: Client,
    bot_token: String,
    chat_id: String,
    message_thread_id: Option<i64>,
}

impl Alerter {
    pub fn new(bot_token: String, chat_id: String, message_thread_id: Option<i64>) -> Self {
        Self {
            client: Client::new(),
            bot_token,
            chat_id,
            message_thread_id,
        }
    }

    pub async fn notify(&self, text: &str) -> Result<(), reqwest::Error> {
        let message = TelegramMessage {
            chat_id: self.chat_id.clone(),
            text: text.to_string(),
            parse_mode: "Markdown".to_string(),
            disable_web_page_preview: true,
            message_thread_id: self.message_thread_id,
        };
        let url = format!("https://api.telegram.org/bot{}/sendMessage", self.bot_token);
        self.client.post(&url).json(&message).send().await?;
        Ok(())
    }

    /// Kirim alert, return message_id untuk bisa di-edit nanti
    pub async fn send(
        &self,
        repo: &str,
        path: &str,
        secrets: &[String],
        snippet: &str,
        link: &str,
    ) -> Result<i64, reqwest::Error> {
        let text = build_alert_text(repo, path, secrets, snippet, link, "🔍 On-chain: _sedang divalidasi..._");

        let message = TelegramMessage {
            chat_id: self.chat_id.clone(),
            text,
            parse_mode: "Markdown".to_string(),
            disable_web_page_preview: true,
            message_thread_id: self.message_thread_id,
        };

        let url = format!("https://api.telegram.org/bot{}/sendMessage", self.bot_token);
        let resp: SendResponse = self.client
            .post(&url)
            .json(&message)
            .send()
            .await?
            .json()
            .await?;

        Ok(resp.result.map(|r| r.message_id).unwrap_or(0))
    }

    /// Edit message yang sudah dikirim dengan hasil on-chain
    pub async fn edit_onchain(
        &self,
        message_id: i64,
        repo: &str,
        path: &str,
        secrets: &[String],
        snippet: &str,
        link: &str,
        onchain_status: &str,
    ) -> Result<(), reqwest::Error> {
        if message_id == 0 {
            return Ok(());
        }

        let text = build_alert_text(repo, path, secrets, snippet, link, onchain_status);

        let edit = TelegramEdit {
            chat_id: self.chat_id.clone(),
            message_id,
            text,
            parse_mode: "Markdown".to_string(),
            disable_web_page_preview: true,
        };

        let url = format!("https://api.telegram.org/bot{}/editMessageText", self.bot_token);
        self.client.post(&url).json(&edit).send().await?;
        Ok(())
    }
}

fn build_alert_text(repo: &str, path: &str, secrets: &[String], snippet: &str, link: &str, onchain: &str) -> String {
    let secrets_list = secrets
        .iter()
        .map(|s| format!("• {}", s))
        .collect::<Vec<_>>()
        .join("\n");

    let snippet_preview = &snippet[..snippet.len().min(300)];
    format!(
        "🚨 *Secret Detected*\n\nRepo: `{}`\nFile: `{}`\n\nSecrets Found:\n{}\n\n{}\n\nSnippet:\n```\n{}\n```\n\n[View File]({})",
        repo, path, secrets_list, onchain, snippet_preview, link
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_alert_contains_onchain_field() {
        let text = build_alert_text(
            "owner/repo", "src/main.rs",
            &["Solana private key".to_string()],
            "let key = [1,2,3];",
            "https://github.com/owner/repo",
            "🔍 On-chain: _sedang divalidasi..._",
        );
        assert!(text.contains("On-chain"));
        assert!(text.contains("owner/repo"));
    }
}
