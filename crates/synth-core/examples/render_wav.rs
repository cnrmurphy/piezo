//! Renders a short demo to `synth-demo.wav` so you can hear the engine without
//! any audio backend installed. Plays a little chord arpeggio while sweeping the
//! filter, exercising oscillators, filters, envelopes, and the LFO.
//!
//! Run with: `cargo run -p synth-core --example render_wav`

use synth_core::params::SynthParams;
use synth_core::Synth;

fn main() {
    let sample_rate = 44_100.0;
    let mut synth = Synth::new(sample_rate);

    // A fat detuned-saw patch with a moving low-pass filter.
    let mut p = SynthParams::default();
    p.set_choice("osc1.waveform", "saw");
    p.set_choice("osc2.waveform", "saw");
    p.set_float("osc2.level", 0.5);
    p.set_float("osc2.fine", 8.0); // slight detune for width
    p.set_float("osc1.filter.cutoff", 1_200.0);
    p.set_float("osc2.filter.cutoff", 1_200.0);
    p.set_float("osc1.filter.resonance", 0.4);
    p.set_float("osc2.filter.resonance", 0.4);
    p.set_float("filter_env.amount", 0.8);
    p.set_float("filter_env.decay", 0.4);
    p.set_float("filter_env.sustain", 0.2);
    p.set_float("amp_env.attack", 0.005);
    p.set_float("amp_env.release", 0.3);
    p.set_choice("lfo.target", "pitch");
    p.set_float("lfo.rate", 5.0);
    p.set_float("lfo.depth", 0.15);
    // Show off the stereo field and reverb.
    p.set_float("master.width", 0.8);
    p.set_float("reverb.mix", 0.3);
    p.set_float("reverb.size", 0.7);
    synth.set_params(p);

    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: sample_rate as u32,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create("synth-demo.wav", spec).expect("create wav");

    // C minor arpeggio, each note held then released.
    let notes = [60, 63, 67, 72, 67, 63];
    let note_samples = (sample_rate * 0.35) as usize;
    let hold_samples = (sample_rate * 0.25) as usize;
    let mut left = vec![0.0f32; 512];
    let mut right = vec![0.0f32; 512];

    for &note in &notes {
        synth.note_on(note, 0.9);
        let mut rendered = 0;
        while rendered < note_samples {
            let n = left.len().min(note_samples - rendered);
            synth.render(&mut left[..n], &mut right[..n]);
            write_block(&mut writer, &left[..n], &right[..n]);
            if rendered == hold_samples {
                synth.note_off(note);
            }
            rendered += n;
        }
    }

    // Let the final release tail ring out.
    for _ in 0..((sample_rate * 0.6) as usize / 512) {
        synth.render(&mut left, &mut right);
        write_block(&mut writer, &left, &right);
    }

    writer.finalize().expect("finalize wav");
    println!("wrote synth-demo.wav");
}

/// Write interleaved stereo samples (left, right, left, right, ...).
fn write_block<W: std::io::Write + std::io::Seek>(
    writer: &mut hound::WavWriter<W>,
    left: &[f32],
    right: &[f32],
) {
    for (&l, &r) in left.iter().zip(right.iter()) {
        let to_i16 = |s: f32| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        writer.write_sample(to_i16(l)).expect("write sample");
        writer.write_sample(to_i16(r)).expect("write sample");
    }
}
