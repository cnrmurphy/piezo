//! Low-frequency oscillator for modulation. Produces a bipolar `[-1, 1]` signal
//! that voices apply to a routed destination (pitch, filter cutoff, or amp).

use std::f32::consts::TAU;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LfoTarget {
    Off,
    Pitch,
    Filter,
    Amp,
}

impl LfoTarget {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "off" | "none" => Some(LfoTarget::Off),
            "pitch" => Some(LfoTarget::Pitch),
            "filter" | "cutoff" => Some(LfoTarget::Filter),
            "amp" | "volume" => Some(LfoTarget::Amp),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            LfoTarget::Off => "off",
            LfoTarget::Pitch => "pitch",
            LfoTarget::Filter => "filter",
            LfoTarget::Amp => "amp",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Lfo {
    sample_rate: f32,
    phase: f32,
}

impl Lfo {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            phase: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    /// Advance one sample at `rate` Hz; returns a sine in `[-1, 1]`.
    pub fn next_sample(&mut self, rate: f32) -> f32 {
        let v = (TAU * self.phase).sin();
        self.phase += rate / self.sample_rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        v
    }
}
