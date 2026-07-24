let ctx = null;

function audio() {
  if (!ctx) ctx = new (window.AudioContext || window.webkitAudioContext)();
  if (ctx.state === "suspended") ctx.resume();
  return ctx;
}

export function play(p = {}) {
  const c = audio();
  const t0 = c.currentTime;
  const dur = Math.max(0.02, p.duration ?? 0.2);
  const type = p.type ?? "square";
  const vol = Math.min(1, p.volume ?? 0.25);

  const gain = c.createGain();
  gain.connect(c.destination);
  gain.gain.setValueAtTime(vol, t0);
  gain.gain.exponentialRampToValueAtTime(0.0001, t0 + dur + (p.release ?? 0.05));

  if (type === "noise") {
    const len = Math.ceil(c.sampleRate * (dur + 0.1));
    const buf = c.createBuffer(1, len, c.sampleRate);
    const data = buf.getChannelData(0);
    for (let i = 0; i < len; i++) data[i] = Math.random() * 2 - 1;
    const src = c.createBufferSource();
    src.buffer = buf;
    const filter = c.createBiquadFilter();
    filter.type = p.filter ?? "lowpass";
    filter.frequency.setValueAtTime(p.freq ?? 800, t0);
    if (p.slide) filter.frequency.exponentialRampToValueAtTime(Math.max(40, (p.freq ?? 800) * p.slide), t0 + dur);
    src.connect(filter);
    filter.connect(gain);
    src.start(t0);
    src.stop(t0 + dur + 0.1);
    return;
  }

  const osc = c.createOscillator();
  osc.type = type;
  osc.frequency.setValueAtTime(p.freq ?? 440, t0);
  if (p.slide) osc.frequency.exponentialRampToValueAtTime(Math.max(20, (p.freq ?? 440) * p.slide), t0 + dur);
  if (p.vibrato) {
    const lfo = c.createOscillator();
    const lfoGain = c.createGain();
    lfo.frequency.value = p.vibrato;
    lfoGain.gain.value = (p.freq ?? 440) * 0.06;
    lfo.connect(lfoGain);
    lfoGain.connect(osc.frequency);
    lfo.start(t0);
    lfo.stop(t0 + dur);
  }
  osc.connect(gain);
  osc.start(t0);
  osc.stop(t0 + dur + (p.release ?? 0.05));
}

export function arp(steps, p = {}) {
  const gap = p.gap ?? 0.07;
  steps.forEach((freq, i) =>
    setTimeout(() => play({ ...p, freq }), i * gap * 1000)
  );
}

export const presets = {
  jump: () => play({ type: "square", freq: 320, slide: 2.2, duration: 0.15, volume: 0.2 }),
  pickup: () => arp([660, 880, 1320], { type: "square", duration: 0.07, volume: 0.18 }),
  powerup: () => arp([330, 440, 550, 660, 880], { type: "triangle", duration: 0.09, volume: 0.2 }),
  hit: () => play({ type: "noise", freq: 900, slide: 0.25, duration: 0.12, volume: 0.3 }),
  explosion: () => play({ type: "noise", freq: 400, slide: 0.12, duration: 0.5, volume: 0.4 }),
  laser: () => play({ type: "sawtooth", freq: 1200, slide: 0.15, duration: 0.18, volume: 0.18 }),
  blip: () => play({ type: "square", freq: 700, duration: 0.05, volume: 0.15 }),
  fail: () => arp([440, 330, 220], { type: "sawtooth", duration: 0.12, volume: 0.2 }),
};
