import { settings } from "/bus.js";

const HIGH = {
  name: "high",
  pixelRatio: 2,
  shadows: true,
  shadowMap: 4096,
  softShadows: true,
  roomLights: true,
  deskLights: true,
  ambient: 0.25,
  hemisphere: 0.9,
  fog: true,
  ambientCrew: 5,
  screensPerFrame: 6,
  screenPeriod: 0.4,
  minimapPeriod: 0.25,
  farRigPeriod: 0,
  farRigDistance: 34,
  bubbleFade: 6,
};

const LOW = {
  name: "low",
  pixelRatio: 1,
  shadows: false,
  shadowMap: 1024,
  softShadows: false,
  roomLights: false,
  deskLights: false,
  ambient: 0.42,
  hemisphere: 1.2,
  fog: true,
  ambientCrew: 2,
  screensPerFrame: 1,
  screenPeriod: 1.6,
  minimapPeriod: 0.6,
  farRigPeriod: 1 / 15,
  farRigDistance: 18,
  bubbleFade: 6,
};

const STRUGGLE_P95 = 26;
const STRUGGLE_FRAMES = 240;
const SAMPLE_CAP = 180;

const tierTaps = new Set();
const samples = [];
let cursor = 0;
let counted = 0;
let offered = false;

export function budget() {
  return settings.get("lowSpec") ? LOW : HIGH;
}

export function tier() {
  return budget().name;
}

export function reducedMotion() {
  if (typeof matchMedia !== "function") return false;
  try {
    return matchMedia("(prefers-reduced-motion: reduce)").matches;
  } catch (err) {
    return false;
  }
}

export function onTier(fn) {
  tierTaps.add(fn);
  return () => tierTaps.delete(fn);
}

settings.onChange((key) => {
  if (key !== null && key !== "lowSpec") return;
  const b = budget();
  for (const fn of tierTaps) {
    try {
      fn(b);
    } catch (err) {
      continue;
    }
  }
});

export function sampleFrame(ms) {
  samples[cursor] = ms;
  cursor = (cursor + 1) % SAMPLE_CAP;
  counted++;
}

export function frameP95() {
  if (!samples.length) return 0;
  const sorted = [...samples].sort((a, b) => a - b);
  const at = Math.min(sorted.length - 1, Math.max(0, Math.ceil(0.95 * sorted.length) - 1));
  return sorted[at];
}

export function frameMean() {
  if (!samples.length) return 0;
  return samples.reduce((a, b) => a + b, 0) / samples.length;
}

export function hardwareHints(renderer) {
  const cores = Number(navigator.hardwareConcurrency) || 0;
  const memory = Number(navigator.deviceMemory) || 0;
  let gpu = "";
  try {
    const gl = renderer && renderer.getContext ? renderer.getContext() : null;
    const info = gl && gl.getExtension("WEBGL_debug_renderer_info");
    if (info) gpu = String(gl.getParameter(info.UNMASKED_RENDERER_WEBGL) || "");
  } catch (err) {
    gpu = "";
  }
  const software = /swiftshader|basic render|llvmpipe|software/i.test(gpu);
  const weak = software || (cores > 0 && cores <= 4) || (memory > 0 && memory <= 4);
  return { cores, memory, gpu, software, weak };
}

export function struggling() {
  if (counted < STRUGGLE_FRAMES || counted % 60 !== 0) return false;
  return frameP95() > STRUGGLE_P95;
}

export function shouldOfferLowSpec() {
  if (offered || settings.get("lowSpec")) return false;
  if (!struggling()) return false;
  offered = true;
  return true;
}

export function noteOffered() {
  offered = true;
}

export function reasonForOffer(hints) {
  const p95 = frameP95().toFixed(0);
  if (hints && hints.software) return `the browser is drawing this in software and a frame is taking ${p95}ms`;
  if (hints && hints.gpu) return `frames are taking ${p95}ms on ${hints.gpu.slice(0, 46)}`;
  return `frames are taking ${p95}ms, so the floor is running under 40fps`;
}
