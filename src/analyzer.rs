use once_cell::sync::Lazy;
use regex::Regex;

#[derive(Debug)]
pub struct AnalysisResult {
    pub found: bool,
    pub secrets: Vec<String>,
}

struct Pattern {
    label: &'static str,
    re: Regex,
}

static PATTERNS: Lazy<Vec<Pattern>> = Lazy::new(|| {
    vec![
        Pattern {
            label: "Solana keypair byte array",
            re: Regex::new(r"\[\s*(?:\d{1,3}\s*,\s*){31,63}\d{1,3}\s*\]").unwrap(),
        },
        Pattern {
            label: "Private key assignment",
            re: Regex::new(
                r#"(?i)(?:private_?key|secret_?key|wallet_?private_?key)\s*[=:]\s*['"]?([A-Za-z0-9+/=_-]{32,})['"]?"#,
            )
            .unwrap(),
        },
        Pattern {
            label: "Ethereum private key (hex 64)",
            re: Regex::new(r"(?:0x)?[0-9a-fA-F]{64}").unwrap(),
        },
        Pattern {
            label: "AWS Access Key",
            re: Regex::new(r"AKIA[0-9A-Z]{16}").unwrap(),
        },
        Pattern {
            label: "GitHub token",
            re: Regex::new(r"gh[pousr]_[A-Za-z0-9_]{36,255}").unwrap(),
        },
        Pattern {
            label: "Generic API key in .env",
            re: Regex::new(
                r#"(?i)(?:api_?key|access_?token|auth_?token)\s*=\s*['"]?[A-Za-z0-9+/._-]{20,}['"]?"#,
            )
            .unwrap(),
        },
        Pattern {
            label: "Mnemonic seed phrase (12–24 words)",
            re: Regex::new(r"(?:[a-z]{3,12}\s+){11,23}[a-z]{3,12}").unwrap(),
        },
    ]
});

pub fn analyze(content: &str) -> AnalysisResult {
    let mut secrets = Vec::new();

    for pattern in PATTERNS.iter() {
        if let Some(m) = pattern.re.find(content) {
            let snippet = &m.as_str()[..m.as_str().len().min(60)];
            secrets.push(format!("{}: `{}`...", pattern.label, snippet));
        }
    }

    AnalysisResult {
        found: !secrets.is_empty(),
        secrets,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detects_solana_byte_array() {
        let content = "let key = [1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32];";
        let result = analyze(content);
        assert!(result.found);
        assert!(result.secrets[0].contains("Solana keypair"));
    }

    #[test]
    fn test_detects_private_key_assignment() {
        let content = "PRIVATE_KEY=5KJvsngHeMpm884wtkJNzQGaCErckhHJBGFsvd3VyK5qMZXj3hS";
        let result = analyze(content);
        assert!(result.found);
    }

    #[test]
    fn test_detects_aws_key() {
        let content = "aws_access_key_id = AKIAIOSFODNN7EXAMPLE";
        let result = analyze(content);
        assert!(result.found);
        assert!(result.secrets.iter().any(|s| s.contains("AWS")));
    }

    #[test]
    fn test_clean_content() {
        let content = "fn main() { println!(\"Hello, world!\"); }";
        let result = analyze(content);
        assert!(!result.found);
    }
}
