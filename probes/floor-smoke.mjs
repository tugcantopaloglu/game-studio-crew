import { readFileSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";
import { dirname, join, resolve } from "node:path";
import { loadFloorModules, stageModules, installDom, floorLayout } from "./floor-dom.mjs";

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

const { scene: sceneMod, perf } = await loadFloorModules(webDir);
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

sceneMod.refreshScreens({ events: 1, tokens: 2, spend: 3, cacheRead: 4, cacheWrite: 5, history: [], feed: [], crewByDept: {} });
const firstPass = sceneMod.paintScreens(Infinity);
const secondPass = sceneMod.paintScreens(Infinity);
check("a screen with unchanged content is not repainted", firstPass > 0 && secondPass === 0, `${firstPass} then ${secondPass}`);

sceneMod.refreshScreens({ events: 9, tokens: 9, spend: 9, cacheRead: 9, cacheWrite: 9, history: [], feed: [], crewByDept: {} });
check("a screen budget of 2 paints at most 2 in a frame", sceneMod.paintScreens(2) <= 2);

console.log("");
if (failures.length) {
  console.log(`${failures.length} check(s) failed`);
  process.exitCode = 1;
} else {
  console.log("all checks passed");
}
