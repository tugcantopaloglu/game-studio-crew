import { settings } from "/bus.js";

const HIGH = {
  name: "high",
  pixelRatio: 2,
  shadows: true,
  postFX: true,
  shadowMap: 4096,
  softShadows: true,
  roomLights: true,
  deskLights: true,
  ambient: 0.15,
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
  postFX: false,
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
const SOFTWARE = /swiftshader|basic render|llvmpipe|software|microsoft basic/i;

const tierTaps = new Set();
const gpuTaps = new Set();
const motionTaps = new Set();
const samples = [];
let cursor = 0;
let counted = 0;
let offered = false;
let render = null;
let builtWithGpu = null;

export function budget() {
  return settings.get("lowSpec") ? LOW : HIGH;
}

export function tier() {
  return budget().name;
}

export function osReducedMotion() {
  if (typeof matchMedia !== "function") return false;
  try {
    return matchMedia("(prefers-reduced-motion: reduce)").matches;
  } catch (err) {
    return false;
  }
}

export function crewMoves() {
  const want = settings.get("motion.crew", "auto");
  if (want === "on") return true;
  if (want === "off") return false;
  return !osReducedMotion();
}

export function reducedMotion() {
  return !crewMoves();
}

export function onMotion(fn) {
  motionTaps.add(fn);
  return () => motionTaps.delete(fn);
}

export function onTier(fn) {
  tierTaps.add(fn);
  return () => tierTaps.delete(fn);
}

export function onGpu(fn) {
  gpuTaps.add(fn);
  return () => gpuTaps.delete(fn);
}

function fanout(taps, arg) {
  for (const fn of taps) {
    try {
      fn(arg);
    } catch (err) {
      continue;
    }
  }
}

settings.onChange((key) => {
  if (key === null || key === "lowSpec") fanout(tierTaps, budget());
  if (key === null || key === "gpu.acceleration") fanout(gpuTaps, gpuNeedsReload());
  if (key === null || key === "motion.crew") fanout(motionTaps, crewMoves());
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

export function gpuWanted() {
  return settings.get("gpu.acceleration", true) !== false;
}

export function contextAttempts(wantGpu) {
  if (!wantGpu) {
    return [{ powerPreference: "low-power", failIfMajorPerformanceCaveat: false }];
  }
  return [
    { powerPreference: "high-performance", failIfMajorPerformanceCaveat: true },
    { powerPreference: "high-performance", failIfMajorPerformanceCaveat: false },
  ];
}

export function createRenderer(THREE, tier) {
  const wantGpu = gpuWanted();
  const attempts = contextAttempts(wantGpu);
  let failure = null;

  for (let i = 0; i < attempts.length; i++) {
    try {
      const renderer = new THREE.WebGLRenderer({
        antialias: tier.pixelRatio > 1,
        powerPreference: attempts[i].powerPreference,
        failIfMajorPerformanceCaveat: attempts[i].failIfMajorPerformanceCaveat,
      });
      builtWithGpu = wantGpu;
      render = {
        renderer,
        wantGpu,
        powerPreference: attempts[i].powerPreference,
        caveat: i > 0,
        tries: i + 1,
      };
      return render;
    } catch (err) {
      failure = err;
    }
  }

  builtWithGpu = wantGpu;
  render = { renderer: null, wantGpu, powerPreference: null, caveat: true, tries: attempts.length };
  throw failure || new Error("no WebGL context");
}

export function renderState() {
  return render;
}

export function gpuNeedsReload() {
  return builtWithGpu !== null && builtWithGpu !== gpuWanted();
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
  const caveat = Boolean(render && render.caveat);
  const software = SOFTWARE.test(gpu) || (caveat && !gpu);
  const accelerated = Boolean(render && render.wantGpu && !render.caveat);
  const weak = software || (cores > 0 && cores <= 4) || (memory > 0 && memory <= 4);
  return { cores, memory, gpu, software, caveat, accelerated, weak };
}

export function describeRenderer(hints) {
  if (!render) return "no renderer";
  if (!render.wantGpu) return "hardware acceleration off, asking for low power";
  const where = hints && hints.gpu ? hints.gpu.slice(0, 46) : "an unnamed device";
  if (render.caveat) return `the browser refused a high-performance context and fell back to ${where}`;
  return `high-performance context granted on ${where}`;
}

export function struggling() {
  if (counted < STRUGGLE_FRAMES || counted % 60 !== 0) return false;
  return frameP95() > STRUGGLE_P95;
}

export function shouldOfferLowSpec(hints) {
  if (offered || settings.get("lowSpec")) return false;
  const knownSoftware = Boolean(hints && hints.software);
  if (!knownSoftware && !struggling()) return false;
  offered = true;
  return true;
}

export function noteOffered() {
  offered = true;
}

export function reasonForOffer(hints) {
  if (hints && hints.software) {
    return hints.gpu
      ? `the browser is drawing this on ${hints.gpu.slice(0, 40)}, which is software rendering`
      : "the browser refused a hardware-accelerated context, so this is being drawn in software";
  }
  const p95 = frameP95().toFixed(0);
  if (hints && hints.gpu) return `frames are taking ${p95}ms on ${hints.gpu.slice(0, 46)}`;
  return `frames are taking ${p95}ms, so the floor is running under 40fps`;
}
