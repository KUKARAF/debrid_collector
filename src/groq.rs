use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const GROQ_BASE_URL: &str = "https://api.groq.com/openai/v1";
pub const DEFAULT_MODEL: &str = "qwen/qwen3-32b";
pub const FALLBACK_MODELS: &[&str] = &["openai/gpt-oss-120b", "llama-3.3-70b-versatile"];

#[derive(Serialize, Deserialize, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [Message],
    response_format: ResponseFormat,
}

#[derive(Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: AssistantMessage,
}

#[derive(Deserialize)]
struct AssistantMessage {
    content: String,
}

pub async fn list_models(api_key: &str) -> Result<Vec<String>> {
    #[derive(Deserialize)]
    struct Model {
        id: String,
    }
    #[derive(Deserialize)]
    struct ModelsResponse {
        data: Vec<Model>,
    }

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{GROQ_BASE_URL}/models"))
        .bearer_auth(api_key)
        .send()
        .await
        .context("Groq API request failed")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Groq API returned {status}: {text}");
    }

    let parsed: ModelsResponse = resp.json().await.context("failed to parse models response")?;
    let mut ids: Vec<String> = parsed.data.into_iter().map(|m| m.id).collect();
    ids.sort();
    Ok(ids)
}

/// Qwen3 models emit reasoning tokens that break json_object mode unless thinking is disabled.
fn is_qwen3(model: &str) -> bool {
    model.to_lowercase().contains("qwen3")
}

pub async fn chat(api_key: &str, model: &str, messages: &[Message]) -> Result<String> {
    let client = reqwest::Client::new();

    // For qwen3 models, append /no-think to the last user message to disable
    // chain-of-thought output, which otherwise causes json_validate_failed errors.
    let patched: Vec<Message>;
    let effective_messages: &[Message] = if is_qwen3(model) {
        patched = messages
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, mut m)| {
                if i == messages.len() - 1 && m.role == "user" {
                    m.content.push_str(" /no-think");
                }
                m
            })
            .collect();
        &patched
    } else {
        messages
    };

    let body = ChatRequest {
        model,
        messages: effective_messages,
        response_format: ResponseFormat { kind: "json_object" },
    };

    let resp = client
        .post(format!("{GROQ_BASE_URL}/chat/completions"))
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .context("Groq API request failed")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Groq API returned {status}: {text}");
    }

    let parsed: ChatResponse = resp.json().await.context("failed to parse Groq response")?;
    let content = parsed
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .unwrap_or_default();

    Ok(content)
}

/// Try `model` first, then each fallback in order if the model is not found.
pub async fn chat_with_fallback(
    api_key: &str,
    model: &str,
    messages: &[Message],
) -> Result<String> {
    let candidates = std::iter::once(model).chain(FALLBACK_MODELS.iter().copied());
    let mut last_err = anyhow::anyhow!("no models to try");
    for candidate in candidates {
        match chat(api_key, candidate, messages).await {
            Ok(resp) => {
                if candidate != model {
                    eprintln!("      (used fallback model: {candidate})");
                }
                return Ok(resp);
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("model_not_found")
                    || msg.contains("does not exist")
                    || msg.contains("json_validate_failed")
                {
                    eprintln!("      model {candidate} failed, trying next...");
                    last_err = e;
                } else {
                    return Err(e);
                }
            }
        }
    }
    Err(last_err)
}
