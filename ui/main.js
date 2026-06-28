// Frontend for the synth. Builds the knob panel from the backend's parameter
// schema, drives the synth by hand, plays notes from the computer keyboard, and
// talks to the agent. Manual edits and agent edits share one patch on the
// backend; the "patch-changed" event keeps the knobs in sync.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const GROUP_ORDER = ["osc1", "osc2", "amp_env", "filter_env", "lfo", "reverb", "master"];
const GROUP_LABELS = {
  osc1: "Oscillator 1",
  osc2: "Oscillator 2",
  amp_env: "Amp Envelope",
  filter_env: "Filter Envelope",
  lfo: "LFO",
  reverb: "Reverb",
  master: "Master",
};

// Maps each control's name to a setter that updates its displayed value.
const updaters = new Map();

async function init() {
  const schema = await invoke("get_schema");
  const params = await invoke("get_params");
  buildControls(schema, params);
  buildKeyboard();
  await buildSequencer();
  wireChat();
  wireComputerKeyboard();

  // The agent edits the same patch; reflect its changes on the knobs.
  await listen("patch-changed", (event) => applyParams(event.payload));
}

function buildControls(schema, params) {
  const controls = document.getElementById("controls");
  const groups = new Map();
  const ensureGroup = (key) => {
    if (!groups.has(key)) {
      const el = document.createElement("div");
      el.className = "group";
      const h = document.createElement("h2");
      h.textContent = GROUP_LABELS[key] || key;
      el.appendChild(h);
      groups.set(key, el);
    }
    return groups.get(key);
  };

  for (const p of schema.floats) {
    ensureGroup(p.group).appendChild(floatControl(p, params.floats[p.name]));
  }
  for (const p of schema.choices) {
    ensureGroup(p.group).appendChild(choiceControl(p, params.choices[p.name]));
  }

  const ordered = [
    ...GROUP_ORDER.filter((g) => groups.has(g)),
    ...[...groups.keys()].filter((g) => !GROUP_ORDER.includes(g)),
  ];
  for (const key of ordered) controls.appendChild(groups.get(key));
}

function floatControl(p, value) {
  const wrap = document.createElement("div");
  wrap.className = "param";

  const head = document.createElement("div");
  head.className = "param-head";
  const label = document.createElement("span");
  label.textContent = p.label;
  const val = document.createElement("span");
  val.className = "val";
  head.append(label, val);

  const input = document.createElement("input");
  input.type = "range";
  input.min = p.min;
  input.max = p.max;
  input.step = (p.max - p.min) / 200 || 0.01;
  input.value = value;

  val.textContent = formatValue(value, p.unit);
  input.value = value;

  input.addEventListener("input", async () => {
    if (input._raf) cancelAnimationFrame(input._raf); // a drag cancels any sweep
    const v = Number(input.value);
    val.textContent = formatValue(v, p.unit);
    await invoke("set_param", { name: p.name, value: v });
  });

  // Streamed agent edits glide the slider to the new value rather than snapping.
  updaters.set(p.name, (target) => animateFloat(input, val, p.unit, target));
  wrap.append(head, input);
  return wrap;
}

function animateFloat(input, valEl, unit, target) {
  if (input._raf) cancelAnimationFrame(input._raf);
  const from = Number(input.value);
  const to = Number(target);
  if (!isFinite(from) || from === to) {
    input.value = to;
    valEl.textContent = formatValue(to, unit);
    return;
  }
  const duration = 150;
  const start = performance.now();
  const tick = (now) => {
    const t = Math.min(1, (now - start) / duration);
    const eased = 1 - Math.pow(1 - t, 3); // ease-out cubic
    const v = from + (to - from) * eased;
    input.value = v;
    valEl.textContent = formatValue(v, unit);
    if (t < 1) {
      input._raf = requestAnimationFrame(tick);
    } else {
      input._raf = null;
      input.value = to;
      valEl.textContent = formatValue(to, unit);
    }
  };
  input._raf = requestAnimationFrame(tick);
}

function choiceControl(p, value) {
  const wrap = document.createElement("div");
  wrap.className = "param";

  const head = document.createElement("div");
  head.className = "param-head";
  const label = document.createElement("span");
  label.textContent = p.label;
  head.appendChild(label);

  const select = document.createElement("select");
  for (const opt of p.options) {
    const o = document.createElement("option");
    o.value = opt;
    o.textContent = opt;
    select.appendChild(o);
  }
  select.value = value;

  select.addEventListener("change", async () => {
    await invoke("set_choice", { name: p.name, value: select.value });
  });

  updaters.set(p.name, (v) => {
    select.value = v;
  });
  wrap.append(head, select);
  return wrap;
}

function formatValue(v, unit) {
  const n = Math.abs(v) >= 100 ? Math.round(v) : Math.round(v * 100) / 100;
  return unit ? `${n} ${unit}` : `${n}`;
}

function applyParams(params) {
  for (const [name, v] of Object.entries(params.floats || {})) {
    updaters.get(name)?.(v);
  }
  for (const [name, v] of Object.entries(params.choices || {})) {
    updaters.get(name)?.(v);
  }
}

// --- Chat ------------------------------------------------------------------

function wireChat() {
  const form = document.getElementById("chat-form");
  const input = document.getElementById("chat-input");
  const button = form.querySelector("button");

  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    const text = input.value.trim();
    if (!text) return;
    input.value = "";
    addMessage("user", text);
    const thinking = addThinking();
    button.disabled = true;
    input.disabled = true;
    setStatus("agent working…");
    try {
      const turn = await invoke("chat", { message: text });
      thinking.remove();
      addMessage("agent", turn.reply || "(no reply)", turn.changes);
      applyParams(turn.params);
    } catch (err) {
      thinking.remove();
      addMessage("error", String(err));
    } finally {
      button.disabled = false;
      input.disabled = false;
      setStatus("");
      input.focus();
    }
  });
}

function setStatus(text) {
  document.getElementById("status").textContent = text;
}

// An animated "the agent is working" bubble shown while a turn is in flight.
function addThinking() {
  const log = document.getElementById("log");
  const el = document.createElement("div");
  el.className = "msg agent thinking";
  el.innerHTML = '<span class="dots"><i></i><i></i><i></i></span>';
  log.appendChild(el);
  log.scrollTop = log.scrollHeight;
  return el;
}

function addMessage(kind, text, changes) {
  const log = document.getElementById("log");
  const msg = document.createElement("div");
  msg.className = `msg ${kind}`;
  msg.textContent = text;
  if (changes && changes.length) {
    const c = document.createElement("div");
    c.className = "changes";
    c.textContent = changes.map((x) => `${x.name} ${x.outcome}`).join("\n");
    msg.appendChild(c);
  }
  log.appendChild(msg);
  log.scrollTop = log.scrollHeight;
}

// --- Keyboard piano --------------------------------------------------------

// QWERTY rows mapped to semitone offsets from the base note (C4 = 60).
const KEY_MAP = {
  a: 0, w: 1, s: 2, e: 3, d: 4, f: 5, t: 6,
  g: 7, y: 8, h: 9, u: 10, j: 11, k: 12, o: 13, l: 14,
};
const BASE_NOTE = 60;
const held = new Set();

function noteName(semi) {
  const names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
  return names[((semi % 12) + 12) % 12];
}

function buildKeyboard() {
  const kb = document.getElementById("keyboard");
  const entries = Object.entries(KEY_MAP).sort((a, b) => a[1] - b[1]);
  for (const [key, semi] of entries) {
    const note = BASE_NOTE + semi;
    const el = document.createElement("div");
    el.className = "key" + (noteName(semi).includes("#") ? " sharp" : "");
    el.dataset.note = note;
    el.textContent = key.toUpperCase();
    el.addEventListener("mousedown", () => pressNote(note, el));
    el.addEventListener("mouseup", () => releaseNote(note, el));
    el.addEventListener("mouseleave", () => releaseNote(note, el));
    kb.appendChild(el);
  }
}

function keyEl(note) {
  return document.querySelector(`.key[data-note="${note}"]`);
}

async function pressNote(note, el) {
  if (held.has(note)) return;
  held.add(note);
  (el || keyEl(note))?.classList.add("down");
  await invoke("note_on", { note, velocity: 0.85 });
}

async function releaseNote(note, el) {
  if (!held.has(note)) return;
  held.delete(note);
  (el || keyEl(note))?.classList.remove("down");
  await invoke("note_off", { note });
}

function wireComputerKeyboard() {
  document.addEventListener("keydown", (e) => {
    if (e.repeat || typingInChat(e)) return;
    const semi = KEY_MAP[e.key.toLowerCase()];
    if (semi === undefined) return;
    pressNote(BASE_NOTE + semi);
  });
  document.addEventListener("keyup", (e) => {
    const semi = KEY_MAP[e.key.toLowerCase()];
    if (semi === undefined) return;
    releaseNote(BASE_NOTE + semi);
  });
}

function typingInChat(e) {
  return e.target && e.target.id === "chat-input";
}

// --- Step sequencer --------------------------------------------------------

let seqLength = 16;

async function buildSequencer() {
  const s = await invoke("seq_state");
  seqLength = s.length;

  const playBtn = document.getElementById("seq-play");
  const tempo = document.getElementById("seq-tempo");
  const lengthSel = document.getElementById("seq-length");
  const grid = document.getElementById("seq-grid");

  let running = s.running;
  const renderPlay = () => {
    playBtn.textContent = running ? "Stop" : "Play";
    playBtn.classList.toggle("on", running);
  };
  renderPlay();
  playBtn.addEventListener("click", async () => {
    running = !running;
    renderPlay();
    await invoke("seq_set_running", { running });
  });

  tempo.value = Math.round(s.bpm);
  tempo.addEventListener("change", async () => {
    const bpm = Math.min(300, Math.max(20, Number(tempo.value) || 120));
    tempo.value = bpm;
    await invoke("seq_set_tempo", { bpm });
  });

  for (let i = 1; i <= 16; i++) {
    const o = document.createElement("option");
    o.value = i;
    o.textContent = i;
    lengthSel.appendChild(o);
  }
  lengthSel.value = s.length;

  const cells = [];
  const updateDisabled = () =>
    cells.forEach((c, i) => c.classList.toggle("disabled", i >= seqLength));

  lengthSel.addEventListener("change", async () => {
    seqLength = Number(lengthSel.value);
    await invoke("seq_set_length", { length: seqLength });
    updateDisabled();
  });

  s.steps.forEach((step, i) => {
    const col = document.createElement("div");
    col.className = "seq-step";

    let active = step.active;
    let note = step.note;
    const send = () => invoke("seq_set_step", { index: i, active, note });

    const pad = document.createElement("button");
    pad.type = "button";
    pad.className = "seq-pad" + (active ? " active" : "");
    const name = document.createElement("span");
    name.className = "seq-note-name";
    name.textContent = midiLabel(note);
    pad.appendChild(name);
    pad.addEventListener("click", () => {
      active = !active;
      pad.classList.toggle("active", active);
      send();
    });

    const noteInput = document.createElement("input");
    noteInput.type = "number";
    noteInput.min = 24;
    noteInput.max = 96;
    noteInput.value = note;
    noteInput.className = "seq-note";
    noteInput.addEventListener("change", () => {
      note = Math.min(96, Math.max(24, Number(noteInput.value) || 60));
      noteInput.value = note;
      name.textContent = midiLabel(note);
      send();
    });

    col.append(pad, noteInput);
    grid.appendChild(col);
    cells.push(col);
  });

  updateDisabled();
}

// MIDI note number to a name with octave (60 = C4).
function midiLabel(m) {
  const names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
  return names[((m % 12) + 12) % 12] + (Math.floor(m / 12) - 1);
}

window.addEventListener("DOMContentLoaded", () => {
  init().catch((err) => {
    document.getElementById("status").textContent = `init error: ${err}`;
  });
});
