//! A single synth voice: two oscillators, each through its own filter, summed
//! and shaped by the amplitude envelope. The filter envelope and the (global)
//! LFO modulate as routed.

use crate::env::Adsr;
use crate::filter::StateVariableFilter;
use crate::lfo::LfoTarget;
use crate::osc::Oscillator;
use crate::params::SynthParams;

/// Convert a MIDI note number to frequency in Hz (A4 = 69 = 440 Hz).
pub fn note_to_freq(note: f32) -> f32 {
    440.0 * 2.0f32.powf((note - 69.0) / 12.0)
}

#[derive(Debug, Clone)]
pub struct Voice {
    oscs: [Oscillator; 2],
    filters: [StateVariableFilter; 2],
    amp_env: Adsr,
    filter_env: Adsr,
    note: u8,
    velocity: f32,
}

impl Voice {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            oscs: [Oscillator::new(sample_rate), Oscillator::new(sample_rate)],
            filters: [
                StateVariableFilter::new(sample_rate),
                StateVariableFilter::new(sample_rate),
            ],
            amp_env: Adsr::new(sample_rate),
            filter_env: Adsr::new(sample_rate),
            note: 0,
            velocity: 0.0,
        }
    }

    pub fn is_active(&self) -> bool {
        !self.amp_env.is_idle()
    }

    pub fn note(&self) -> u8 {
        self.note
    }

    pub fn trigger(&mut self, note: u8, velocity: f32) {
        self.note = note;
        self.velocity = velocity;
        self.oscs.iter_mut().for_each(Oscillator::reset);
        self.filters.iter_mut().for_each(StateVariableFilter::reset);
        self.amp_env.trigger();
        self.filter_env.trigger();
    }

    pub fn release(&mut self) {
        self.amp_env.release();
        self.filter_env.release();
    }

    /// Render one sample. `lfo` is the shared LFO output in `[-1, 1]`.
    pub fn next_sample(&mut self, p: &SynthParams, lfo: f32) -> f32 {
        if !self.is_active() {
            return 0.0;
        }

        let lfo_amt = p.lfo.depth * lfo;
        let pitch_mod = if p.lfo.target == LfoTarget::Pitch { lfo_amt * 2.0 } else { 0.0 };
        let filter_lfo_oct = if p.lfo.target == LfoTarget::Filter { lfo_amt * 2.0 } else { 0.0 };
        let amp_trem = if p.lfo.target == LfoTarget::Amp { 1.0 + lfo_amt * 0.5 } else { 1.0 };

        let env_cutoff_oct = p.filter_env.amount * self.filter_env.next_sample(&p.filter_env.env) * 4.0;

        let base = self.note as f32;
        let mut mixed = 0.0;
        for i in 0..2 {
            let osc = p.osc[i];
            let semis = base + osc.tune + osc.fine / 100.0 + pitch_mod;
            let freq = note_to_freq(semis);
            let raw = self.oscs[i].next_sample(freq, osc.waveform) * osc.level;

            let fp = p.filter[i];
            let cutoff = fp.cutoff * 2.0f32.powf(env_cutoff_oct + filter_lfo_oct);
            mixed += self.filters[i].process(raw, cutoff, fp.resonance, fp.mode);
        }

        let amp = self.amp_env.next_sample(&p.amp_env);
        mixed * amp * self.velocity * amp_trem
    }
}
