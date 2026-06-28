//! Tauri backend for the synth desktop app.
//!
//! Holds the canonical patch, a `Send` controller for the audio engine, and the
//! agent. The frontend builds its knobs from `get_schema`, drives the synth by
//! hand through `set_param`/`set_choice`, plays notes via `note_on`/`note_off`,
//! and talks to the agent via `chat`. Manual edits and agent edits operate on
//! the same patch, so they stay consistent; after any change the new patch is
//! pushed to the audio engine and broadcast to the UI so the knobs follow.

use std::sync::Mutex;

use serde_json::{json, Map, Value};
use synth_agent::{Agent, ClaudeClient};
use synth_audio::{AudioController, AudioHandle};
use synth_core::params::{choice_params, float_params, SynthParams};
use synth_core::sequencer::Sequencer;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::sleep;

/// Delay between applying each agent change, so they sweep in rather than all
/// snapping at once.
const SWEEP_STEP: Duration = Duration::from_millis(90);

struct AppState {
    audio: AudioController,
    /// The single source of truth for the current patch.
    patch: Mutex<SynthParams>,
    /// `None` when `ANTHROPIC_API_KEY` is unset — the UI still works, but chat
    /// reports that the agent is unavailable.
    agent: AsyncMutex<Option<Agent<ClaudeClient>>>,
    /// UI-side mirror of the sequencer. The authoritative sequencer runs on the
    /// audio thread; this copy is never ticked, it just tracks the configuration
    /// so the UI can be served its current state.
    seq: Mutex<Sequencer>,
}

/// The parameter catalog (names, ranges, options), so the UI can build controls
/// generically. Generated from the same tables the agent uses.
#[tauri::command]
fn get_schema() -> Value {
    let floats: Vec<Value> = float_params()
        .iter()
        .map(|p| json!({
            "name": p.name, "label": p.label, "group": group_of(p.name),
            "min": p.min, "max": p.max, "default": p.default, "unit": p.unit,
        }))
        .collect();
    let choices: Vec<Value> = choice_params()
        .iter()
        .map(|p| json!({
            "name": p.name, "label": p.label, "group": group_of(p.name),
            "options": p.options,
        }))
        .collect();
    json!({ "floats": floats, "choices": choices })
}

#[tauri::command]
fn get_params(state: State<'_, AppState>) -> Value {
    params_json(&state.patch.lock().unwrap())
}

#[tauri::command]
fn set_param(state: State<'_, AppState>, name: String, value: f64) -> Value {
    let new = {
        let mut patch = state.patch.lock().unwrap();
        patch.set_float(&name, value as f32);
        *patch
    };
    state.audio.set_params(new);
    params_json(&new)
}

#[tauri::command]
fn set_choice(state: State<'_, AppState>, name: String, value: String) -> Value {
    let new = {
        let mut patch = state.patch.lock().unwrap();
        patch.set_choice(&name, &value);
        *patch
    };
    state.audio.set_params(new);
    params_json(&new)
}

#[tauri::command]
fn note_on(state: State<'_, AppState>, note: u8, velocity: f32) {
    state.audio.note_on(note, velocity);
}

#[tauri::command]
fn note_off(state: State<'_, AppState>, note: u8) {
    state.audio.note_off(note);
}

/// The sequencer's current configuration, for building its UI.
#[tauri::command]
fn seq_state(state: State<'_, AppState>) -> Value {
    seq_json(&state.seq.lock().unwrap())
}

#[tauri::command]
fn seq_set_running(state: State<'_, AppState>, running: bool) {
    state.seq.lock().unwrap().set_running(running);
    state.audio.seq_set_running(running);
}

#[tauri::command]
fn seq_set_tempo(state: State<'_, AppState>, bpm: f32) {
    state.seq.lock().unwrap().set_tempo(bpm);
    state.audio.seq_set_tempo(bpm);
}

#[tauri::command]
fn seq_set_length(state: State<'_, AppState>, length: usize) {
    state.seq.lock().unwrap().set_length(length);
    state.audio.seq_set_length(length);
}

#[tauri::command]
fn seq_set_step(state: State<'_, AppState>, index: usize, active: bool, note: u8) {
    state.seq.lock().unwrap().set_step(index, active, note);
    state.audio.seq_set_step(index, active, note);
}

fn seq_json(seq: &Sequencer) -> Value {
    let steps: Vec<Value> = seq
        .steps()
        .iter()
        .map(|s| json!({ "active": s.active, "note": s.note }))
        .collect();
    json!({
        "running": seq.is_running(),
        "bpm": seq.bpm(),
        "length": seq.length(),
        "steps": steps,
    })
}

/// Run one agent turn. Syncs the agent with the current patch first (so it sees
/// hand-tweaked knobs), then applies its edits and broadcasts the new patch.
#[tauri::command]
async fn chat(state: State<'_, AppState>, app: AppHandle, message: String) -> Result<Value, String> {
    let mut guard = state.agent.lock().await;
    let agent = guard
        .as_mut()
        .ok_or("ANTHROPIC_API_KEY is not set, so the agent is unavailable")?;

    let current = *state.patch.lock().unwrap();
    agent.set_params(current);

    let turn = agent.send(&message).await.map_err(|e| e.to_string())?;
    let new = *agent.params();

    // Stream the edits in one at a time rather than snapping the whole patch at
    // once: start from the pre-turn patch and step each changed parameter to its
    // final value, pushing to audio and broadcasting to the UI as we go.
    let mut running = current;
    for change in &turn.changes {
        copy_param(&mut running, &new, &change.name);
        state.audio.set_params(running);
        let _ = app.emit("patch-changed", params_json(&running));
        sleep(SWEEP_STEP).await;
    }

    // Land exactly on the agent's final patch (covers any subtlety the per-name
    // replay might miss) and record it as the canonical state.
    *state.patch.lock().unwrap() = new;
    state.audio.set_params(new);

    let changes: Vec<Value> = turn
        .changes
        .iter()
        .map(|c| json!({ "name": c.name, "outcome": c.outcome }))
        .collect();
    Ok(json!({ "reply": turn.reply, "changes": changes, "params": params_json(&new) }))
}

/// Copy a single named parameter's value from `src` into `dst`, whether it is a
/// numeric or a choice parameter.
fn copy_param(dst: &mut SynthParams, src: &SynthParams, name: &str) {
    if let Some(v) = src.get_float(name) {
        dst.set_float(name, v);
    } else if let Some(v) = src.get_choice(name) {
        dst.set_choice(name, v);
    }
}

/// Group a parameter name by its first dotted segment (osc1, amp_env, lfo, ...).
fn group_of(name: &str) -> &str {
    name.split('.').next().unwrap_or(name)
}

/// Current patch values as `{ floats: {name: value}, choices: {name: value} }`.
fn params_json(p: &SynthParams) -> Value {
    let mut floats = Map::new();
    for fp in float_params() {
        if let Some(v) = p.get_float(fp.name) {
            floats.insert(fp.name.to_string(), json!(v));
        }
    }
    let mut choices = Map::new();
    for cp in choice_params() {
        if let Some(v) = p.get_choice(cp.name) {
            choices.insert(cp.name.to_string(), json!(v));
        }
    }
    json!({ "floats": floats, "choices": choices })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // The cpal stream is `!Send`, so it must live on its own thread. Start audio
    // there, hand back a `Send` controller, and park the thread to keep the
    // stream alive for the life of the app.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || match AudioHandle::start() {
        Ok(audio) => {
            let _ = tx.send(Some(audio.controller()));
            loop {
                std::thread::park();
            }
        }
        Err(err) => {
            eprintln!("audio init failed: {err}");
            let _ = tx.send(None);
        }
    });
    let audio = rx
        .recv()
        .ok()
        .flatten()
        .expect("audio output device required");

    let initial = SynthParams::default();
    audio.set_params(initial);

    let agent = ClaudeClient::from_env().ok().map(|client| {
        let mut a = Agent::new(client);
        a.set_params(initial);
        a
    });
    if agent.is_none() {
        eprintln!("note: ANTHROPIC_API_KEY not set — running without the agent");
    }

    let seq = Sequencer::new(audio.sample_rate());

    let state = AppState {
        audio,
        patch: Mutex::new(initial),
        agent: AsyncMutex::new(agent),
        seq: Mutex::new(seq),
    };

    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            get_schema, get_params, set_param, set_choice, note_on, note_off, chat,
            seq_state, seq_set_running, seq_set_tempo, seq_set_length, seq_set_step
        ])
        .setup(|app| {
            // Make sure the window is shown on launch.
            let _ = app.get_webview_window("main");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the synth app");
}
