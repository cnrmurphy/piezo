//! A Freeverb-style stereo algorithmic reverb on the master bus: a bank of
//! parallel feedback comb filters feeding a chain of allpass filters, run as two
//! channels. The right channel's delay lengths are offset by a few samples,
//! which de-correlates the two channels so the tail spreads into a smooth wash
//! instead of a harsh, metallic mono ring.
//!
//! `size` scales the delay-line lengths (a bigger room has longer, more spread
//! reflections), `decay` sets the comb feedback (how long the tail rings), and
//! `mix` blends the wet signal against the dry input.

/// Comb-filter delay lengths in samples at 44.1 kHz (the classic Freeverb
/// tunings). Scaled for the actual sample rate and the `size` parameter.
const COMB_TUNING: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_TUNING: [usize; 4] = [556, 441, 341, 225];

/// Samples the right channel's delay lengths are offset from the left, for
/// stereo de-correlation.
const STEREO_SPREAD: usize = 23;

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

/// One channel's comb + allpass bank.
struct Channel {
    combs: Vec<Comb>,
    allpasses: Vec<Allpass>,
    /// Per-filter delay offset (0 for left, `STEREO_SPREAD` for right).
    offset: usize,
}

impl Channel {
    fn new(sr_factor: f32, offset: usize) -> Self {
        // Capacity accounts for the offset and the largest `size` scaling.
        let cap = |base: usize| {
            (((base + offset) as f32 * sr_factor * SIZE_MAX).ceil() as usize) + 2
        };
        Self {
            combs: COMB_TUNING.iter().map(|&t| Comb::new(cap(t))).collect(),
            allpasses: ALLPASS_TUNING.iter().map(|&t| Allpass::new(cap(t))).collect(),
            offset,
        }
    }

    fn process(&mut self, drive: f32, feedback: f32, sr_factor: f32, size: f32) -> f32 {
        let mut wet = 0.0;
        for (comb, &base) in self.combs.iter_mut().zip(COMB_TUNING.iter()) {
            comb.set_len(scaled_len(base + self.offset, sr_factor, size));
            wet += comb.process(drive, feedback);
        }
        for (allpass, &base) in self.allpasses.iter_mut().zip(ALLPASS_TUNING.iter()) {
            allpass.set_len(scaled_len(base + self.offset, sr_factor, size));
            wet = allpass.process(wet);
        }
        wet
    }
}

pub struct Reverb {
    sr_factor: f32,
    left: Channel,
    right: Channel,
}

impl Reverb {
    pub fn new(sample_rate: f32) -> Self {
        let sr_factor = sample_rate / 44_100.0;
        Self {
            sr_factor,
            left: Channel::new(sr_factor, 0),
            right: Channel::new(sr_factor, STEREO_SPREAD),
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        *self = Reverb::new(sample_rate);
    }

    /// Process one stereo sample. `mix` 0 = fully dry, 1 = fully wet.
    pub fn process(&mut self, in_l: f32, in_r: f32, mix: f32, size: f32, decay: f32) -> (f32, f32) {
        if mix <= 0.0 {
            return (in_l, in_r);
        }
        let feedback = 0.7 + decay.clamp(0.0, 1.0) * 0.28;
        // Both channel banks are driven by the mono sum of the input.
        let drive = (in_l + in_r) * 0.5 * INPUT_GAIN;

        let wet_l = self.left.process(drive, feedback, self.sr_factor, size);
        let wet_r = self.right.process(drive, feedback, self.sr_factor, size);

        let mix = mix.clamp(0.0, 1.0);
        (
            in_l * (1.0 - mix) + wet_l * WET_GAIN * mix,
            in_r * (1.0 - mix) + wet_r * WET_GAIN * mix,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fully_dry_passes_input_through() {
        let mut rv = Reverb::new(48_000.0);
        for (l, r) in [(0.5, -0.5), (-0.3, 0.2), (0.9, 0.0)] {
            assert_eq!(rv.process(l, r, 0.0, 0.5, 0.5), (l, r));
        }
    }

    #[test]
    fn produces_a_decaying_tail_after_the_input_stops() {
        let mut rv = Reverb::new(48_000.0);
        rv.process(1.0, 1.0, 0.8, 0.6, 0.7); // one impulse, then silence
        let mut tail_energy = 0.0f64;
        let mut peak_late = 0.0f32;
        for i in 0..48_000 {
            let (l, r) = rv.process(0.0, 0.0, 0.8, 0.6, 0.7);
            tail_energy += (l as f64) * (l as f64) + (r as f64) * (r as f64);
            if i > 24_000 {
                peak_late = peak_late.max(l.abs()).max(r.abs());
            }
        }
        assert!(tail_energy > 0.0, "reverb produced no tail");
        assert!(peak_late < 0.2, "tail did not decay, late peak = {peak_late}");
    }

    #[test]
    fn channels_decorrelate() {
        // With a mono input, the two channels should still differ thanks to the
        // right-channel delay offset.
        let mut rv = Reverb::new(48_000.0);
        let mut diff = 0.0f64;
        rv.process(1.0, 1.0, 1.0, 0.6, 0.7);
        for _ in 0..20_000 {
            let (l, r) = rv.process(0.0, 0.0, 1.0, 0.6, 0.7);
            diff += (l - r).abs() as f64;
        }
        assert!(diff > 1.0, "channels did not de-correlate, summed |L-R| = {diff}");
    }

    #[test]
    fn stays_finite_at_extreme_settings() {
        let mut rv = Reverb::new(44_100.0);
        for _ in 0..20_000 {
            let (l, r) = rv.process(0.8, -0.8, 1.0, 1.0, 1.0);
            assert!(l.is_finite() && r.is_finite());
        }
    }
}
