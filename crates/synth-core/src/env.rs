//! A linear ADSR envelope generator. Output runs in `[0, 1]`. Used both for the
//! amplitude envelope and (routed) for filter-cutoff modulation.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// ADSR times are in seconds; `sustain` is a level in `[0, 1]`.
#[derive(Debug, Clone, Copy)]
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
    level: f32,
}

impl Adsr {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            stage: Stage::Idle,
            level: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
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

    /// Advance one sample and return the current level.
    pub fn next_sample(&mut self, s: &AdsrSettings) -> f32 {
        match self.stage {
            Stage::Idle => {}
            Stage::Attack => {
                self.level += self.rate(s.attack);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level -= self.rate(s.decay) * (1.0 - s.sustain);
                if self.level <= s.sustain {
                    self.level = s.sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => {
                self.level = s.sustain;
            }
            Stage::Release => {
                self.level -= self.rate(s.release) * (s.sustain.max(0.0001));
                if self.level <= 0.0 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }

    /// Per-sample increment to traverse a full `[0,1]` span in `seconds`.
    fn rate(&self, seconds: f32) -> f32 {
        if seconds <= 0.0 {
            1.0
        } else {
            1.0 / (seconds * self.sample_rate)
        }
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
        for _ in 0..(sr as usize / 10) {
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
}
