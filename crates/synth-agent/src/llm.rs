//! A provider-agnostic chat-with-tools interface.
//!
//! Nothing here mentions Anthropic. The agent loop is written against
//! [`LlmClient`] alone, so a different backend (a local model, a mock for
//! tests) only has to implement this one trait. The concrete Claude client
//! lives in [`crate::claude`].

use async_trait::async_trait;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

/// One piece of a message. A single assistant turn may contain explanatory text
/// alongside one or more tool calls.
#[derive(Debug, Clone)]
pub enum Content {
    Text(String),
    /// The model asked to run a tool.
    ToolUse { id: String, name: String, input: Value },
    /// Our reply to a prior tool call, fed back on the next turn.
    ToolResult { tool_use_id: String, content: String, is_error: bool },
}

#[derive(Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub content: Vec<Content>,
}

impl Message {
    pub fn user_text(text: impl Into<String>) -> Self {
        Self { role: Role::User, content: vec![Content::Text(text.into())] }
    }
}

/// A tool the model may call. `input_schema` is a JSON Schema object.
#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub system: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDef>,
    pub max_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    /// The model produced a final answer.
    EndTurn,
    /// The model wants one or more tools run before it continues.
    ToolUse,
    /// Output was cut off at `max_tokens`.
    MaxTokens,
    Other(String),
}

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: Vec<Content>,
    pub stop_reason: StopReason,
}

impl LlmResponse {
    /// Concatenate all text blocks (ignoring tool calls).
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|c| match c {
                Content::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }
}

/// A chat model that can call tools.
#[async_trait]
pub trait LlmClient {
    async fn complete(&self, request: &LlmRequest) -> anyhow::Result<LlmResponse>;
}
