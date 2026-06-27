//! A topology-preserving-transform state-variable filter (Zavalishin / Andy
//! Simper form). One structure yields low-, high-, and band-pass outputs and
//! stays stable across the full cutoff/resonance range.

use std::f32::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    LowPass,
    HighPass,
    BandPass,
}

impl FilterMode {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "lp" | "lowpass" | "low_pass" => Some(FilterMode::LowPass),
            "hp" | "highpass" | "high_pass" => Some(FilterMode::HighPass),
            "bp" | "bandpass" | "band_pass" => Some(FilterMode::BandPass),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            FilterMode::LowPass => "lowpass",
            FilterMode::HighPass => "highpass",
            FilterMode::BandPass => "bandpass",
        }
    }
}

#[derive(Debug, Clone)]
pub struct StateVariableFilter {
    sample_rate: f32,
    // Filter state (integrator memories).
    ic1eq: f32,
    ic2eq: f32,
}

impl StateVariableFilter {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            ic1eq: 0.0,
            ic2eq: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    pub fn reset(&mut self) {
        self.ic1eq = 0.0;
        self.ic2eq = 0.0;
    }

    /// Process one sample. `cutoff` in Hz, `resonance` in `[0, 1]` where higher
    /// is more emphasis at the cutoff frequency.
    pub fn process(&mut self, input: f32, cutoff: f32, resonance: f32, mode: FilterMode) -> f32 {
        // Keep cutoff safely below Nyquist to avoid the tan() blowing up.
        let cutoff = cutoff.clamp(20.0, self.sample_rate * 0.45);
        let g = (PI * cutoff / self.sample_rate).tan();
        // Map resonance -> quality factor; q in [0.5, ~20].
        let q = 0.5 + resonance.clamp(0.0, 0.999) * 19.5;
        let k = 1.0 / q;

        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;

        let v3 = input - self.ic2eq;
        let v1 = a1 * self.ic1eq + a2 * v3;
        let v2 = self.ic2eq + a2 * self.ic1eq + a3 * v3;
        self.ic1eq = 2.0 * v1 - self.ic1eq;
        self.ic2eq = 2.0 * v2 - self.ic2eq;

        match mode {
            FilterMode::LowPass => v2,
            FilterMode::HighPass => input - k * v1 - v2,
            FilterMode::BandPass => v1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RMS of the filter output for a sine at `freq`, after settling.
    fn rms_response(filter_cutoff: f32, sig_freq: f32, mode: FilterMode) -> f32 {
        let sr = 48_000.0;
        let mut filter = StateVariableFilter::new(sr);
        let mut phase = 0.0f32;
        let mut sum_sq = 0.0f64;
        let n = 48_000;
        for i in 0..n {
            let x = (phase * std::f32::consts::TAU).sin();
            phase = (phase + sig_freq / sr) % 1.0;
            let y = filter.process(x, filter_cutoff, 0.2, mode);
            // Skip the settling transient.
            if i > n / 2 {
                sum_sq += (y as f64) * (y as f64);
            }
        }
        (sum_sq / (n as f64 / 2.0)).sqrt() as f32
    }

    #[test]
    fn lowpass_attenuates_highs_passes_lows() {
        let low = rms_response(1_000.0, 100.0, FilterMode::LowPass);
        let high = rms_response(1_000.0, 10_000.0, FilterMode::LowPass);
        assert!(low > high * 5.0, "lp should pass lows: low={low}, high={high}");
    }

    #[test]
    fn highpass_attenuates_lows_passes_highs() {
        let low = rms_response(1_000.0, 100.0, FilterMode::HighPass);
        let high = rms_response(1_000.0, 10_000.0, FilterMode::HighPass);
        assert!(high > low * 5.0, "hp should pass highs: low={low}, high={high}");
    }

    #[test]
    fn stays_finite_at_extremes() {
        let mut f = StateVariableFilter::new(48_000.0);
        for _ in 0..10_000 {
            let y = f.process(1.0, 20_000.0, 0.999, FilterMode::LowPass);
            assert!(y.is_finite());
        }
    }
}
