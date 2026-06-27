//! Plays a short chord through the speakers to verify live audio output.
//!
//! Run with: `cargo run -p synth-audio --example play`

use std::thread::sleep;
use std::time::Duration;

use synth_audio::AudioHandle;
use synth_core::SynthParams;

fn main() -> anyhow::Result<()> {
    let audio = AudioHandle::start()?;
    println!("audio running at {} Hz", audio.sample_rate());

    let mut p = SynthParams::default();
    p.set_choice("osc2.waveform", "saw");
    p.set_float("osc2.level", 0.4);
    p.set_float("osc2.fine", 7.0);
    p.set_float("osc1.filter.cutoff", 2_000.0);
    audio.set_params(p);

    // A C major chord.
    for note in [60, 64, 67] {
        audio.note_on(note, 0.8);
    }
    sleep(Duration::from_millis(1500));

    for note in [60, 64, 67] {
        audio.note_off(note);
    }
    // Let the release tail ring out.
    sleep(Duration::from_millis(800));

    println!("done");
    Ok(())
}
