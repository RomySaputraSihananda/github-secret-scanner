use reqwest::Client;
use serde::Serialize;

#[derive(Serialize)]
struct TelegramMessage {
    chat_id: String,
    text: String,
    parse_mode: String,
    disable_web_page_preview: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_thread_id: Option<i64>,
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

    pub async fn send(
        &self,
        repo: &str,
        path: &str,
        secrets: &[String],
        snippet: &str,
        link: &str,
    ) -> Result<(), reqwest::Error> {
        let secrets_list = secrets
            .iter()
            .map(|s| format!("• {}", s))
            .collect::<Vec<_>>()
            .join("\n");

        let snippet_preview = &snippet[..snippet.len().min(300)];
        let text = format!(
            "🚨 *Secret Detected*\n\nRepo: `{}`\nFile: `{}`\n\nSecrets Found:\n{}\n\nSnippet:\n```\n{}\n```\n\n[View File]({})",
            repo, path, secrets_list, snippet_preview, link
        );

        let message = TelegramMessage {
            chat_id: self.chat_id.clone(),
            text,
            parse_mode: "Markdown".to_string(),
            disable_web_page_preview: true,
            message_thread_id: self.message_thread_id,
        };

        let url = format!(
            "https://api.telegram.org/bot{}/sendMessage",
            self.bot_token
        );
        self.client.post(&url).json(&message).send().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_format_contains_repo() {
        let secrets = vec!["Solana private key".to_string()];
        let snippet = "let key = [1,2,3];";
        let repo = "owner/repo";
        let path = "src/main.rs";
        let link = "https://github.com/owner/repo/blob/main/src/main.rs";

        let secrets_list = secrets
            .iter()
            .map(|s| format!("• {}", s))
            .collect::<Vec<_>>()
            .join("\n");

        let text = format!(
            "🚨 *Secret Detected*\n\nRepo: `{}`\nFile: `{}`\n\nSecrets Found:\n{}\n\nSnippet:\n```\n{}\n```\n\n[View File]({})",
            repo, path, secrets_list, snippet, link
        );

        assert!(text.contains("owner/repo"));
        assert!(text.contains("Solana private key"));
        assert!(text.contains("https://github.com"));
    }
}
