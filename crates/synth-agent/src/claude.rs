//! The Claude (Anthropic) implementation of [`LlmClient`].
//!
//! Rust has no official Anthropic SDK, so this talks to the Messages API over
//! HTTP directly. It translates our neutral [`crate::llm`] types to and from the
//! Anthropic wire format; the rest of the crate never sees these details.

use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::{Content, LlmClient, LlmRequest, LlmResponse, Message, Role, StopReason};

/// Current Anthropic API version and default model. Adaptive thinking is left
/// off for now: with tool use it requires echoing signed thinking blocks back
/// through the loop, which we can add later without changing the trait.
const API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MODEL: &str = "claude-opus-4-8";

pub struct ClaudeClient {
    http: reqwest::Client,
    api_key: String,
    model: String,
}

impl ClaudeClient {
    /// Build a client, reading the key from `ANTHROPIC_API_KEY`.
    pub fn from_env() -> anyhow::Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .context("ANTHROPIC_API_KEY is not set")?;
        Ok(Self::with_key(api_key))
    }

    pub fn with_key(api_key: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key: api_key.into(),
            model: DEFAULT_MODEL.to_string(),
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }
}

#[async_trait]
impl LlmClient for ClaudeClient {
    async fn complete(&self, request: &LlmRequest) -> anyhow::Result<LlmResponse> {
        let body = json!({
            "model": self.model,
            "max_tokens": request.max_tokens,
            "system": request.system,
            "tools": request.tools.iter().map(|t| json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.input_schema,
            })).collect::<Vec<_>>(),
            "messages": request.messages.iter().map(encode_message).collect::<Vec<_>>(),
        });

        let resp = self
            .http
            .post(API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("sending request to Anthropic")?;

        let status = resp.status();
        let payload: Value = resp.json().await.context("decoding Anthropic response")?;
        if !status.is_success() {
            let msg = payload["error"]["message"].as_str().unwrap_or("unknown error");
            return Err(anyhow!("Anthropic API error ({status}): {msg}"));
        }

        decode_response(&payload)
    }
}

fn encode_message(m: &Message) -> Value {
    let role = match m.role {
        Role::User => "user",
        Role::Assistant => "assistant",
    };
    let content: Vec<Value> = m.content.iter().map(encode_content).collect();
    json!({ "role": role, "content": content })
}

fn encode_content(c: &Content) -> Value {
    match c {
        Content::Text(t) => json!({ "type": "text", "text": t }),
        Content::ToolUse { id, name, input } => {
            json!({ "type": "tool_use", "id": id, "name": name, "input": input })
        }
        Content::ToolResult { tool_use_id, content, is_error } => json!({
            "type": "tool_result",
            "tool_use_id": tool_use_id,
            "content": content,
            "is_error": is_error,
        }),
    }
}

fn decode_response(payload: &Value) -> anyhow::Result<LlmResponse> {
    let blocks = payload["content"]
        .as_array()
        .ok_or_else(|| anyhow!("response had no content array"))?;

    let mut content = Vec::new();
    for block in blocks {
        match block["type"].as_str() {
            Some("text") => {
                content.push(Content::Text(block["text"].as_str().unwrap_or("").to_string()));
            }
            Some("tool_use") => content.push(Content::ToolUse {
                id: block["id"].as_str().unwrap_or("").to_string(),
                name: block["name"].as_str().unwrap_or("").to_string(),
                input: block["input"].clone(),
            }),
            // Ignore block types we don't model (e.g. thinking).
            _ => {}
        }
    }

    let stop_reason = match payload["stop_reason"].as_str() {
        Some("end_turn") | Some("stop_sequence") => StopReason::EndTurn,
        Some("tool_use") => StopReason::ToolUse,
        Some("max_tokens") => StopReason::MaxTokens,
        Some(other) => StopReason::Other(other.to_string()),
        None => StopReason::Other("missing".to_string()),
    };

    Ok(LlmResponse { content, stop_reason })
}
