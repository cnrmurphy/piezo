//! An analog-style ADSR envelope. Each stage moves toward its target with a
//! one-pole filter, giving exponential (curved) segments rather than straight
//! lines — decays that drop fast then taper, which sounds far more natural than
//! a linear ramp. Output runs in `[0, 1]`. Used for both amplitude and (routed)
//! filter-cutoff modulation.

/// Curvature of the attack and of the decay/release segments. The attack uses a
/// gentler curve; decay and release are more sharply exponential.
const TARGET_RATIO_A: f32 = 0.3;
const TARGET_RATIO_DR: f32 = 0.0001;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// ADSR times are in seconds; `sustain` is a level in `[0, 1]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdsrSettings {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

impl Default for AdsrSettings {
    fn default() -> Self {
        Self {
            attack: 0.01,
            decay: 0.1,
            sustain: 0.8,
            release: 0.2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Adsr {
    sample_rate: f32,
    stage: Stage,
    output: f32,
    /// The settings the coefficients below were computed from; recomputed only
    /// when the settings change.
    cached: Option<AdsrSettings>,
    attack_coef: f32,
    attack_base: f32,
    decay_coef: f32,
    decay_base: f32,
    release_coef: f32,
    release_base: f32,
    sustain: f32,
}

/// One-pole coefficient to traverse `rate` samples toward a target offset by
/// `ratio` (the ratio controls how curved the segment is).
fn calc_coef(rate: f32, ratio: f32) -> f32 {
    if rate <= 0.0 {
        0.0
    } else {
        (-((1.0 + ratio) / ratio).ln() / rate).exp()
    }
}

impl Adsr {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            stage: Stage::Idle,
            output: 0.0,
            cached: None,
            attack_coef: 0.0,
            attack_base: 0.0,
            decay_coef: 0.0,
            decay_base: 0.0,
            release_coef: 0.0,
            release_base: 0.0,
            sustain: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.cached = None;
    }

    pub fn trigger(&mut self) {
        self.stage = Stage::Attack;
    }

    pub fn release(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    /// True once the envelope has fully released and gone silent.
    pub fn is_idle(&self) -> bool {
        self.stage == Stage::Idle
    }

    fn update_coefs(&mut self, s: &AdsrSettings) {
        let sr = self.sample_rate;
        self.sustain = s.sustain.clamp(0.0, 1.0);
        self.attack_coef = calc_coef(s.attack * sr, TARGET_RATIO_A);
        self.attack_base = (1.0 + TARGET_RATIO_A) * (1.0 - self.attack_coef);
        self.decay_coef = calc_coef(s.decay * sr, TARGET_RATIO_DR);
        self.decay_base = (self.sustain - TARGET_RATIO_DR) * (1.0 - self.decay_coef);
        self.release_coef = calc_coef(s.release * sr, TARGET_RATIO_DR);
        self.release_base = -TARGET_RATIO_DR * (1.0 - self.release_coef);
        self.cached = Some(*s);
    }

    /// Advance one sample and return the current level.
    pub fn next_sample(&mut self, s: &AdsrSettings) -> f32 {
        if self.cached.as_ref() != Some(s) {
            self.update_coefs(s);
        }
        match self.stage {
            Stage::Idle => {}
            Stage::Attack => {
                self.output = self.attack_base + self.output * self.attack_coef;
                if self.output >= 1.0 {
                    self.output = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.output = self.decay_base + self.output * self.decay_coef;
                if self.output <= self.sustain {
                    self.output = self.sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => {
                self.output = self.sustain;
            }
            Stage::Release => {
                self.output = self.release_base + self.output * self.release_coef;
                if self.output <= 0.0 {
                    self.output = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rises_to_peak_then_releases_to_zero() {
        let sr = 48_000.0;
        let mut env = Adsr::new(sr);
        let s = AdsrSettings {
            attack: 0.01,
            decay: 0.05,
            sustain: 0.5,
            release: 0.02,
        };
        env.trigger();
        let mut peak = 0.0f32;
        for _ in 0..(sr as usize / 5) {
            peak = peak.max(env.next_sample(&s));
        }
        assert!(peak > 0.99, "attack should reach ~1.0, got {peak}");

        // Hold at sustain.
        let held = env.next_sample(&s);
        assert!((held - 0.5).abs() < 0.05, "sustain ~0.5, got {held}");

        env.release();
        for _ in 0..(sr as usize) {
            env.next_sample(&s);
        }
        assert!(env.is_idle(), "should be idle after release");
    }

    #[test]
    fn instant_attack_when_time_is_zero() {
        let mut env = Adsr::new(48_000.0);
        let s = AdsrSettings { attack: 0.0, decay: 0.0, sustain: 1.0, release: 0.1 };
        env.trigger();
        let v = env.next_sample(&s);
        assert!(v > 0.99, "zero attack should jump to full, got {v}");
    }
}
