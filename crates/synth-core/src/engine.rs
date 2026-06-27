//! The playable synth: owns the patch, a pool of voices, and the global LFO.
//! Drive it with `note_on`/`note_off` and pull audio with `render`.

use crate::lfo::Lfo;
use crate::params::SynthParams;
use crate::reverb::Reverb;
use crate::voice::Voice;

const DEFAULT_POLYPHONY: usize = 8;

pub struct Synth {
    sample_rate: f32,
    params: SynthParams,
    voices: Vec<Voice>,
    lfo: Lfo,
    reverb: Reverb,
    /// Round-robin pointer for voice stealing.
    next_voice: usize,
}

impl Synth {
    pub fn new(sample_rate: f32) -> Self {
        Self::with_polyphony(sample_rate, DEFAULT_POLYPHONY)
    }

    pub fn with_polyphony(sample_rate: f32, voices: usize) -> Self {
        Self {
            sample_rate,
            params: SynthParams::default(),
            voices: (0..voices.max(1)).map(|_| Voice::new(sample_rate)).collect(),
            lfo: Lfo::new(sample_rate),
            reverb: Reverb::new(sample_rate),
            next_voice: 0,
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn params(&self) -> &SynthParams {
        &self.params
    }

    pub fn params_mut(&mut self) -> &mut SynthParams {
        &mut self.params
    }

    pub fn set_params(&mut self, params: SynthParams) {
        self.params = params;
    }

    /// Start a note. `velocity` is `[0, 1]`. Prefers a free voice, else steals
    /// the oldest in round-robin order.
    pub fn note_on(&mut self, note: u8, velocity: f32) {
        let idx = self
            .voices
            .iter()
            .position(|v| !v.is_active())
            .unwrap_or_else(|| {
                let i = self.next_voice;
                self.next_voice = (self.next_voice + 1) % self.voices.len();
                i
            });
        self.voices[idx].trigger(note, velocity);
    }

    /// Release every voice currently playing `note`.
    pub fn note_off(&mut self, note: u8) {
        for v in self.voices.iter_mut().filter(|v| v.is_active() && v.note() == note) {
            v.release();
        }
    }

    /// Number of voices currently producing sound.
    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.is_active()).count()
    }

    /// Render a block of mono samples, summing all voices and applying master
    /// volume. A soft clamp keeps stacked voices from hard-clipping.
    pub fn render(&mut self, out: &mut [f32]) {
        for sample in out.iter_mut() {
            let lfo = self.lfo.next_sample(self.params.lfo.rate);
            let mut mix = 0.0;
            for v in &mut self.voices {
                mix += v.next_sample(&self.params, lfo);
            }
            let rv = self.params.reverb;
            let wet = self.reverb.process(mix, rv.mix, rv.size, rv.decay);
            *sample = (wet * self.params.master_volume).clamp(-1.0, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(buf: &[f32]) -> f32 {
        let s: f64 = buf.iter().map(|x| (*x as f64) * (*x as f64)).sum();
        (s / buf.len() as f64).sqrt() as f32
    }

    #[test]
    fn silent_until_note_played() {
        let mut synth = Synth::new(48_000.0);
        let mut buf = vec![0.0; 4_800];
        synth.render(&mut buf);
        assert_eq!(rms(&buf), 0.0);
    }

    #[test]
    fn note_on_produces_sound_note_off_decays_to_silence() {
        let mut synth = Synth::new(48_000.0);
        synth.note_on(69, 1.0); // A4
        let mut buf = vec![0.0; 4_800];
        synth.render(&mut buf);
        assert!(rms(&buf) > 0.01, "note should make sound, rms={}", rms(&buf));
        assert!(synth.active_voices() >= 1);

        synth.note_off(69);
        // Render well past the release time; voice should free itself.
        let mut tail = vec![0.0; 48_000];
        synth.render(&mut tail);
        assert_eq!(synth.active_voices(), 0, "voice should free after release");
    }

    #[test]
    fn output_never_exceeds_unity() {
        let mut synth = Synth::new(48_000.0);
        // Pile on notes to try to force clipping past [-1, 1].
        for n in 60..68 {
            synth.note_on(n, 1.0);
        }
        let mut buf = vec![0.0; 9_600];
        synth.render(&mut buf);
        assert!(buf.iter().all(|s| (-1.0..=1.0).contains(s)));
    }
}
