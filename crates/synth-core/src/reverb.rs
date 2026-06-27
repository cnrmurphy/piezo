//! A Freeverb-style algorithmic reverb applied to the master bus: a bank of
//! parallel feedback comb filters feeding a chain of allpass filters.
//!
//! `size` scales the delay-line lengths (a bigger room has longer, more spread
//! reflections), `decay` sets the comb feedback (how long the tail rings), and
//! `mix` blends the wet signal against the dry input.

/// Comb-filter delay lengths in samples at 44.1 kHz (the classic Freeverb
/// tunings). Scaled for the actual sample rate and the `size` parameter.
const COMB_TUNING: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_TUNING: [usize; 4] = [556, 441, 341, 225];

// `size` scales delay lengths within this range of the base tuning.
const SIZE_MIN: f32 = 0.6;
const SIZE_MAX: f32 = 1.4;

const DAMP: f32 = 0.2;
const INPUT_GAIN: f32 = 0.015;
const WET_GAIN: f32 = 3.0;
const ALLPASS_FEEDBACK: f32 = 0.5;

/// A lowpass-feedback comb filter with an adjustable delay length. The buffer is
/// allocated once at the maximum length so `size` changes need no reallocation.
struct Comb {
    buf: Vec<f32>,
    index: usize,
    len: usize,
    filter_store: f32,
}

impl Comb {
    fn new(capacity: usize) -> Self {
        Self { buf: vec![0.0; capacity.max(1)], index: 0, len: capacity.max(1), filter_store: 0.0 }
    }

    fn set_len(&mut self, len: usize) {
        self.len = len.clamp(1, self.buf.len());
        if self.index >= self.len {
            self.index = 0;
        }
    }

    fn process(&mut self, input: f32, feedback: f32) -> f32 {
        let out = self.buf[self.index];
        self.filter_store = out * (1.0 - DAMP) + self.filter_store * DAMP;
        self.buf[self.index] = input + self.filter_store * feedback;
        self.index += 1;
        if self.index >= self.len {
            self.index = 0;
        }
        out
    }
}

/// An allpass filter with an adjustable delay length.
struct Allpass {
    buf: Vec<f32>,
    index: usize,
    len: usize,
}

impl Allpass {
    fn new(capacity: usize) -> Self {
        Self { buf: vec![0.0; capacity.max(1)], index: 0, len: capacity.max(1) }
    }

    fn set_len(&mut self, len: usize) {
        self.len = len.clamp(1, self.buf.len());
        if self.index >= self.len {
            self.index = 0;
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let buffered = self.buf[self.index];
        let out = -input + buffered;
        self.buf[self.index] = input + buffered * ALLPASS_FEEDBACK;
        self.index += 1;
        if self.index >= self.len {
            self.index = 0;
        }
        out
    }
}

/// Delay length in samples for a base tuning, scaled by sample rate and `size`.
fn scaled_len(base: usize, sr_factor: f32, size: f32) -> usize {
    let scale = SIZE_MIN + size.clamp(0.0, 1.0) * (SIZE_MAX - SIZE_MIN);
    ((base as f32 * sr_factor * scale).round() as usize).max(1)
}

pub struct Reverb {
    sr_factor: f32,
    combs: Vec<Comb>,
    allpasses: Vec<Allpass>,
}

impl Reverb {
    pub fn new(sample_rate: f32) -> Self {
        let sr_factor = sample_rate / 44_100.0;
        let cap = |base: usize| ((base as f32 * sr_factor * SIZE_MAX).ceil() as usize) + 2;
        Self {
            sr_factor,
            combs: COMB_TUNING.iter().map(|&t| Comb::new(cap(t))).collect(),
            allpasses: ALLPASS_TUNING.iter().map(|&t| Allpass::new(cap(t))).collect(),
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        *self = Reverb::new(sample_rate);
    }

    /// Process one sample. `mix` 0 = fully dry, 1 = fully wet.
    pub fn process(&mut self, input: f32, mix: f32, size: f32, decay: f32) -> f32 {
        if mix <= 0.0 {
            return input;
        }
        let feedback = 0.7 + decay.clamp(0.0, 1.0) * 0.28;
        let sr_factor = self.sr_factor;

        let mut wet = 0.0;
        let scaled = input * INPUT_GAIN;
        for (comb, &base) in self.combs.iter_mut().zip(COMB_TUNING.iter()) {
            comb.set_len(scaled_len(base, sr_factor, size));
            wet += comb.process(scaled, feedback);
        }
        for (allpass, &base) in self.allpasses.iter_mut().zip(ALLPASS_TUNING.iter()) {
            allpass.set_len(scaled_len(base, sr_factor, size));
            wet = allpass.process(wet);
        }

        let mix = mix.clamp(0.0, 1.0);
        input * (1.0 - mix) + wet * WET_GAIN * mix
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fully_dry_passes_input_through() {
        let mut rv = Reverb::new(48_000.0);
        for x in [0.5, -0.3, 0.9, 0.0] {
            assert_eq!(rv.process(x, 0.0, 0.5, 0.5), x);
        }
    }

    #[test]
    fn produces_a_decaying_tail_after_the_input_stops() {
        let mut rv = Reverb::new(48_000.0);
        // One impulse, then silence.
        rv.process(1.0, 0.8, 0.6, 0.7);
        let mut tail_energy = 0.0f64;
        let mut peak_late = 0.0f32;
        for i in 0..48_000 {
            let y = rv.process(0.0, 0.8, 0.6, 0.7);
            tail_energy += (y as f64) * (y as f64);
            if i > 24_000 {
                peak_late = peak_late.max(y.abs());
            }
        }
        // The tail rings on well after the impulse...
        assert!(tail_energy > 0.0, "reverb produced no tail");
        // ...but has decayed substantially by half a second.
        assert!(peak_late < 0.2, "tail did not decay, late peak = {peak_late}");
    }

    #[test]
    fn stays_finite_at_extreme_settings() {
        let mut rv = Reverb::new(44_100.0);
        for _ in 0..20_000 {
            let y = rv.process(0.8, 1.0, 1.0, 1.0);
            assert!(y.is_finite());
        }
    }
}
