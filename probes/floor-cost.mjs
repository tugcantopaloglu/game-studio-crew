import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";
import { loadFloorModules, checkoutWeb, percentile, canvasOps } from "./floor-dom.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "..");
const rev = process.env.REV || "";
const webDir = rev
  ? checkoutWeb(rev, join(repo, "crates", "studio-server", "web"), repo)
  : join(repo, "crates", "studio-server", "web");
const floorPath = process.env.FLOOR_JSON || join(here, "out", "floor.json");
const FRAMES = Number(process.env.FRAMES || 900);
const WARMUP = Number(process.env.WARMUP || 120);

const RING = { idle: 0x2f3644, running: 0x4ad991, blocked: 0xd9c24a, meeting: 0x6fa8d1, error: 0xd95555 };
const RINGS = ["idle", "running", "running", "meeting", "blocked", "error", "running", "idle"];

const lowSpec = process.env.LOW_SPEC === "1";
const { THREE, scene: sceneMod, perf } = await loadFloorModules(webDir, { lowSpec });
const tier = perf && perf.budget ? perf.budget() : { name: "high", screensPerFrame: 999, screenPeriod: 0.4, minimapPeriod: 0.25, farRigPeriod: 0, farRigDistance: 34 };
const floor = JSON.parse(readFileSync(floorPath, "utf8"));

const scene = new THREE.Scene();
const camera = new THREE.PerspectiveCamera(34, 16 / 9, 0.1, 400);
camera.position.set(24, 30, 24);
camera.lookAt(0, 0.5, 0);
camera.updateMatrixWorld(true);

const buildStart = performance.now();
const built = sceneMod.buildOffice(floor, scene);
const buildMs = performance.now() - buildStart;

const avatars = built.avatars;
const ambient = built.ambient || [];
const surfaces = sceneMod.screenCount ? sceneMod.screenCount() : 0;

const state = new Map();
[...avatars.keys()].forEach((role, i) => {
  state.set(role, { ring: RINGS[i % RINGS.length], tokens: 1200 + i * 90, log: [], tool: "Read", summary: "" });
});
const deskState = (role) => state.get(role) || { ring: "idle", tokens: 0, log: [] };

const stats = {
  run: "run_probe", events: 4210, tokens: 918_233, spend: 3.9214,
  cacheRead: 88_670, cacheWrite: 8_867, symbols: 4102,
  history: Array.from({ length: 26 }, (_, i) => (i % 9) / 9),
  feed: Array.from({ length: 12 }, (_, i) => ({ seq: 4200 + i, type: "tool_call", bad: false })),
};

function screenData() {
  const crewByDept = {};
  for (const [role, a] of avatars) {
    (crewByDept[a.dept] ||= []).push({
      role,
      tokens: deskState(role).tokens,
      color: "#" + (RING[deskState(role).ring] || RING.idle).toString(16).padStart(6, "0"),
    });
  }
  const tot = stats.cacheRead + stats.cacheWrite;
  return {
    run: stats.run, events: stats.events, tokens: stats.tokens, spend: stats.spend,
    cacheRead: stats.cacheRead, cacheWrite: stats.cacheWrite,
    cacheHit: tot ? (stats.cacheRead / tot) * 100 : null,
    history: stats.history, feed: stats.feed, crewByDept,
  };
}

const mini = { width: 150, height: 98 };
const mctx = document.createElement("canvas").getContext("2d");
function renderMinimap() {
  const sx = mini.width / floor.width, sy = mini.height / floor.height;
  mctx.clearRect(0, 0, mini.width, mini.height);
  for (const r of floor.rooms.concat([floor.lobby])) {
    mctx.fillStyle = r.department === "lobby" ? "rgba(148,163,184,.16)" : "rgba(148,163,184,.09)";
    mctx.fillRect(r.x * sx, r.y * sy, r.w * sx, r.h * sy);
    mctx.strokeStyle = "rgba(148,163,184,.22)";
    mctx.strokeRect(r.x * sx, r.y * sy, r.w * sx, r.h * sy);
  }
  for (const d of floor.desks) {
    const st = deskState(d.role);
    mctx.fillStyle = "#" + (RING[st.ring] || RING.idle).toString(16).padStart(6, "0");
    mctx.fillRect(d.x * sx - 1, d.y * sy - 1, Math.max(3, d.w * sx), Math.max(3, d.h * sy));
  }
}

function census() {
  let meshes = 0, shadowCasters = 0, instanced = 0, instances = 0, tris = 0, shadowTris = 0;
  const materials = new Set(), geometries = new Set(), textures = new Set();
  const lights = { point: 0, spot: 0, directional: 0, other: 0 };
  scene.traverse((o) => {
    if (o.isLight) {
      if (o.isPointLight) lights.point++;
      else if (o.isSpotLight) lights.spot++;
      else if (o.isDirectionalLight) lights.directional++;
      else lights.other++;
      return;
    }
    if (!o.isMesh) return;
    const g = o.geometry;
    const per = g.index ? g.index.count / 3 : g.attributes.position.count / 3;
    const n = o.isInstancedMesh ? o.count : 1;
    meshes++;
    if (o.isInstancedMesh) { instanced++; instances += o.count; }
    tris += per * n;
    if (o.castShadow) { shadowCasters++; shadowTris += per * n; }
    geometries.add(g);
    for (const m of Array.isArray(o.material) ? o.material : [o.material]) {
      materials.add(m);
      if (m.map) textures.add(m.map);
    }
  });
  return { meshes, shadowCasters, instanced, instances, tris, shadowTris, materials: materials.size, geometries: geometries.size, textures: textures.size, lights };
}

let screensDirty = true;
let lastScreenPaint = 0;
let lastMini = 0;
let miniDirty = true;
let painted = 0;
let t = 0;

const camPos = camera.position;
function rigStep(a) {
  if (!tier.farRigPeriod) return 0;
  return a.person.position.distanceTo(camPos) > tier.farRigDistance ? tier.farRigPeriod : 0;
}

function frame(dt) {
  const marks = {};
  let mark = performance.now();

  for (const [role, a] of avatars) {
    const st = deskState(role);
    const stuck = st.ring === "error" || st.ring === "blocked";
    const busy = st.ring === "running" || st.ring === "meeting" || stuck;
    sceneMod.wanderStep(a, busy, dt, t);
    a.rig.update(sceneMod.avatarPose(a, st.ring, t), dt, t, rigStep(a));

    a.ringMat.opacity = stuck
      ? 0.55 + Math.sin(t * 7 + a.seed) * 0.45
      : st.ring === "idle" ? 0.28 : 0.7 + Math.sin(t * 4 + a.seed) * 0.3;
    if (a.alarm) a.alarm.intensity = stuck ? 2.6 + Math.sin(t * 7 + a.seed) * 2.0 : 0;

    const lit = st.ring === "running" || st.ring === "meeting";
    if (a.lamp) {
      a.lamp.visible = lit || stuck;
      if (a.lamp.visible) {
        const tint = stuck ? RING.error : RING[st.ring];
        a.bulb.material.color.setHex(tint);
        a.cone.material.color.setHex(tint);
        a.pool.material.color.setHex(tint);
        const w = 0.5 + Math.sin(t * (stuck ? 7 : 3.4) + a.seed) * 0.5;
        a.bulb.material.opacity = 0.6 + w * 0.4;
        a.bulb.scale.setScalar(1 + w * 0.2);
        a.cone.material.opacity = 0.11 + w * 0.10;
        a.pool.material.opacity = 0.14 + w * 0.12;
        if (a.spot) {
          a.spot.color.setHex(tint);
          a.spot.intensity = 22 + w * 22;
        }
      } else if (a.spot) {
        a.spot.intensity = 0;
      }
    }
  }
  marks.crew = performance.now() - mark;
  mark = performance.now();

  for (const a of ambient) {
    sceneMod.wanderStep(a, false, dt, t);
    a.rig.update(sceneMod.avatarPose(a, "idle", t), dt, t, rigStep(a));
  }
  marks.ambient = performance.now() - mark;
  mark = performance.now();

  if (miniDirty && t - lastMini > tier.minimapPeriod) {
    renderMinimap();
    miniDirty = false;
    lastMini = t;
  }
  marks.minimap = performance.now() - mark;
  mark = performance.now();

  if (screensDirty && t - lastScreenPaint > tier.screenPeriod) {
    const marked = sceneMod.refreshScreens(screenData());
    if (!sceneMod.paintScreens) painted += marked || surfaces;
    screensDirty = false;
    lastScreenPaint = t;
  }
  if (sceneMod.paintScreens) painted += sceneMod.paintScreens(tier.screensPerFrame);
  marks.screens = performance.now() - mark;
  mark = performance.now();

  scene.updateMatrixWorld();
  marks.matrices = performance.now() - mark;

  return marks;
}

const PHASES = ["crew", "ambient", "minimap", "screens", "matrices"];
const samples = { total: [] };
for (const p of PHASES) samples[p] = [];

function churn(i) {
  stats.events++;
  stats.tokens += 137;
  stats.spend += 0.0012;
  screensDirty = true;
  if (i % 40 === 0) {
    const roles = [...avatars.keys()];
    const role = roles[i % roles.length];
    state.get(role).ring = RINGS[(i / 40 + 3) % RINGS.length | 0];
    miniDirty = true;
  }
}

for (let i = 0; i < WARMUP; i++) {
  t += 1 / 60;
  churn(i);
  frame(1 / 60);
}

if (globalThis.gc) globalThis.gc();
const heapBefore = process.memoryUsage().heapUsed;
const opsBefore = { ...canvasOps };
painted = 0;

for (let i = 0; i < FRAMES; i++) {
  t += 1 / 60;
  churn(i);
  const start = performance.now();
  const marks = frame(1 / 60);
  samples.total.push(performance.now() - start);
  for (const p of PHASES) samples[p].push(marks[p]);
}

if (globalThis.gc) globalThis.gc();
const heapAfter = process.memoryUsage().heapUsed;
const c = census();

const label = process.env.LABEL || (perf && perf.tier ? perf.tier() : "unknown");
console.log(`floor cost probe   tier=${label}   frames=${FRAMES}   node=${process.version}`);
console.log(`floor: ${floor.rooms.length} rooms, ${floor.desks.length} desks, ${(floor.spares || []).length} spares, ${floor.width}x${floor.height}`);
console.log(`build: ${buildMs.toFixed(1)}ms for the whole office graph`);
console.log("");
console.log("scene census (GPU-independent)");
console.log(`  renderable meshes      ${c.meshes}`);
console.log(`  of those, shadow-casting ${c.shadowCasters}`);
console.log(`  instanced meshes       ${c.instanced} carrying ${c.instances} instances`);
console.log(`  triangles              ${Math.round(c.tris).toLocaleString()}`);
console.log(`  triangles per shadow pass ${Math.round(c.shadowTris).toLocaleString()}`);
console.log(`  distinct materials     ${c.materials}`);
console.log(`  distinct geometries    ${c.geometries}`);
console.log(`  canvas textures        ${c.textures}`);
console.log(`  lights                 ${c.lights.point} point, ${c.lights.spot} spot, ${c.lights.directional} directional`);
console.log("");
console.log("cpu time inside the animate loop, excluding renderer.render (no GPU here)");
console.log("  phase           mean       p50       p95       max");
for (const p of ["crew", "ambient", "minimap", "screens", "matrices"]) {
  const s = samples[p];
  const mean = s.reduce((a, b) => a + b, 0) / s.length;
  console.log(
    `  ${p.padEnd(14)}${mean.toFixed(3).padStart(8)}  ${percentile(s, 50).toFixed(3).padStart(8)}  ` +
    `${percentile(s, 95).toFixed(3).padStart(8)}  ${Math.max(...s).toFixed(3).padStart(8)}`
  );
}
const tot = samples.total;
const mean = tot.reduce((a, b) => a + b, 0) / tot.length;
console.log(
  `  ${"TOTAL".padEnd(14)}${mean.toFixed(3).padStart(8)}  ${percentile(tot, 50).toFixed(3).padStart(8)}  ` +
  `${percentile(tot, 95).toFixed(3).padStart(8)}  ${Math.max(...tot).toFixed(3).padStart(8)}`
);
console.log("");
console.log("allocation and 2d canvas work over those frames");
console.log(`  heap retained after gc ${((heapAfter - heapBefore) / 1024).toFixed(0)} KiB total, ${((heapAfter - heapBefore) / FRAMES).toFixed(1)} B per frame`);
console.log(`  screen repaints        ${painted} (${(painted / FRAMES).toFixed(2)} per frame, ${surfaces} surfaces)`);
console.log(`  canvas fillRect/fill   ${canvasOps.fills - opsBefore.fills} (${((canvasOps.fills - opsBefore.fills) / FRAMES).toFixed(1)} per frame)`);
console.log(`  canvas fillText        ${canvasOps.texts - opsBefore.texts} (${((canvasOps.texts - opsBefore.texts) / FRAMES).toFixed(1)} per frame)`);
console.log(`  canvas clearRect       ${canvasOps.clears - opsBefore.clears} (${((canvasOps.clears - opsBefore.clears) / FRAMES).toFixed(1)} per frame)`);
