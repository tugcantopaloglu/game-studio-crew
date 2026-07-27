import { settings, onStage } from "/bus.js";

const NEAR = 15;
const MAX_VOICES = 2;
const VOWELS = [
  [420, 1650], [560, 1200], [320, 2100], [700, 1150], [480, 1850], [380, 950],
];

let ctx = null;
let master = null;
let stage = null;
let ticker = null;
let voices = 0;
let started = false;

function hash(text) {
  let h = 2166136261;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return (h >>> 0) / 4294967295;
}

function clamp(v, lo, hi) {
  return v < lo ? lo : v > hi ? hi : v;
}

function volume() {
  if (!settings.get("chatter.enabled")) return 0;
  return clamp(Number(settings.get("chatter.volume")) || 0, 0, 1);
}

function open() {
  if (ctx) return ctx;
  const Ctor = window.AudioContext || window.webkitAudioContext;
  if (!Ctor) return null;
  try {
    ctx = new Ctor();
  } catch (err) {
    ctx = null;
    return null;
  }
  master = ctx.createGain();
  master.gain.value = volume();
  master.connect(ctx.destination);
  return ctx;
}

function wake() {
  const c = open();
  if (!c) return;
  if (c.state === "suspended") c.resume().catch(() => {});
  if (master) master.gain.value = volume();
}

function audible() {
  return ctx && ctx.state === "running" && master && volume() > 0 && stage;
}

function utter(a, loudness, pan, seed) {
  if (voices >= MAX_VOICES) return 0;
  const c = ctx;
  const t0 = c.currentTime + 0.03;
  const base = 96 + seed * 96;
  const count = 2 + Math.floor(Math.random() * 5);

  const osc = c.createOscillator();
  osc.type = seed > 0.55 ? "sawtooth" : "triangle";
  const band = c.createBiquadFilter();
  band.type = "bandpass";
  band.Q.value = 5.5;
  const tone = c.createBiquadFilter();
  tone.type = "lowpass";
  tone.frequency.value = 2400;
  const gain = c.createGain();
  gain.gain.setValueAtTime(0.0001, t0);

  osc.connect(band);
  band.connect(tone);
  tone.connect(gain);
  if (c.createStereoPanner) {
    const panner = c.createStereoPanner();
    panner.pan.value = clamp(pan, -1, 1);
    gain.connect(panner);
    panner.connect(master);
  } else {
    gain.connect(master);
  }

  let at = t0;
  for (let i = 0; i < count; i++) {
    const len = 0.09 + Math.random() * 0.09;
    const gap = 0.03 + Math.random() * 0.05;
    const drop = i === count - 1 ? 0.78 : 1 + (Math.random() - 0.5) * 0.16;
    const from = VOWELS[Math.floor(Math.random() * VOWELS.length)];
    const to = VOWELS[Math.floor(Math.random() * VOWELS.length)];
    const peak = Math.max(0.002, loudness * (0.55 + Math.random() * 0.45));

    osc.frequency.setValueAtTime(base * (0.94 + Math.random() * 0.14), at);
    osc.frequency.linearRampToValueAtTime(base * drop, at + len);
    band.frequency.setValueAtTime(from[Math.random() > 0.5 ? 0 : 1], at);
    band.frequency.linearRampToValueAtTime(to[Math.random() > 0.5 ? 0 : 1], at + len);

    gain.gain.setValueAtTime(0.0001, at);
    gain.gain.exponentialRampToValueAtTime(peak, at + 0.03);
    gain.gain.setValueAtTime(peak, at + len - 0.035);
    gain.gain.exponentialRampToValueAtTime(0.0001, at + len);
    at += len + gap;
  }

  voices++;
  osc.onended = () => { voices--; };
  osc.start(t0);
  osc.stop(at + 0.06);

  const dur = at - t0;
  if (a && stage.now) a.talkUntil = stage.now() + dur;
  return dur;
}

function screenPan(position) {
  const cam = stage.camera;
  const right = cam.matrixWorld.elements;
  const dx = position.x - cam.position.x;
  const dz = position.z - cam.position.z;
  return clamp((right[0] * dx + right[2] * dz) / 14, -0.85, 0.85);
}

function speak(role, force) {
  if (!audible()) return;
  const a = stage.avatars.get(role);
  if (!a) return;
  const d = a.person.position.distanceTo(stage.camera.position);
  if (!force && d > NEAR) return;
  const falloff = clamp(1 - (d - 3) / (NEAR + 8), 0.14, 1);
  utter(a, falloff * 0.85, screenPan(a.person.position), hash(role));
}

function tick() {
  if (!audible() || voices >= MAX_VOICES) return;
  const focus = stage.focused ? stage.focused() : null;
  const cam = stage.camera;

  let pick = null;
  let best = -1;
  for (const [role, a] of stage.avatars) {
    const st = stage.deskState(role);
    const meeting = st.ring === "meeting";
    const d = a.person.position.distanceTo(cam.position);
    if (!meeting && d > NEAR && role !== focus) continue;
    const chance = (meeting ? 0.55 : role === focus ? 0.3 : 0.16) * Math.random();
    if (chance > best) { best = chance; pick = { role, meeting }; }
  }
  if (!pick) return;
  if (best < (pick.meeting ? 0.16 : 0.1)) return;
  speak(pick.role, pick.meeting);
}

export function chatterSays(role) {
  speak(role, true);
}

export function startChatter() {
  if (started) return;
  started = true;
  onStage((s) => { stage = s; });
  settings.onChange(() => { if (master) master.gain.value = volume(); });
  for (const kind of ["pointerdown", "keydown", "touchstart"]) {
    addEventListener(kind, wake, { passive: true });
  }
  ticker = setInterval(tick, 850);
  return () => { clearInterval(ticker); ticker = null; };
}
