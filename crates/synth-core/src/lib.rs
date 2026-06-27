//! Synth core: a pure-Rust subtractive synthesizer engine.
//!
//! Signal path per voice: two [`osc`]illators → per-oscillator [`filter`]s →
//! summed → amplitude [`env`]elope. A second envelope and the [`lfo`] modulate
//! the filter/pitch/amp as routed. Everything the synth exposes lives in
//! [`params`], which the UI and the agentic harness drive by name.
//!
//! This crate deliberately has no audio-backend dependency, so it builds and
//! tests anywhere; live output lives in a separate layer.

pub mod engine;
pub mod env;
pub mod filter;
pub mod lfo;
pub mod osc;
pub mod params;
pub mod voice;

pub use engine::Synth;
pub use params::{SetResult, SynthParams};
