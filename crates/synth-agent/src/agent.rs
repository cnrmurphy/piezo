//! The conversational agent. Holds the running patch and chat history, runs the
//! tool-use loop against any [`LlmClient`], and applies the model's edits to the
//! synth parameters.

use anyhow::{anyhow, Result};
use synth_core::params::SynthParams;

use crate::llm::{Content, LlmClient, LlmRequest, Message, Role, StopReason};
use crate::tools::{apply_tool_call, system_prompt, tool_defs, ParamChange};

/// Safety bound so a misbehaving model can't loop forever calling tools.
const MAX_TOOL_ROUNDS: usize = 6;
const MAX_TOKENS: u32 = 4096;

/// The result of one user turn.
#[derive(Debug, Clone)]
pub struct AgentTurn {
    /// The model's natural-language reply.
    pub reply: String,
    /// Every parameter the model changed this turn, in order.
    pub changes: Vec<ParamChange>,
}

pub struct Agent<C: LlmClient> {
    client: C,
    system: String,
    history: Vec<Message>,
    params: SynthParams,
}

impl<C: LlmClient> Agent<C> {
    pub fn new(client: C) -> Self {
        Self {
            client,
            system: system_prompt(),
            history: Vec::new(),
            params: SynthParams::default(),
        }
    }

    /// Start from an existing patch instead of the default.
    pub fn with_params(mut self, params: SynthParams) -> Self {
        self.params = params;
        self
    }

    /// The current patch, reflecting every edit the agent has made.
    pub fn params(&self) -> &SynthParams {
        &self.params
    }

    /// Replace the working patch — e.g. to sync with knobs the user moved by
    /// hand before the next turn, so relative edits start from the real state.
    pub fn set_params(&mut self, params: SynthParams) {
        self.params = params;
    }

    /// Send a user message and run the tool loop until the model gives a final
    /// answer. Returns its reply and the parameter changes it made.
    pub async fn send(&mut self, user_message: &str) -> Result<AgentTurn> {
        self.history.push(Message::user_text(user_message));
        let tools = tool_defs();
        let mut changes = Vec::new();

        for _ in 0..MAX_TOOL_ROUNDS {
            let request = LlmRequest {
                system: self.system.clone(),
                messages: self.history.clone(),
                tools: tools.clone(),
                max_tokens: MAX_TOKENS,
            };
            let response = self.client.complete(&request).await?;

            // Record the assistant turn verbatim so tool_use ids line up.
            self.history.push(Message { role: Role::Assistant, content: response.content.clone() });

            if response.stop_reason != StopReason::ToolUse {
                return Ok(AgentTurn { reply: response.text(), changes });
            }

            // Execute every requested tool and feed the results back.
            let mut results = Vec::new();
            for block in &response.content {
                if let Content::ToolUse { id, name, input } = block {
                    let (content, is_error, change) = apply_tool_call(&mut self.params, name, input);
                    if let Some(c) = change {
                        changes.push(c);
                    }
                    results.push(Content::ToolResult {
                        tool_use_id: id.clone(),
                        content,
                        is_error,
                    });
                }
            }
            self.history.push(Message { role: Role::User, content: results });
        }

        Err(anyhow!("agent exceeded {MAX_TOOL_ROUNDS} tool rounds without finishing"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{LlmResponse, ToolDef};
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::Mutex;

    /// A scripted client that replays a fixed sequence of responses, so the
    /// agent loop can be tested without any network or API key.
    struct ScriptedClient {
        responses: Mutex<std::collections::VecDeque<LlmResponse>>,
        seen_tools: Mutex<bool>,
    }

    impl ScriptedClient {
        fn new(responses: Vec<LlmResponse>) -> Self {
            Self {
                responses: Mutex::new(responses.into_iter().collect()),
                seen_tools: Mutex::new(false),
            }
        }
    }

    #[async_trait]
    impl LlmClient for ScriptedClient {
        async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
            // The agent must always offer its tools and a non-empty system prompt.
            assert!(!request.tools.is_empty());
            assert!(!request.system.is_empty());
            *self.seen_tools.lock().unwrap() = true;
            Ok(self.responses.lock().unwrap().pop_front().expect("ran out of scripted responses"))
        }
    }

    fn tool_use(id: &str, name: &str, input: serde_json::Value) -> LlmResponse {
        LlmResponse {
            content: vec![Content::ToolUse { id: id.into(), name: name.into(), input }],
            stop_reason: StopReason::ToolUse,
        }
    }

    fn final_text(text: &str) -> LlmResponse {
        LlmResponse { content: vec![Content::Text(text.into())], stop_reason: StopReason::EndTurn }
    }

    #[tokio::test]
    async fn applies_tool_calls_and_returns_reply() {
        let client = ScriptedClient::new(vec![
            tool_use("t1", "set_choice", json!({ "name": "osc1.waveform", "value": "square" })),
            tool_use("t2", "set_parameter", json!({ "name": "osc1.filter.cutoff", "value": 500.0 })),
            final_text("Made it a darker square."),
        ]);
        let mut agent = Agent::new(client);

        let turn = agent.send("make it darker and buzzy").await.unwrap();

        assert_eq!(turn.reply, "Made it a darker square.");
        assert_eq!(turn.changes.len(), 2);
        assert_eq!(agent.params().get_choice("osc1.waveform"), Some("square"));
        assert_eq!(agent.params().get_float("osc1.filter.cutoff"), Some(500.0));
    }

    #[tokio::test]
    async fn out_of_range_value_is_clamped_and_reported() {
        let client = ScriptedClient::new(vec![
            tool_use("t1", "set_parameter", json!({ "name": "master.volume", "value": 5.0 })),
            final_text("Turned it up to the max."),
        ]);
        let mut agent = Agent::new(client);

        let turn = agent.send("way louder").await.unwrap();

        assert_eq!(agent.params().get_float("master.volume"), Some(1.0));
        assert_eq!(turn.changes[0].outcome, "clamped to 1");
    }

    #[tokio::test]
    async fn reads_current_patch_then_makes_a_relative_edit() {
        // "brighter": the model first inspects the patch, then lowers nothing /
        // raises cutoff relative to what it saw. The read returns no change.
        let client = ScriptedClient::new(vec![
            tool_use("t1", "get_current_patch", json!({})),
            tool_use("t2", "set_parameter", json!({ "name": "osc1.filter.cutoff", "value": 12000.0 })),
            final_text("Opened the filter up for a brighter tone."),
        ]);
        let mut agent = Agent::new(client);

        let turn = agent.send("a bit brighter").await.unwrap();

        // Only the cutoff edit counts as a change; the read does not.
        assert_eq!(turn.changes.len(), 1);
        assert_eq!(turn.changes[0].name, "osc1.filter.cutoff");
        assert_eq!(agent.params().get_float("osc1.filter.cutoff"), Some(12000.0));
    }

    #[tokio::test]
    async fn errors_surface_to_the_model_without_crashing() {
        // First the model picks a bad name; we report the error; it recovers.
        let client = ScriptedClient::new(vec![
            tool_use("t1", "set_parameter", json!({ "name": "osc1.bogus", "value": 1.0 })),
            final_text("Sorry, I used a wrong name."),
        ]);
        let mut agent = Agent::new(client);

        let turn = agent.send("do something").await.unwrap();
        assert!(turn.changes.is_empty());
        assert_eq!(turn.reply, "Sorry, I used a wrong name.");
    }

    // Keep ToolDef importable for the doc of intent.
    #[allow(dead_code)]
    fn _assert_tooldef_type(_: ToolDef) {}
}
