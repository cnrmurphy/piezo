// Frontend for the synth. Builds the knob panel from the backend's parameter
// schema, drives the synth by hand, plays notes from the computer keyboard, and
// talks to the agent. Manual edits and agent edits share one patch on the
// backend; the "patch-changed" event keeps the knobs in sync.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const GROUP_ORDER = ["osc1", "osc2", "amp_env", "filter_env", "lfo", "master"];
const GROUP_LABELS = {
  osc1: "Oscillator 1",
  osc2: "Oscillator 2",
  amp_env: "Amp Envelope",
  filter_env: "Filter Envelope",
  lfo: "LFO",
  master: "Master",
};

// Maps each control's name to a setter that updates its displayed value.
const updaters = new Map();

async function init() {
  const schema = await invoke("get_schema");
  const params = await invoke("get_params");
  buildControls(schema, params);
  buildKeyboard();
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

  const show = (v) => {
    val.textContent = formatValue(v, p.unit);
    input.value = v;
  };
  show(value);

  input.addEventListener("input", async () => {
    const v = Number(input.value);
    val.textContent = formatValue(v, p.unit);
    await invoke("set_param", { name: p.name, value: v });
  });

  updaters.set(p.name, show);
  wrap.append(head, input);
  return wrap;
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
    button.disabled = true;
    try {
      const turn = await invoke("chat", { message: text });
      addMessage("agent", turn.reply || "(no reply)", turn.changes);
      applyParams(turn.params);
    } catch (err) {
      addMessage("error", String(err));
    } finally {
      button.disabled = false;
      input.focus();
    }
  });
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

window.addEventListener("DOMContentLoaded", () => {
  init().catch((err) => {
    document.getElementById("status").textContent = `init error: ${err}`;
  });
});
