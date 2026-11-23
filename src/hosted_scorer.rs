//! The hosted Scorers: OpenAI text-embedding-3-small (the paper's) and
//! Voyage AI, each called over HTTPS with the user's own key.
//!
//! Keys come only from the environment (`OPENAI_API_KEY`, `VOYAGE_API_KEY`).
//! The key is held in memory, sent only in the `Authorization` header to
//! the provider, and never written anywhere: not to disk, the report, the
//! data file, run-state.json or any message. Error text from the provider is
//! not passed through for a rejected key, and anything else it says is
//! scrubbed of key-shaped strings first.
//!
//! HTTP goes through ureq with rustls on ring, the same pure-Rust stack
//! hf-hub uses for the local Scorer's download.

use crate::embedder::Embedder;
use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use std::time::Duration;

/// Testing only: send the Scorer's requests to this local URL instead of
/// the provider. Only `http://127.0.0.1:…` and `http://localhost:…` are
/// accepted, so a key can never be sent anywhere else through it.
pub const URL_ENV: &str = "SYNTHETIC_CUSTOMERS_SCORER_URL";

/// Texts per request; both providers accept far more (OpenAI 2,048,
/// Voyage 1,000), but small requests keep each one well under the token caps.
const BATCH: usize = 128;
/// Tries per request when the provider is busy or the network drops.
const TRIES: u32 = 4;
/// Longest wait between tries, whatever `Retry-After` asks.
const MAX_WAIT: Duration = Duration::from_secs(60);

/// A hosted embedding provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    OpenAi,
    Voyage,
}

impl Provider {
    /// The environment variable holding the key.
    pub fn key_env(self) -> &'static str {
        match self {
            Provider::OpenAi => "OPENAI_API_KEY",
            Provider::Voyage => "VOYAGE_API_KEY",
        }
    }

    /// The embedding model used.
    pub fn model(self) -> &'static str {
        match self {
            Provider::OpenAi => "text-embedding-3-small",
            Provider::Voyage => "voyage-4",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Provider::OpenAi => "OpenAI",
            Provider::Voyage => "Voyage AI",
        }
    }

    fn scorer(self) -> &'static str {
        match self {
            Provider::OpenAi => "openai",
            Provider::Voyage => "voyage",
        }
    }

    fn url(self) -> &'static str {
        match self {
            Provider::OpenAi => "https://api.openai.com/v1/embeddings",
            Provider::Voyage => "https://api.voyageai.com/v1/embeddings",
        }
    }

    /// The message when the key is missing.
    fn missing_key(self) -> String {
        let env = self.key_env();
        let scorer = self.scorer();
        match self {
            Provider::OpenAi => format!(
                "scorer = \"{scorer}\" needs an OpenAI API key in the {env} environment \
                 variable, and it is not set. Create a key at \
                 https://platform.openai.com/api-keys, then set it in the shell that runs \
                 synthetic-customers (`export {env}=...`). A ChatGPT subscription (Plus, Pro, \
                 Team) is not an OpenAI API key: the API is a separate account billed by use. \
                 Or use scorer = \"local\", which needs no key"
            ),
            Provider::Voyage => format!(
                "scorer = \"{scorer}\" needs a Voyage AI API key in the {env} environment \
                 variable, and it is not set. Create a key at \
                 https://dashboard.voyageai.com/api-keys (the first 200 million tokens are \
                 free), then set it in the shell that runs synthetic-customers \
                 (`export {env}=...`). Or use scorer = \"local\", which needs no key"
            ),
        }
    }
}

/// A hosted Scorer. Holds the key, so it deliberately has no `Debug`.
pub struct HostedEmbedder {
    provider: Provider,
    key: String,
    url: String,
    name: String,
    agent: ureq::Agent,
}

impl HostedEmbedder {
    /// The Scorer with its key from the environment. Fails, without any
    /// network call, when the key is missing or empty.
    pub fn from_env(provider: Provider) -> Result<Self> {
        let key = std::env::var(provider.key_env())
            .ok()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
            .ok_or_else(|| anyhow!(provider.missing_key()))?;
        let url = match std::env::var(URL_ENV).ok().filter(|u| !u.is_empty()) {
            Some(url) => {
                if !(url.starts_with("http://127.0.0.1:") || url.starts_with("http://localhost:")) {
                    bail!("{URL_ENV} is for tests and only accepts a local http:// URL");
                }
                url
            }
            None => provider.url().to_string(),
        };
        Ok(Self::new(provider, key, url))
    }

    fn new(provider: Provider, key: String, url: String) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(120)))
            .http_status_as_error(false)
            .build()
            .into();
        Self {
            name: format!("{} ({})", provider.scorer(), provider.model()),
            provider,
            key,
            url,
            agent,
        }
    }

    /// One request for at most `BATCH` texts, retried when the provider is
    /// busy (429, 5xx) or the connection fails.
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f64>>> {
        let body = serde_json::json!({ "model": self.provider.model(), "input": texts });
        let label = self.provider.label();
        let mut last = String::new();
        let mut wait = Duration::ZERO;
        for attempt in 0..TRIES {
            if attempt > 0 {
                std::thread::sleep(wait.min(MAX_WAIT));
            }
            let backoff = Duration::from_secs(1 << attempt);
            wait = backoff;
            let response = self
                .agent
                .post(&self.url)
                .header("Authorization", &format!("Bearer {}", self.key))
                .send_json(&body);
            let mut response = match response {
                Ok(r) => r,
                Err(e) => {
                    last = format!("cannot reach {label}: {}", self.scrub(&e.to_string()));
                    continue;
                }
            };
            let status = response.status().as_u16();
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok());
            let text = match response.body_mut().read_to_string() {
                Ok(t) => t,
                Err(e) => {
                    last = format!(
                        "cannot read {label}'s answer: {}",
                        self.scrub(&e.to_string())
                    );
                    continue;
                }
            };
            match status {
                200..=299 => return parse(&text, texts.len(), label),
                401 | 403 => bail!(
                    "{label} rejected the key in {} (HTTP {status}): check that it is a valid, \
                     active API key{}",
                    self.provider.key_env(),
                    if self.provider == Provider::OpenAi {
                        ". A ChatGPT subscription is not an OpenAI API key"
                    } else {
                        ""
                    }
                ),
                429 | 500..=599 => {
                    last = if status == 429 {
                        let mut m = format!(
                            "{label} refused the request: rate limit or quota reached (HTTP 429). \
                             Wait a little, or check the account's billing and limits"
                        );
                        if self.provider == Provider::Voyage {
                            m.push_str(
                                "; Voyage AI's limits can be much lower until a payment \
                                 method is added, even with free tokens left",
                            );
                        }
                        m
                    } else {
                        format!("{label} had a server error (HTTP {status})")
                    };
                    wait = retry_after.map_or(backoff, Duration::from_secs);
                }
                _ => bail!(
                    "{label} refused the embeddings request (HTTP {status}){}",
                    self.provider_message(&text)
                ),
            }
        }
        bail!("{last} (gave up after {TRIES} tries)")
    }

    /// The provider's own error message, scrubbed and shortened, for errors
    /// that are not about the key.
    fn provider_message(&self, body: &str) -> String {
        let value: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
        let message = value
            .pointer("/error/message")
            .or_else(|| value.get("detail"))
            .and_then(|m| m.as_str())
            .unwrap_or_default();
        let message: String = self.scrub(message).chars().take(300).collect();
        if message.is_empty() {
            String::new()
        } else {
            format!(": {message}")
        }
    }

    /// Remove the key, and anything shaped like a key, from text.
    fn scrub(&self, text: &str) -> String {
        scrub(text, &self.key)
    }
}

/// Replace `key` and every word that looks like an API key (`sk-…`, `pa-…`,
/// or any word sharing the key's first 8 characters) with `[redacted]`.
fn scrub(text: &str, key: &str) -> String {
    let text = if key.is_empty() {
        text.to_string()
    } else {
        text.replace(key, "[redacted]")
    };
    let prefix: String = key.chars().take(8).collect();
    text.split_inclusive(char::is_whitespace)
        .map(|word| {
            let w = word.trim_start_matches(|c: char| !c.is_alphanumeric());
            if w.starts_with("sk-")
                || w.starts_with("pa-")
                || (prefix.chars().count() == 8 && w.contains(&prefix))
            {
                let ws: String = word.chars().filter(|c| c.is_whitespace()).collect();
                format!("[redacted]{ws}")
            } else {
                word.to_string()
            }
        })
        .collect()
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingItem>,
}

#[derive(Deserialize)]
struct EmbeddingItem {
    index: usize,
    embedding: Vec<f64>,
}

/// The vectors in input order, checking one came back for each input.
fn parse(body: &str, expected: usize, label: &str) -> Result<Vec<Vec<f64>>> {
    let mut response: EmbeddingResponse = serde_json::from_str(body)
        .with_context(|| format!("{label} sent an answer that is not an embeddings list"))?;
    response.data.sort_by_key(|item| item.index);
    let in_order = response
        .data
        .iter()
        .enumerate()
        .all(|(i, item)| item.index == i);
    if response.data.len() != expected || !in_order {
        bail!(
            "{label} sent {} vectors for {expected} texts",
            response.data.len()
        );
    }
    Ok(response.data.into_iter().map(|d| d.embedding).collect())
}

impl Embedder for HostedEmbedder {
    fn name(&self) -> &str {
        &self.name
    }

    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f64>>> {
        let mut out = Vec::with_capacity(texts.len());
        for chunk in texts.chunks(BATCH) {
            out.extend(self.embed_batch(chunk)?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_removes_the_key_and_key_shaped_words() {
        let key = "sk-proj-SECRET1234567890";
        assert_eq!(
            scrub(&format!("Incorrect API key provided: {key}."), key),
            "Incorrect API key provided: [redacted]."
        );
        assert_eq!(
            scrub("Incorrect API key provided: sk-proj-***7890. See docs", key),
            "Incorrect API key provided: [redacted] See docs"
        );
        assert_eq!(scrub("key pa-abc rejected", "x"), "key [redacted] rejected");
        assert_eq!(scrub("all good here", key), "all good here");
    }

    #[test]
    fn parse_orders_vectors_by_index_and_checks_the_count() {
        let body = r#"{"data":[{"index":1,"embedding":[2.0]},{"index":0,"embedding":[1.0]}]}"#;
        assert_eq!(parse(body, 2, "X").unwrap(), vec![vec![1.0], vec![2.0]]);
        assert!(parse(body, 3, "X").is_err());
        let dup = r#"{"data":[{"index":0,"embedding":[2.0]},{"index":0,"embedding":[1.0]}]}"#;
        assert!(parse(dup, 2, "X").is_err());
        assert!(parse("not json", 1, "X").is_err());
    }

    #[test]
    fn the_name_says_provider_and_model() {
        let e = HostedEmbedder::new(Provider::Voyage, "k".into(), "http://127.0.0.1:1".into());
        assert_eq!(e.name(), "voyage (voyage-4)");
        let e = HostedEmbedder::new(Provider::OpenAi, "k".into(), "http://127.0.0.1:1".into());
        assert_eq!(e.name(), "openai (text-embedding-3-small)");
    }
}
