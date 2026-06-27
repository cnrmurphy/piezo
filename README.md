# Piezo

A small subtractive synthesizer with an agentic harness: shape the sound by
describing it in plain English, the same way you'd ask an agent to write code.
The agent adjusts the synth's parameters for you.

## About
I thought it would be interesting if you could have a synthesizer controlled by an AI agent. I'm mostly interested in how
it navigates the process of synthesis. Please note that this is a prototype and therefore it is heavily vibecoded so I that
I could experiment quickly.

## Features

- Two oscillators, each with its own filter (low-/high-/band-pass).
- Waveforms: sine, saw, square, and white noise. Saw and square are
  band-limited (PolyBLEP) so they don't alias.
- An amplitude envelope and a separate filter envelope (ADSR).
- An LFO routable to pitch, filter cutoff, or amplitude.
- 8-voice polyphony.
- An agent that edits the patch from a plain-English description.
- A desktop UI with a knob for every parameter, a chat box, and a keyboard
  piano (play with the `A`–`L` row, or click).

## Architecture

| Crate         | What it does                                                      |
| ------------- | ---------------------------------------------------------------- |
| `synth-core`  | Rust DSP: oscillators, filters, envelopes, LFO, voices, and the named parameter store. |
| `synth-audio` | Real-time output via [cpal]. The engine runs on the audio thread; callers send commands over a lock-free channel. |
| `synth-agent` | The agentic harness: a provider-agnostic LLM trait, a Claude implementation, and a tool-use loop. Tools and the system prompt are generated from the parameter table. |
| `synth-cli`   | A terminal REPL                       |
| `src-tauri` + `ui/` | The desktop app: knobs, keyboard piano, and the agent chat. |

[cpal]: https://github.com/RustAudio/cpal

## Prerequisites

- A recent Rust toolchain.
- A working audio output device.
- For the agent (optional): an `ANTHROPIC_API_KEY` (see [Cost](#cost)).

On Fedora, the system libraries needed to build the audio layer and the desktop
app:

```sh
sudo dnf install alsa-lib-devel \
  webkit2gtk4.1-devel gtk3-devel libsoup3-devel librsvg2-devel openssl-devel
```

(Other Linux distributions need the equivalent ALSA and WebKitGTK/GTK3/libsoup3
development packages.)

## Running

### Desktop app

```sh
export ANTHROPIC_API_KEY=sk-ant-...        # optional — see Cost
cargo run -p synth-tauri
```

A window opens with the knobs, chat, and keyboard. Play notes with the `A`–`L`
row of your keyboard (or click the keys). Type a sound into the chat ("warm
mellow pad", "brighter", "more movement") and watch the knobs sweep to their new
values as the agent patches.

On Wayland, if the window renders blank, prefix the command with
`WEBKIT_DISABLE_DMABUF_RENDERER=1`.

Without `ANTHROPIC_API_KEY` the app still runs for hands-on knob and keyboard
use; the chat reports that the agent is unavailable.

### Terminal REPL

```sh
export ANTHROPIC_API_KEY=sk-ant-...
cargo run -p synth-cli
```

Type a description and it edits the patch and plays a chord so you hear it.
`play` repeats the current patch; `quit` exits.

### Hear the engine with no app and no API key

```sh
cargo run -p synth-core --example render_wav   # writes synth-demo.wav
```

## How the agent works

The agent does not listen to the audio. It translates descriptive language into
parameter changes using a language model's knowledge of subtractive synthesis,
guided by a system prompt that lists every parameter (with ranges) and the
signal path. It edits the patch through three tools — set a numeric parameter,
set a choice parameter, and read the current patch — all generated from the same
parameter table the UI uses. Reading the current patch lets relative requests
like "brighter" adjust from the actual current values, including knobs you moved
by hand.

The model provider is abstracted behind a trait, with one concrete Claude
(Anthropic) implementation, so a different or local model can be swapped in
without changing the agent logic.

## Cost

The agent calls the Anthropic Claude API directly, billed per token against the
account that owns your `ANTHROPIC_API_KEY`. This is separate from any
subscription. Everything except the chat — the knobs, the keyboard, and the WAV
example — is free and runs without a key.

## Testing

```sh
cargo test --workspace
```

The DSP core is covered by unit tests, and the agent's tool loop is tested
against a scripted mock model, so neither needs an audio device or an API key.

## License

MIT. See [LICENSE](LICENSE).
