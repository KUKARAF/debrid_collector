use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const GROQ_BASE_URL: &str = "https://api.groq.com/openai/v1";
pub const MODEL: &str = "moonshotai/kimi-k2";

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

pub async fn chat(api_key: &str, messages: &[Message]) -> Result<String> {
    let client = reqwest::Client::new();
    let body = ChatRequest {
        model: MODEL,
        messages,
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
