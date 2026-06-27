//! Bridges the synth's parameter store to the LLM's tool interface.
//!
//! The available tools and the system prompt are both generated from
//! `synth_core`'s parameter tables, so adding a parameter to the synth makes it
//! immediately controllable by the agent with no extra wiring.

use std::fmt::Write as _;

use serde_json::{json, Value};
use synth_core::params::{choice_params, float_params, SetResult, SynthParams};

use crate::llm::ToolDef;

pub const SET_PARAMETER: &str = "set_parameter";
pub const SET_CHOICE: &str = "set_choice";
pub const GET_CURRENT_PATCH: &str = "get_current_patch";

/// The tools the agent may call to shape the sound.
pub fn tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: GET_CURRENT_PATCH.to_string(),
            description: "Read the synth's current parameter values. Call this \
                before making a relative change (e.g. \"brighter\", \"less \
                resonance\") so you adjust from the actual current settings."
                .to_string(),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        ToolDef {
            name: SET_PARAMETER.to_string(),
            description: "Set a numeric synth parameter to a value. The value is \
                clamped to the parameter's valid range."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Parameter name, e.g. osc1.filter.cutoff" },
                    "value": { "type": "number", "description": "New value" }
                },
                "required": ["name", "value"]
            }),
        },
        ToolDef {
            name: SET_CHOICE.to_string(),
            description: "Set a multiple-choice synth parameter (waveform, filter \
                mode, or LFO target) to one of its allowed options."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Parameter name, e.g. osc1.waveform" },
                    "value": { "type": "string", "description": "One of the allowed options" }
                },
                "required": ["name", "value"]
            }),
        },
    ]
}

/// Build a system prompt that documents every parameter, so the model knows the
/// exact names, ranges, and options to use with the tools.
pub fn system_prompt() -> String {
    let mut s = String::from(
        "You are the sound designer for a two-oscillator subtractive synthesizer. \
         The user describes a sound in plain English; you shape it by calling the \
         set_parameter and set_choice tools. For a relative request like \"brighter\" \
         or \"less resonance\", first call get_current_patch so you adjust from the \
         actual current values. Make several coordinated edits when a request calls \
         for it, then briefly explain in one or two sentences what you changed and \
         why. Signal path per voice: two oscillators, each through its own filter, \
         summed, then an amplitude envelope; a filter envelope and an LFO add \
         movement.\n\nNumeric parameters (name: range unit):\n",
    );
    for p in float_params() {
        let unit = if p.unit.is_empty() { String::new() } else { format!(" {}", p.unit) };
        let _ = writeln!(s, "- {}: {} to {}{}", p.name, p.min, p.max, unit);
    }
    s.push_str("\nChoice parameters (name: options):\n");
    for p in choice_params() {
        let _ = writeln!(s, "- {}: {}", p.name, p.options.join(", "));
    }
    s
}

/// Render the current patch as a compact "name: value" list, so the agent can
/// see the actual settings before making a relative change.
pub fn format_patch(params: &SynthParams) -> String {
    let mut s = String::from("Current patch:\n");
    for p in float_params() {
        if let Some(v) = params.get_float(p.name) {
            let _ = writeln!(s, "- {}: {}", p.name, v);
        }
    }
    for p in choice_params() {
        if let Some(v) = params.get_choice(p.name) {
            let _ = writeln!(s, "- {}: {}", p.name, v);
        }
    }
    s
}

/// One edit the agent applied, for reporting back to the user/UI.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamChange {
    pub name: String,
    pub outcome: String,
}

/// Apply a single tool call to `params`. Returns the text to feed back to the
/// model as the tool result, whether it was an error, and the change record (if
/// the edit succeeded).
pub fn apply_tool_call(
    params: &mut SynthParams,
    tool_name: &str,
    input: &Value,
) -> (String, bool, Option<ParamChange>) {
    if tool_name == GET_CURRENT_PATCH {
        return (format_patch(params), false, None);
    }

    let name = input["name"].as_str().unwrap_or_default().to_string();

    match tool_name {
        SET_PARAMETER => {
            let Some(value) = input["value"].as_f64() else {
                return ("value must be a number".to_string(), true, None);
            };
            describe(params.set_float(&name, value as f32), &name)
        }
        SET_CHOICE => {
            let Some(value) = input["value"].as_str() else {
                return ("value must be a string".to_string(), true, None);
            };
            describe(params.set_choice(&name, value), &name)
        }
        other => (format!("unknown tool: {other}"), true, None),
    }
}

fn describe(result: SetResult, name: &str) -> (String, bool, Option<ParamChange>) {
    match result {
        SetResult::Ok(v) => {
            let outcome = format!("set to {v}");
            (format!("ok: {name} {outcome}"), false, Some(ParamChange { name: name.to_string(), outcome }))
        }
        SetResult::Clamped(v) => {
            let outcome = format!("clamped to {v}");
            (
                format!("ok: {name} out of range, {outcome}"),
                false,
                Some(ParamChange { name: name.to_string(), outcome }),
            )
        }
        SetResult::UnknownParam => (format!("error: unknown parameter '{name}'"), true, None),
        SetResult::InvalidChoice => {
            (format!("error: invalid option for '{name}'"), true, None)
        }
    }
}
