use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const BASE_URL: &str = "https://openrouter.ai/api/v1";
pub const DEFAULT_MODEL: &str = "z-ai/glm-5.1";
pub const CLASSIFIER_MODEL: &str = "z-ai/glm-4.5-air";
pub const FALLBACK_MODELS: &[&str] = &["qwen/qwen3-32b", "meta-llama/llama-3.3-70b-instruct"];

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
        .get(format!("{BASE_URL}/models"))
        .bearer_auth(api_key)
        .send()
        .await
        .context("OpenRouter API request failed")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("OpenRouter API returned {status}: {text}");
    }

    let parsed: ModelsResponse = resp.json().await.context("failed to parse models response")?;
    let mut ids: Vec<String> = parsed.data.into_iter().map(|m| m.id).collect();
    ids.sort();
    Ok(ids)
}

/// Plain-text chat — no JSON response_format. Used for yes/no classification.
pub async fn chat_text(api_key: &str, model: &str, messages: &[Message]) -> Result<String> {
    #[derive(Serialize)]
    struct PlainRequest<'a> {
        model: &'a str,
        messages: &'a [Message],
    }

    let client = reqwest::Client::new();
    let body = PlainRequest { model, messages };

    let resp = client
        .post(format!("{BASE_URL}/chat/completions"))
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .context("OpenRouter API request failed")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("OpenRouter API returned {status}: {text}");
    }

    let parsed: ChatResponse = resp.json().await.context("failed to parse OpenRouter response")?;
    Ok(parsed.choices.into_iter().next().map(|c| c.message.content).unwrap_or_default())
}

async fn chat(api_key: &str, model: &str, messages: &[Message]) -> Result<String> {
    let client = reqwest::Client::new();

    let body = ChatRequest {
        model,
        messages,
        response_format: ResponseFormat { kind: "json_object" },
    };

    let resp = client
        .post(format!("{BASE_URL}/chat/completions"))
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .context("OpenRouter API request failed")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("OpenRouter API returned {status}: {text}");
    }

    let parsed: ChatResponse = resp.json().await.context("failed to parse OpenRouter response")?;
    let content = parsed
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .unwrap_or_default();

    Ok(content)
}

/// Try `model` first, then each fallback in order if the model fails.
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
