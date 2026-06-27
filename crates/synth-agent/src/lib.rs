//! Agentic harness for the synth: describe a sound in plain English and the
//! agent shapes the patch by calling tools.
//!
//! The design is provider-agnostic. [`llm::LlmClient`] is the only backend
//! contract; [`claude::ClaudeClient`] is the concrete Anthropic implementation,
//! and [`agent::Agent`] runs the tool-use loop against whatever client it is
//! given (including a mock, for tests). Tools and the system prompt are
//! generated from `synth_core`'s parameter tables.

pub mod agent;
pub mod claude;
pub mod llm;
pub mod tools;

pub use agent::{Agent, AgentTurn};
pub use claude::ClaudeClient;
pub use tools::ParamChange;
