//! Real-time audio output for the synth.
//!
//! The synth engine lives on the audio callback thread. Everything else (UI,
//! agent) talks to it by sending [`Command`]s over a lock-free channel, which
//! the callback drains at the start of each buffer. This keeps the realtime
//! thread free of locks and allocation, and keeps `synth-core` unaware of how
//! audio reaches the speakers.

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, Sender};
use synth_core::{Synth, SynthParams};

/// A message from the UI/agent to the audio thread.
pub enum Command {
    NoteOn { note: u8, velocity: f32 },
    NoteOff { note: u8 },
    /// Replace the whole patch. `SynthParams` is plain `Copy` data, so sending a
    /// fresh snapshot is cheaper and simpler than streaming individual edits.
    SetParams(SynthParams),
    SeqRunning(bool),
    SeqTempo(f32),
    SeqLength(usize),
    SeqStep { index: usize, active: bool, note: u8 },
}

/// Owns the audio stream and the sending end of the command channel. Dropping it
/// stops audio. Held by the main/UI thread.
pub struct AudioHandle {
    tx: Sender<Command>,
    sample_rate: f32,
    // Keep the stream alive; cpal stops output when this is dropped.
    _stream: cpal::Stream,
}

impl AudioHandle {
    /// Open the default output device and start streaming silence until notes
    /// arrive.
    pub fn start() -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow!("no default audio output device"))?;
        let config = device
            .default_output_config()
            .context("querying default output config")?;
        let sample_rate = config.sample_rate().0 as f32;
        let channels = config.channels() as usize;

        let (tx, rx) = crossbeam_channel::unbounded::<Command>();
        let synth = Synth::new(sample_rate);

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => build_stream(&device, &config.into(), synth, rx, channels)?,
            other => return Err(anyhow!("unsupported sample format: {other:?}")),
        };
        stream.play().context("starting audio stream")?;

        Ok(Self { tx, sample_rate, _stream: stream })
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn note_on(&self, note: u8, velocity: f32) {
        let _ = self.tx.send(Command::NoteOn { note, velocity });
    }

    pub fn note_off(&self, note: u8) {
        let _ = self.tx.send(Command::NoteOff { note });
    }

    pub fn set_params(&self, params: SynthParams) {
        let _ = self.tx.send(Command::SetParams(params));
    }

    /// A `Send + Sync + Clone` handle to control the audio without owning the
    /// stream. The cpal `Stream` itself is `!Send`, so it must stay on its
    /// creating thread; this carries only the command channel, which can live in
    /// shared application state (e.g. a Tauri-managed state).
    pub fn controller(&self) -> AudioController {
        AudioController { tx: self.tx.clone(), sample_rate: self.sample_rate }
    }
}

/// A detachable, thread-safe handle for driving the audio engine. Obtain one
/// from [`AudioHandle::controller`].
#[derive(Clone)]
pub struct AudioController {
    tx: Sender<Command>,
    sample_rate: f32,
}

impl AudioController {
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn note_on(&self, note: u8, velocity: f32) {
        let _ = self.tx.send(Command::NoteOn { note, velocity });
    }

    pub fn note_off(&self, note: u8) {
        let _ = self.tx.send(Command::NoteOff { note });
    }

    pub fn set_params(&self, params: SynthParams) {
        let _ = self.tx.send(Command::SetParams(params));
    }

    pub fn seq_set_running(&self, running: bool) {
        let _ = self.tx.send(Command::SeqRunning(running));
    }

    pub fn seq_set_tempo(&self, bpm: f32) {
        let _ = self.tx.send(Command::SeqTempo(bpm));
    }

    pub fn seq_set_length(&self, length: usize) {
        let _ = self.tx.send(Command::SeqLength(length));
    }

    pub fn seq_set_step(&self, index: usize, active: bool, note: u8) {
        let _ = self.tx.send(Command::SeqStep { index, active, note });
    }
}

fn build_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut synth: Synth,
    rx: Receiver<Command>,
    channels: usize,
) -> Result<cpal::Stream> {
    // Scratch buffer for one block of mono audio, reused across callbacks so the
    // realtime thread never allocates.
    let mut mono = Vec::new();

    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                // Apply any pending note/param changes before rendering.
                while let Ok(cmd) = rx.try_recv() {
                    match cmd {
                        Command::NoteOn { note, velocity } => synth.note_on(note, velocity),
                        Command::NoteOff { note } => synth.note_off(note),
                        Command::SetParams(p) => synth.set_params(p),
                        Command::SeqRunning(r) => synth.sequencer().set_running(r),
                        Command::SeqTempo(b) => synth.sequencer().set_tempo(b),
                        Command::SeqLength(n) => synth.sequencer().set_length(n),
                        Command::SeqStep { index, active, note } => {
                            synth.sequencer().set_step(index, active, note)
                        }
                    }
                }

                let frames = data.len() / channels;
                mono.resize(frames, 0.0);
                synth.render(&mut mono);

                // Fan the mono signal out to every output channel.
                for (frame, &s) in data.chunks_mut(channels).zip(mono.iter()) {
                    for sample in frame.iter_mut() {
                        *sample = s;
                    }
                }
            },
            |err| eprintln!("audio stream error: {err}"),
            None,
        )
        .context("building output stream")?;
    Ok(stream)
}
