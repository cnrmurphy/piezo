//! The patch: every knob the synth exposes, plus a flat, named accessor layer.
//!
//! Both the UI (knobs/sliders) and the agentic harness operate purely through
//! the descriptor tables and the string-named get/set methods here, so there is
//! exactly one source of truth for parameter names, ranges, and defaults.

use crate::env::AdsrSettings;
use crate::filter::FilterMode;
use crate::lfo::LfoTarget;
use crate::osc::Waveform;

#[derive(Debug, Clone, Copy)]
pub struct OscParams {
    pub waveform: Waveform,
    /// Coarse tuning in semitones relative to the played note.
    pub tune: f32,
    /// Fine tuning in cents.
    pub fine: f32,
    /// Output level `[0, 1]`.
    pub level: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct FilterParams {
    pub cutoff: f32,
    pub resonance: f32,
    pub mode: FilterMode,
}

#[derive(Debug, Clone, Copy)]
pub struct FilterEnvParams {
    pub env: AdsrSettings,
    /// How much the filter envelope pushes cutoff, in `[-1, 1]` (bipolar).
    pub amount: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct LfoParams {
    pub rate: f32,
    pub depth: f32,
    pub target: LfoTarget,
}

/// Master-bus reverb. `mix` blends wet against dry, `size` scales the room, and
/// `decay` sets how long the tail rings. All in `[0, 1]`.
#[derive(Debug, Clone, Copy)]
pub struct ReverbParams {
    pub mix: f32,
    pub size: f32,
    pub decay: f32,
}

/// The full synth patch. `Clone` is cheap (plain data), which lets the audio
/// thread take a snapshot without locking for the duration of a render block.
#[derive(Debug, Clone, Copy)]
pub struct SynthParams {
    pub osc: [OscParams; 2],
    pub filter: [FilterParams; 2],
    pub amp_env: AdsrSettings,
    pub filter_env: FilterEnvParams,
    pub lfo: LfoParams,
    pub reverb: ReverbParams,
    pub master_volume: f32,
}

impl Default for SynthParams {
    fn default() -> Self {
        let osc = OscParams {
            waveform: Waveform::Saw,
            tune: 0.0,
            fine: 0.0,
            level: 0.5,
        };
        let filter = FilterParams {
            cutoff: 8_000.0,
            resonance: 0.1,
            mode: FilterMode::LowPass,
        };
        Self {
            osc: [osc, OscParams { level: 0.0, ..osc }],
            filter: [filter, filter],
            amp_env: AdsrSettings::default(),
            filter_env: FilterEnvParams {
                env: AdsrSettings::default(),
                amount: 0.0,
            },
            lfo: LfoParams {
                rate: 5.0,
                depth: 0.0,
                target: LfoTarget::Off,
            },
            reverb: ReverbParams {
                mix: 0.0,
                size: 0.5,
                decay: 0.5,
            },
            master_volume: 0.8,
        }
    }
}

/// A numeric parameter: continuous value with a range, used to build sliders and
/// to validate/clamp agent edits.
pub struct FloatParam {
    pub name: &'static str,
    pub label: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub unit: &'static str,
    get: fn(&SynthParams) -> f32,
    set: fn(&mut SynthParams, f32),
}

/// An enumerated parameter (waveform, filter mode, LFO target).
pub struct ChoiceParam {
    pub name: &'static str,
    pub label: &'static str,
    pub options: &'static [&'static str],
    get: fn(&SynthParams) -> &'static str,
    set: fn(&mut SynthParams, &str) -> bool,
}

const WAVEFORMS: &[&str] = &["sine", "saw", "square", "noise"];
const FILTER_MODES: &[&str] = &["lowpass", "highpass", "bandpass"];
const LFO_TARGETS: &[&str] = &["off", "pitch", "filter", "amp"];

/// All continuous parameters. Non-capturing closures coerce to `fn` pointers, so
/// each row is a self-contained get/set pair with no name duplication elsewhere.
#[rustfmt::skip]
pub fn float_params() -> Vec<FloatParam> {
    fn fp(name: &'static str, label: &'static str, min: f32, max: f32, default: f32,
          unit: &'static str, get: fn(&SynthParams) -> f32, set: fn(&mut SynthParams, f32))
          -> FloatParam {
        FloatParam { name, label, min, max, default, unit, get, set }
    }
    vec![
        fp("osc1.tune",  "Osc 1 Tune",  -24.0, 24.0, 0.0, "st",  |p| p.osc[0].tune,  |p, v| p.osc[0].tune = v),
        fp("osc1.fine",  "Osc 1 Fine",  -100.0, 100.0, 0.0, "ct", |p| p.osc[0].fine,  |p, v| p.osc[0].fine = v),
        fp("osc1.level", "Osc 1 Level", 0.0, 1.0, 0.5, "",        |p| p.osc[0].level, |p, v| p.osc[0].level = v),
        fp("osc2.tune",  "Osc 2 Tune",  -24.0, 24.0, 0.0, "st",  |p| p.osc[1].tune,  |p, v| p.osc[1].tune = v),
        fp("osc2.fine",  "Osc 2 Fine",  -100.0, 100.0, 0.0, "ct", |p| p.osc[1].fine,  |p, v| p.osc[1].fine = v),
        fp("osc2.level", "Osc 2 Level", 0.0, 1.0, 0.0, "",        |p| p.osc[1].level, |p, v| p.osc[1].level = v),

        fp("osc1.filter.cutoff",    "Osc 1 Cutoff",    20.0, 20_000.0, 8_000.0, "Hz", |p| p.filter[0].cutoff,    |p, v| p.filter[0].cutoff = v),
        fp("osc1.filter.resonance", "Osc 1 Resonance", 0.0, 1.0, 0.1, "",            |p| p.filter[0].resonance, |p, v| p.filter[0].resonance = v),
        fp("osc2.filter.cutoff",    "Osc 2 Cutoff",    20.0, 20_000.0, 8_000.0, "Hz", |p| p.filter[1].cutoff,    |p, v| p.filter[1].cutoff = v),
        fp("osc2.filter.resonance", "Osc 2 Resonance", 0.0, 1.0, 0.1, "",            |p| p.filter[1].resonance, |p, v| p.filter[1].resonance = v),

        fp("amp_env.attack",  "Amp Attack",  0.0, 5.0, 0.01, "s", |p| p.amp_env.attack,  |p, v| p.amp_env.attack = v),
        fp("amp_env.decay",   "Amp Decay",   0.0, 5.0, 0.1,  "s", |p| p.amp_env.decay,   |p, v| p.amp_env.decay = v),
        fp("amp_env.sustain", "Amp Sustain", 0.0, 1.0, 0.8,  "",  |p| p.amp_env.sustain, |p, v| p.amp_env.sustain = v),
        fp("amp_env.release", "Amp Release", 0.0, 5.0, 0.2,  "s", |p| p.amp_env.release, |p, v| p.amp_env.release = v),

        fp("filter_env.attack",  "Filter Env Attack",  0.0, 5.0, 0.01, "s", |p| p.filter_env.env.attack,  |p, v| p.filter_env.env.attack = v),
        fp("filter_env.decay",   "Filter Env Decay",   0.0, 5.0, 0.1,  "s", |p| p.filter_env.env.decay,   |p, v| p.filter_env.env.decay = v),
        fp("filter_env.sustain", "Filter Env Sustain", 0.0, 1.0, 0.8,  "",  |p| p.filter_env.env.sustain, |p, v| p.filter_env.env.sustain = v),
        fp("filter_env.release", "Filter Env Release", 0.0, 5.0, 0.2,  "s", |p| p.filter_env.env.release, |p, v| p.filter_env.env.release = v),
        fp("filter_env.amount",  "Filter Env Amount",  -1.0, 1.0, 0.0,  "", |p| p.filter_env.amount,      |p, v| p.filter_env.amount = v),

        fp("lfo.rate",  "LFO Rate",  0.0, 20.0, 5.0, "Hz", |p| p.lfo.rate,  |p, v| p.lfo.rate = v),
        fp("lfo.depth", "LFO Depth", 0.0, 1.0, 0.0, "",    |p| p.lfo.depth, |p, v| p.lfo.depth = v),

        fp("reverb.mix",   "Reverb Mix",   0.0, 1.0, 0.0, "", |p| p.reverb.mix,   |p, v| p.reverb.mix = v),
        fp("reverb.size",  "Reverb Size",  0.0, 1.0, 0.5, "", |p| p.reverb.size,  |p, v| p.reverb.size = v),
        fp("reverb.decay", "Reverb Decay", 0.0, 1.0, 0.5, "", |p| p.reverb.decay, |p, v| p.reverb.decay = v),

        fp("master.volume", "Master Volume", 0.0, 1.0, 0.8, "", |p| p.master_volume, |p, v| p.master_volume = v),
    ]
}

#[rustfmt::skip]
pub fn choice_params() -> Vec<ChoiceParam> {
    fn cp(name: &'static str, label: &'static str, options: &'static [&'static str],
          get: fn(&SynthParams) -> &'static str, set: fn(&mut SynthParams, &str) -> bool)
          -> ChoiceParam {
        ChoiceParam { name, label, options, get, set }
    }
    vec![
        cp("osc1.waveform", "Osc 1 Wave", WAVEFORMS, |p| p.osc[0].waveform.as_str(),
           |p, s| match Waveform::from_str(s) { Some(w) => { p.osc[0].waveform = w; true } None => false }),
        cp("osc2.waveform", "Osc 2 Wave", WAVEFORMS, |p| p.osc[1].waveform.as_str(),
           |p, s| match Waveform::from_str(s) { Some(w) => { p.osc[1].waveform = w; true } None => false }),
        cp("osc1.filter.mode", "Osc 1 Filter", FILTER_MODES, |p| p.filter[0].mode.as_str(),
           |p, s| match FilterMode::from_str(s) { Some(m) => { p.filter[0].mode = m; true } None => false }),
        cp("osc2.filter.mode", "Osc 2 Filter", FILTER_MODES, |p| p.filter[1].mode.as_str(),
           |p, s| match FilterMode::from_str(s) { Some(m) => { p.filter[1].mode = m; true } None => false }),
        cp("lfo.target", "LFO Target", LFO_TARGETS, |p| p.lfo.target.as_str(),
           |p, s| match LfoTarget::from_str(s) { Some(t) => { p.lfo.target = t; true } None => false }),
    ]
}

/// The outcome of a named edit, so the agent can be told precisely what happened.
#[derive(Debug, PartialEq)]
pub enum SetResult {
    /// Applied; carries the value actually stored (after clamping for floats).
    Ok(String),
    /// Value was outside the range and got clamped to this value.
    Clamped(f32),
    UnknownParam,
    InvalidChoice,
}

impl SynthParams {
    /// Set a numeric parameter by name, clamping to its declared range.
    pub fn set_float(&mut self, name: &str, value: f32) -> SetResult {
        match float_params().iter().find(|p| p.name == name) {
            Some(p) => {
                let clamped = value.clamp(p.min, p.max);
                (p.set)(self, clamped);
                if (clamped - value).abs() > f32::EPSILON {
                    SetResult::Clamped(clamped)
                } else {
                    SetResult::Ok(clamped.to_string())
                }
            }
            None => SetResult::UnknownParam,
        }
    }

    /// Set an enumerated parameter by name.
    pub fn set_choice(&mut self, name: &str, value: &str) -> SetResult {
        match choice_params().iter().find(|p| p.name == name) {
            Some(p) => {
                if (p.set)(self, value) {
                    SetResult::Ok(value.to_ascii_lowercase())
                } else {
                    SetResult::InvalidChoice
                }
            }
            None => SetResult::UnknownParam,
        }
    }

    /// Read a numeric parameter by name.
    pub fn get_float(&self, name: &str) -> Option<f32> {
        float_params().iter().find(|p| p.name == name).map(|p| (p.get)(self))
    }

    /// Read an enumerated parameter by name.
    pub fn get_choice(&self, name: &str) -> Option<&'static str> {
        choice_params().iter().find(|p| p.name == name).map(|p| (p.get)(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_float_descriptor_roundtrips() {
        let mut p = SynthParams::default();
        for fp in float_params() {
            // Default must be readable and within range.
            let got = p.get_float(fp.name).expect(fp.name);
            assert!(got >= fp.min && got <= fp.max, "{} default out of range", fp.name);
            // Setting mid-range stores exactly.
            let mid = (fp.min + fp.max) * 0.5;
            assert_eq!(p.set_float(fp.name, mid), SetResult::Ok(mid.to_string()));
            assert_eq!(p.get_float(fp.name), Some(mid));
        }
    }

    #[test]
    fn float_out_of_range_is_clamped() {
        let mut p = SynthParams::default();
        assert_eq!(p.set_float("master.volume", 9.0), SetResult::Clamped(1.0));
        assert_eq!(p.get_float("master.volume"), Some(1.0));
    }

    #[test]
    fn choices_roundtrip_and_reject_garbage() {
        let mut p = SynthParams::default();
        for cp in choice_params() {
            for opt in cp.options {
                assert_eq!(p.set_choice(cp.name, opt), SetResult::Ok(opt.to_string()));
                assert_eq!(p.get_choice(cp.name), Some(*opt));
            }
            assert_eq!(p.set_choice(cp.name, "bogus"), SetResult::InvalidChoice);
        }
    }

    #[test]
    fn unknown_params_reported() {
        let mut p = SynthParams::default();
        assert_eq!(p.set_float("osc9.level", 0.5), SetResult::UnknownParam);
        assert_eq!(p.set_choice("osc9.waveform", "sine"), SetResult::UnknownParam);
    }
}
