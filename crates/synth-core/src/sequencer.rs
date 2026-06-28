//! A simple 16-step sequencer that drives the engine. When running, it advances
//! one step at a time (16th notes at the set tempo), releasing the previous
//! step's note and triggering the current step's note. Looping it lets you hear
//! the agent reshape the sound live while a pattern plays.
//!
//! The sequencer only decides *when* to start and stop notes; the engine owns
//! the voices. `tick` returns the note events for one sample so the engine can
//! act on them without aliasing borrows.

/// Maximum number of steps.
pub const MAX_STEPS: usize = 16;

const DEFAULT_BPM: f32 = 120.0;
const STEP_VELOCITY: f32 = 0.9;

#[derive(Debug, Clone, Copy)]
pub struct Step {
    pub active: bool,
    /// MIDI note this step plays when active.
    pub note: u8,
}

/// Note events produced by a single sample tick.
#[derive(Debug, Clone, Copy, Default)]
pub struct SeqTick {
    pub release: Option<u8>,
    pub trigger: Option<(u8, f32)>,
}

#[derive(Debug, Clone)]
pub struct Sequencer {
    sample_rate: f32,
    running: bool,
    bpm: f32,
    steps: [Step; MAX_STEPS],
    /// Number of steps in the loop, `1..=MAX_STEPS`.
    length: usize,
    // Runtime position (unused while stopped).
    pos: usize,
    counter: u32,
    playing: Option<u8>,
}

impl Sequencer {
    pub fn new(sample_rate: f32) -> Self {
        // A gentle C-minor-pentatonic arp on every other step, so hitting play
        // gives an immediate musical loop to audition sounds against.
        let scale = [48, 51, 53, 55, 58, 60, 63, 65, 68, 70, 72, 75, 77, 79, 82, 84];
        let mut steps = [Step { active: false, note: 60 }; MAX_STEPS];
        for (i, step) in steps.iter_mut().enumerate() {
            step.note = scale[i];
            step.active = i % 2 == 0;
        }
        Self {
            sample_rate,
            running: false,
            bpm: DEFAULT_BPM,
            steps,
            length: MAX_STEPS,
            pos: 0,
            counter: 0,
            playing: None,
        }
    }

    pub fn is_running(&self) -> bool {
        self.running
    }
    pub fn bpm(&self) -> f32 {
        self.bpm
    }
    pub fn length(&self) -> usize {
        self.length
    }
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// Start or stop playback. Starting resets to the first step.
    pub fn set_running(&mut self, running: bool) {
        self.running = running;
        if running {
            self.pos = 0;
            self.counter = 0;
        }
        // A lingering note is released by the next `tick` when stopped.
    }

    pub fn set_tempo(&mut self, bpm: f32) {
        self.bpm = bpm.clamp(20.0, 300.0);
    }

    pub fn set_length(&mut self, length: usize) {
        self.length = length.clamp(1, MAX_STEPS);
        if self.pos >= self.length {
            self.pos = 0;
            self.counter = 0;
        }
    }

    pub fn set_step(&mut self, index: usize, active: bool, note: u8) {
        if let Some(step) = self.steps.get_mut(index) {
            step.active = active;
            step.note = note;
        }
    }

    /// Samples per step, treating each step as a 16th note.
    fn samples_per_step(&self) -> u32 {
        ((self.sample_rate * 15.0 / self.bpm.max(1.0)) as u32).max(1)
    }

    /// Advance one sample and return any note events at this instant.
    pub fn tick(&mut self) -> SeqTick {
        if !self.running {
            // Release a note left sounding when playback stopped.
            return SeqTick { release: self.playing.take(), trigger: None };
        }

        let mut tick = SeqTick::default();
        if self.counter == 0 {
            // Entering a new step: end the previous note, start this one.
            tick.release = self.playing.take();
            let step = self.steps[self.pos];
            if step.active {
                tick.trigger = Some((step.note, STEP_VELOCITY));
                self.playing = Some(step.note);
            }
        }

        self.counter += 1;
        if self.counter >= self.samples_per_step() {
            self.counter = 0;
            self.pos = (self.pos + 1) % self.length.max(1);
        }
        tick
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopped_sequencer_emits_nothing() {
        let mut seq = Sequencer::new(48_000.0);
        for _ in 0..10_000 {
            let t = seq.tick();
            assert!(t.trigger.is_none() && t.release.is_none());
        }
    }

    #[test]
    fn running_triggers_first_active_step_immediately() {
        let mut seq = Sequencer::new(48_000.0);
        seq.set_step(0, true, 60);
        seq.set_running(true);
        let t = seq.tick();
        assert_eq!(t.trigger, Some((60, STEP_VELOCITY)));
    }

    #[test]
    fn steps_advance_and_loop_at_tempo() {
        let sr = 48_000.0;
        let mut seq = Sequencer::new(sr);
        seq.set_length(2);
        seq.set_step(0, true, 60);
        seq.set_step(1, true, 64);
        seq.set_tempo(120.0);
        seq.set_running(true);

        let spb = (sr * 15.0 / 120.0) as usize; // samples per 16th note
        let mut triggers = Vec::new();
        for _ in 0..(spb * 3) {
            if let Some((n, _)) = seq.tick().trigger {
                triggers.push(n);
            }
        }
        // step0 -> step1 -> loop back to step0.
        assert_eq!(triggers, vec![60, 64, 60]);
    }

    #[test]
    fn stopping_releases_the_held_note() {
        let mut seq = Sequencer::new(48_000.0);
        seq.set_step(0, true, 72);
        seq.set_running(true);
        assert_eq!(seq.tick().trigger, Some((72, STEP_VELOCITY)));
        seq.set_running(false);
        assert_eq!(seq.tick().release, Some(72));
    }
}
