//! Terminal REPL for the synth. Type a description of a sound; the agent edits
//! the patch, the change is pushed to the live audio engine, and a short chord
//! plays so you hear the result. Type `play` to hear the current patch again,
//! or `quit` to exit.
//!
//! Needs `ANTHROPIC_API_KEY` set and a working audio output device.

use std::io::{self, Write};
use std::thread::sleep;
use std::time::Duration;

use synth_agent::{Agent, ClaudeClient};
use synth_audio::AudioHandle;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let audio = AudioHandle::start()?;
    let mut agent = Agent::new(ClaudeClient::from_env()?);

    // Push the starting patch and play it once so there's a baseline to hear.
    audio.set_params(*agent.params());
    println!("synth ready at {} Hz.", audio.sample_rate());
    println!("Describe a sound (e.g. \"warm mellow pad\"). 'play' repeats it, 'quit' exits.\n");
    play_chord(&audio);

    let stdin = io::stdin();
    loop {
        print!("> ");
        io::stdout().flush()?;
        let mut line = String::new();
        if stdin.read_line(&mut line)? == 0 {
            break; // EOF
        }
        let line = line.trim();
        match line {
            "" => continue,
            "quit" | "exit" => break,
            "play" => {
                play_chord(&audio);
                continue;
            }
            _ => {}
        }

        match agent.send(line).await {
            Ok(turn) => {
                audio.set_params(*agent.params());
                for change in &turn.changes {
                    println!("  {} {}", change.name, change.outcome);
                }
                if !turn.reply.is_empty() {
                    println!("{}\n", turn.reply);
                }
                play_chord(&audio);
            }
            Err(err) => eprintln!("error: {err}\n"),
        }
    }

    Ok(())
}

/// Play a short C-major chord so the current patch is audible.
fn play_chord(audio: &AudioHandle) {
    for note in [60, 64, 67] {
        audio.note_on(note, 0.8);
    }
    sleep(Duration::from_millis(900));
    for note in [60, 64, 67] {
        audio.note_off(note);
    }
    sleep(Duration::from_millis(500));
}
