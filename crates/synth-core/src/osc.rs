//! Oscillators. Naive saw/square waveforms alias badly at high pitches, so we
//! apply PolyBLEP (polynomial band-limited step) correction to keep them clean.

use std::f32::consts::TAU;

/// The four basic waveforms the synth can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waveform {
    Sine,
    Saw,
    Square,
    Noise,
}

impl Waveform {
    /// Parse from a lowercase name. Used by the parameter store / agent.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "sine" => Some(Waveform::Sine),
            "saw" => Some(Waveform::Saw),
            "square" => Some(Waveform::Square),
            "noise" | "whitenoise" | "white_noise" => Some(Waveform::Noise),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Waveform::Sine => "sine",
            Waveform::Saw => "saw",
            Waveform::Square => "square",
            Waveform::Noise => "noise",
        }
    }
}

/// A single phase-accumulating oscillator. `phase` runs in `[0, 1)`.
#[derive(Debug, Clone)]
pub struct Oscillator {
    sample_rate: f32,
    phase: f32,
    /// Linear-congruential RNG state for white noise.
    rng: u32,
}

impl Oscillator {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            phase: 0.0,
            rng: 0x1234_5678,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
    }

    /// Advance one sample at `freq` Hz and return the waveform value in `[-1, 1]`.
    pub fn next_sample(&mut self, freq: f32, waveform: Waveform) -> f32 {
        if waveform == Waveform::Noise {
            return self.next_noise();
        }

        let dt = (freq / self.sample_rate).clamp(0.0, 0.5);
        let t = self.phase;

        let value = match waveform {
            Waveform::Sine => (TAU * t).sin(),
            Waveform::Saw => {
                // Naive saw ramps -1..1, corrected at the wrap discontinuity.
                let mut v = 2.0 * t - 1.0;
                v -= poly_blep(t, dt);
                v
            }
            Waveform::Square => {
                let mut v = if t < 0.5 { 1.0 } else { -1.0 };
                // Correct both edges of the square (rising at 0, falling at 0.5).
                v += poly_blep(t, dt);
                v -= poly_blep((t + 0.5) % 1.0, dt);
                v
            }
            Waveform::Noise => unreachable!(),
        };

        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        value
    }

    fn next_noise(&mut self) -> f32 {
        // xorshift32 -> uniform float in [-1, 1].
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// PolyBLEP correction term for a unit step at the phase wrap point.
/// `t` is the current phase in `[0,1)`, `dt` is the per-sample phase increment.
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_stays_in_range_and_is_periodic() {
        let mut osc = Oscillator::new(48_000.0);
        let mut max = f32::MIN;
        for _ in 0..48_000 {
            let v = osc.next_sample(440.0, Waveform::Sine);
            assert!((-1.001..=1.001).contains(&v));
            max = max.max(v);
        }
        // A full second of 440 Hz must reach near the peak.
        assert!(max > 0.99, "sine never approached peak, max={max}");
    }

    #[test]
    fn saw_and_square_bounded() {
        for wf in [Waveform::Saw, Waveform::Square] {
            let mut osc = Oscillator::new(48_000.0);
            for _ in 0..10_000 {
                let v = osc.next_sample(220.0, wf);
                // PolyBLEP can slightly overshoot 1.0; allow a small margin.
                assert!((-1.6..=1.6).contains(&v), "{wf:?} out of range: {v}");
            }
        }
    }

    #[test]
    fn waveform_roundtrips_through_strings() {
        for wf in [Waveform::Sine, Waveform::Saw, Waveform::Square, Waveform::Noise] {
            assert_eq!(Waveform::from_str(wf.as_str()), Some(wf));
        }
        assert_eq!(Waveform::from_str("whitenoise"), Some(Waveform::Noise));
        assert_eq!(Waveform::from_str("triangle"), None);
    }
}
