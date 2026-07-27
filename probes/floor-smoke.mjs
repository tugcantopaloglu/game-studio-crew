import { readFileSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";
import { dirname, join, resolve } from "node:path";
import { loadFloorModules, stageModules, installDom, floorLayout, glRequests } from "./floor-dom.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "..");
const webDir = join(repo, "crates", "studio-server", "web");

const failures = [];

function check(what, ok, detail) {
  console.log(`  ${ok ? "ok  " : "FAIL"}  ${what}${detail ? "  " + detail : ""}`);
  if (!ok) failures.push(what);
}

const html = readFileSync(join(webDir, "floor.html"), "utf8");
const module = html.match(/<script type="module">([\s\S]*?)<\/script>/);

console.log("floor smoke check (static; no browser is involved)");

if (!module) {
  check("floor.html carries an inline module", false);
} else {
  const dir = stageModules(webDir);
  const body = module[1].replace(/(from\s+")\/([\w./-]+")/g, "$1./$2")
    .replace(/\.\/vendor\/three\.module\.js/g, "./vendor-three.module.js");
  const path = join(dir, "floor-module.mjs");
  writeFileSync(path, body);

  let parsed = true;
  try {
    execFileSync(process.execPath, ["--check", path], { stdio: "pipe" });
  } catch (err) {
    parsed = false;
    check("the floor module parses", false, String(err.stderr || err.message).split("\n")[0]);
  }
  if (parsed) check("the floor module parses", true);

  installDom();
  const wanted = new Map();
  for (const line of body.split(/\r?\n/)) {
    const m = line.match(/^import\s+\{([^}]+)\}\s+from\s+"\.\/([\w.-]+)"/);
    if (!m) continue;
    const names = m[1].split(",").map((s) => s.trim().split(/\s+as\s+/)[0]).filter(Boolean);
    const list = wanted.get(m[2]) || [];
    list.push(...names);
    wanted.set(m[2], list);
  }

  for (const [file, names] of wanted) {
    let exports = null;
    try {
      exports = await import(pathToFileURL(join(dir, file)).href);
    } catch (err) {
      check(`${file} loads`, false, err.message);
      continue;
    }
    const missing = names.filter((n) => !(n in exports));
    check(`${file} exports the ${names.length} names the floor imports`, missing.length === 0, missing.length ? "missing: " + missing.join(", ") : "");
  }
}

const { THREE, scene: sceneMod, perf, bus } = await loadFloorModules(webDir);
const settings = bus.settings;
const floor = await floorLayout(join(here, "out", "floor.json"), process.env.FLOOR_URL);
const { Scene } = await import(pathToFileURL(join(stageModules(webDir), "vendor-three.module.js")).href);

check("perf.js names a tier", typeof perf.tier === "function" && ["high", "low"].includes(perf.tier()), perf.tier());
check("perf.js publishes a budget", perf.budget && typeof perf.budget().screensPerFrame === "number");
check("perf.js answers about reduced motion", typeof perf.reducedMotion() === "boolean");

const built = sceneMod.buildOffice(floor, new Scene());
check("every desk in the layout got an avatar", built.avatars.size === floor.desks.length, `${built.avatars.size} of ${floor.desks.length}`);
check("the static fixtures are frozen out of the matrix walk", built.fixtures.matrixWorldAutoUpdate === false);
check("the screens collapsed to one surface per room", sceneMod.screenCount() === floor.rooms.length, `${sceneMod.screenCount()} surfaces, ${floor.rooms.length} rooms`);

const anyRig = [...built.avatars.values()][0].rig;
const joints = ["hips", "torso", "head", "armL", "armR", "thighL", "thighR", "shinL", "shinR"];
check("the nine-joint rig is intact", joints.every((j) => anyRig[j]), joints.filter((j) => !anyRig[j]).join(", "));
check("the prop is parented to the right hand", anyRig.prop && anyRig.armR.children.includes(anyRig.prop));

const a = [...built.avatars.values()][0];
sceneMod.setMotion(false);
a.person.position.set(9, 0.22, 9);
sceneMod.wanderStep(a, false, 1 / 60, 1);
check("reduced motion parks an avatar at its desk instead of walking", a.person.position.distanceTo(a.home) < 1e-6 && a.speed === 0);
check("reduced motion sits the crew down", sceneMod.avatarPose(a, "running", 1).sitting === true);

const drifter = built.ambient[0];
sceneMod.wanderStep(drifter, false, 1 / 60, 1);
check("reduced motion leaves the lobby crew standing, not sitting on air", sceneMod.avatarPose(drifter, "idle", 1).sitting === false);
sceneMod.setMotion(true);

const pose = sceneMod.avatarPose(a, "running", 1);
check("the pose object is reused rather than reallocated", sceneMod.avatarPose(a, "running", 2) === pose);

function rigIsFinite(rig) {
  const bad = [];
  for (const name of ["hips", "torso", "head", "armL", "armR", "thighL", "thighR", "shinL", "shinR", "prop"]) {
    const j = rig[name];
    if (!j) continue;
    for (const v of [j.position, j.rotation, j.scale]) {
      for (const axis of ["x", "y", "z"]) {
        if (!Number.isFinite(v[axis])) bad.push(`${name}.${axis}`);
      }
    }
  }
  for (const scalar of ["phase", "yaw", "sit", "lean", "headYaw", "headPitch", "owed"]) {
    if (!Number.isFinite(rig[scalar])) bad.push(scalar);
  }
  return bad;
}

const skipped = built.ambient[1] || built.ambient[0];
const step = 1 / 15;
let ran = 0;
for (let i = 0; i < 60; i++) {
  if (skipped.rig.update(sceneMod.avatarPose(skipped, "idle", i / 60), 1 / 60, i / 60, step)) ran++;
}
check("a throttled rig still runs, at about the rate it was asked for", ran >= 13 && ran <= 17, `${ran} updates in 60 frames at ${(1 / step).toFixed(0)}Hz`);

const poisoned = rigIsFinite(skipped.rig);
check(
  "a throttled rig keeps every joint finite, so an uninitialised frame debt cannot poison it",
  poisoned.length === 0,
  poisoned.length ? "not finite: " + poisoned.slice(0, 6).join(", ") : ""
);
check("an unthrottled rig runs every frame", built.ambient[0].rig.update(sceneMod.avatarPose(built.ambient[0], "idle", 1), 1 / 60, 1, 0) === true);

sceneMod.refreshScreens({ events: 1, tokens: 2, spend: 3, cacheRead: 4, cacheWrite: 5, history: [], feed: [], crewByDept: {} });
const firstPass = sceneMod.paintScreens(Infinity);
const secondPass = sceneMod.paintScreens(Infinity);
check("a screen with unchanged content is not repainted", firstPass > 0 && secondPass === 0, `${firstPass} then ${secondPass}`);

sceneMod.refreshScreens({ events: 9, tokens: 9, spend: 9, cacheRead: 9, cacheWrite: 9, history: [], feed: [], crewByDept: {} });
check("a screen budget of 2 paints at most 2 in a frame", sceneMod.paintScreens(2) <= 2);

glRequests.length = 0;
try {
  new THREE.WebGLRenderer({ powerPreference: "high-performance", failIfMajorPerformanceCaveat: true });
} catch (err) {
  void err;
}
const asked = glRequests.find((r) => r.attrs);
check(
  "the real three.js forwards powerPreference into canvas.getContext",
  Boolean(asked) && asked.attrs.powerPreference === "high-performance",
  asked ? `${asked.kind} powerPreference=${asked.attrs.powerPreference} failIfMajorPerformanceCaveat=${asked.attrs.failIfMajorPerformanceCaveat}` : "no context request recorded"
);

function fakeThree(failFirst) {
  const seen = [];
  return {
    seen,
    WebGLRenderer: class {
      constructor(attrs) {
        seen.push(attrs);
        if (failFirst && attrs.failIfMajorPerformanceCaveat) {
          throw new Error("Error creating WebGL context with your selected attributes.");
        }
      }
    },
  };
}

const tierNow = perf.budget();

const granted = fakeThree(false);
const grantedState = perf.createRenderer(granted, tierNow);
check("acceleration on asks for a high-performance context first", granted.seen[0].powerPreference === "high-performance" && granted.seen[0].failIfMajorPerformanceCaveat === true);
check("a granted context is reported as accelerated with no caveat", grantedState.caveat === false && grantedState.tries === 1);
check("a granted context reads back as accelerated", perf.hardwareHints(null).accelerated === true);

const refused = fakeThree(true);
const refusedState = perf.createRenderer(refused, tierNow);
check(
  "a refused high-performance context retries without the caveat flag rather than failing",
  refused.seen.length === 2 && refused.seen[1].failIfMajorPerformanceCaveat === false && refusedState.renderer !== null,
  `${refused.seen.length} attempts`
);
check("a refused context is reported as a software fallback", refusedState.caveat === true && perf.hardwareHints(null).software === true);
check("a software fallback offers low spec immediately rather than after 240 frames", perf.shouldOfferLowSpec(perf.hardwareHints(null)) === true);

settings.set("gpu.acceleration", false);
check("turning acceleration off asks for low power", perf.contextAttempts(perf.gpuWanted())[0].powerPreference === "low-power");
check("turning acceleration off makes exactly one attempt", perf.contextAttempts(perf.gpuWanted()).length === 1);
check("flipping the setting after the renderer exists asks for a reload", perf.gpuNeedsReload() === true);
const offState = perf.createRenderer(fakeThree(false), tierNow);
check("acceleration off is not reported as accelerated", offState.wantGpu === false && perf.hardwareHints(null).accelerated === false);
check("a rebuilt renderer no longer asks for a reload", perf.gpuNeedsReload() === false);
settings.set("gpu.acceleration", true);

console.log("");
if (failures.length) {
  console.log(`${failures.length} check(s) failed`);
  process.exitCode = 1;
} else {
  console.log("all checks passed");
}
