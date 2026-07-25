import { mkdtempSync, readFileSync, writeFileSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

export const canvasOps = { contexts: 0, fills: 0, texts: 0, strokes: 0, clears: 0 };

function ctx2d() {
  canvasOps.contexts++;
  const noop = () => {};
  return {
    fillStyle: "#000", strokeStyle: "#000", font: "", lineWidth: 1, textBaseline: "top",
    globalAlpha: 1, lineCap: "butt", lineJoin: "miter",
    fillRect: () => { canvasOps.fills++; },
    strokeRect: () => { canvasOps.strokes++; },
    clearRect: () => { canvasOps.clears++; },
    fillText: () => { canvasOps.texts++; },
    strokeText: () => { canvasOps.texts++; },
    measureText: (s) => ({ width: String(s).length * 24 }),
    beginPath: noop, closePath: noop, moveTo: noop, lineTo: noop, arc: noop, arcTo: noop,
    rect: noop, save: noop, restore: noop, translate: noop, rotate: noop, scale: noop,
    setTransform: noop, drawImage: noop, putImageData: noop,
    fill: () => { canvasOps.fills++; },
    stroke: () => { canvasOps.strokes++; },
    createLinearGradient: () => ({ addColorStop: noop }),
    createRadialGradient: () => ({ addColorStop: noop }),
    getImageData: (x, y, w, h) => ({ data: new Uint8ClampedArray(w * h * 4), width: w, height: h }),
  };
}

function element(tag) {
  const node = {
    tagName: String(tag).toUpperCase(),
    width: 300, height: 150, style: {}, children: [],
    nodeType: 1, className: "", id: "", textContent: "", innerHTML: "",
    getContext: (kind) => (kind === "2d" ? ctx2d() : null),
    setAttribute: noop2, getAttribute: () => null, removeAttribute: noop2,
    addEventListener: noop2, removeEventListener: noop2, dispatchEvent: () => true,
    appendChild: (c) => { node.children.push(c); return c; },
    append: (...c) => { node.children.push(...c); },
    removeChild: (c) => c, insertBefore: (c) => c, remove: noop2,
    querySelector: () => null, querySelectorAll: () => [],
    getBoundingClientRect: () => ({ left: 0, top: 0, width: 1600, height: 900, right: 1600, bottom: 900 }),
    classList: { add: noop2, remove: noop2, toggle: noop2, contains: () => false },
    focus: noop2, blur: noop2, click: noop2,
    toDataURL: () => "data:,",
  };
  return node;
}

function noop2() {}

export function installDom() {
  const body = element("body");
  const documentStub = {
    body,
    documentElement: element("html"),
    createElement: element,
    createElementNS: (_ns, tag) => element(tag),
    createTextNode: (t) => ({ nodeType: 3, textContent: t }),
    getElementById: () => null,
    querySelector: () => null,
    querySelectorAll: () => [],
    addEventListener: noop2,
    removeEventListener: noop2,
  };
  globalThis.document = documentStub;
  globalThis.window = globalThis;
  globalThis.self = globalThis;
  globalThis.devicePixelRatio = 1;
  globalThis.innerWidth = 1600;
  globalThis.innerHeight = 900;
  globalThis.addEventListener = noop2;
  globalThis.removeEventListener = noop2;
  globalThis.requestAnimationFrame = (fn) => setTimeout(() => fn(performance.now()), 0);
  globalThis.cancelAnimationFrame = clearTimeout;
  globalThis.matchMedia = (q) => ({ matches: false, media: q, addEventListener: noop2, removeEventListener: noop2 });
  globalThis.localStorage = {
    store: new Map(),
    getItem(k) { return this.store.has(k) ? this.store.get(k) : null; },
    setItem(k, v) { this.store.set(k, String(v)); },
    removeItem(k) { this.store.delete(k); },
    clear() { this.store.clear(); },
  };
  globalThis.fetch = () => Promise.reject(new Error("no network in the probe"));
  return documentStub;
}

export function stageModules(webDir) {
  const dir = mkdtempSync(join(tmpdir(), "floor-probe-"));
  for (const name of readdirSync(webDir)) {
    if (!name.endsWith(".js")) continue;
    const src = readFileSync(join(webDir, name), "utf8").replace(/(from\s+")\/([\w./-]+")/g, "$1./$2");
    writeFileSync(join(dir, name), src);
  }
  const vendorSrc = readFileSync(join(webDir, "vendor", "three.module.js"), "utf8");
  writeFileSync(join(dir, "vendor-three.module.js"), vendorSrc);
  for (const name of readdirSync(dir)) {
    if (name === "vendor-three.module.js") continue;
    const p = join(dir, name);
    writeFileSync(p, readFileSync(p, "utf8").replace(/\.\/vendor\/three\.module\.js/g, "./vendor-three.module.js"));
  }
  return dir;
}

export async function loadFloorModules(webDir) {
  installDom();
  const dir = stageModules(webDir);
  const url = (n) => pathToFileURL(join(dir, n)).href;
  const THREE = await import(url("vendor-three.module.js"));
  const scene = await import(url("scene.js"));
  const avatar = await import(url("avatar.js"));
  const voxel = await import(url("voxel.js"));
  let perf = null;
  try {
    perf = await import(url("perf.js"));
  } catch (err) {
    perf = null;
  }
  return { THREE, scene, avatar, voxel, perf, dir };
}

export function percentile(samples, p) {
  const sorted = [...samples].sort((a, b) => a - b);
  const at = Math.min(sorted.length - 1, Math.max(0, Math.ceil((p / 100) * sorted.length) - 1));
  return sorted[at];
}

export function ms(v) {
  return v.toFixed(3).padStart(8);
}
