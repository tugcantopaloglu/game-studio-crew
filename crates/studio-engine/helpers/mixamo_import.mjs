import { readFile, writeFile, mkdir } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { resolve, dirname, basename } from "node:path";

import * as THREE from "./vendor/three.module.js";
import { GLTFExporter } from "./vendor/GLTFExporter.js";
import { FBXLoader } from "./vendor/FBXLoader.js";
import { mapping, retargetClip, strip } from "./retarget.mjs";

const args = process.argv.slice(2);
const flags = new Map();
const plain = [];
for (let i = 0; i < args.length; i++) {
  if (args[i].startsWith("--")) {
    flags.set(args[i].slice(2), args[i + 1] && !args[i + 1].startsWith("--") ? args[++i] : "1");
  } else {
    plain.push(args[i]);
  }
}
const [factoryPath, animationPath, outPath] = plain;
if (!factoryPath || !animationPath || !outPath) {
  console.error(
    "usage: node mixamo_import.mjs <factory.js> <animation.fbx> <out.glb> [--name walk] [--fps 30]"
  );
  process.exit(2);
}

globalThis.THREE = THREE;
if (typeof globalThis.FileReader === "undefined") {
  globalThis.FileReader = class {
    readAsArrayBuffer(blob) {
      blob.arrayBuffer().then((buf) => {
        this.result = buf;
        if (this.onloadend) this.onloadend();
      });
    }
    readAsDataURL(blob) {
      blob.arrayBuffer().then((buf) => {
        this.result =
          "data:" +
          (blob.type || "application/octet-stream") +
          ";base64," +
          Buffer.from(buf).toString("base64");
        if (this.onloadend) this.onloadend();
      });
    }
  };
}

const mod = await import(pathToFileURL(resolve(factoryPath)).href);
const factory =
  typeof mod.default === "function"
    ? mod.default
    : Object.values(mod).find((v) => typeof v === "function");
if (!factory) {
  console.error(`${factoryPath} exports no factory function`);
  process.exit(2);
}

let model = factory(THREE);
if (model && typeof model.then === "function") model = await model;
if (model && model.group instanceof THREE.Object3D) model = model.group;
if (!(model instanceof THREE.Object3D)) {
  console.error("the factory did not return a THREE.Object3D");
  process.exit(2);
}
model.updateMatrixWorld(true);

let held = [];
if (typeof mod.clips === "function") {
  const asked = await mod.clips(THREE, model);
  if (Array.isArray(asked)) held = asked;
}

const raw = await readFile(resolve(animationPath));
const buffer = raw.buffer.slice(raw.byteOffset, raw.byteOffset + raw.byteLength);

let loaded;
try {
  loaded = new FBXLoader().parse(buffer, dirname(resolve(animationPath)) + "/");
} catch (err) {
  console.error(`${animationPath} could not be read as an fbx: ${err.message}`);
  process.exit(2);
}

const GENERIC = /^(mixamo\.com|take ?\d+|armature.*|unnamed.*)$/i;

function moves(track) {
  const keys = track.times ? track.times.length : 0;
  if (keys < 2 || !track.values || !track.values.length) return false;
  const stride = track.values.length / keys;
  for (let i = stride; i < track.values.length; i++) {
    if (Math.abs(track.values[i] - track.values[i % stride]) > 1e-6) return true;
  }
  return false;
}

function nameFor(clip, at, index) {
  const given = String(clip.name || "").trim();
  const bare = given.split("|").pop().trim();
  if (bare && !GENERIC.test(bare)) return bare;
  const stem = basename(at)
    .replace(/\.[^.]+$/, "")
    .replace(/[^a-z0-9]+/gi, "_")
    .replace(/^_+|_+$/g, "")
    .toLowerCase();
  return index === 0 ? stem || "mixamo" : `${stem || "mixamo"}_${index}`;
}

const clips = (loaded.animations || []).filter(
  (clip) => clip && clip.duration > 0 && clip.tracks.length && clip.tracks.some(moves)
);
if (!clips.length) {
  console.error(
    `${basename(animationPath)} carries no animation; download it from mixamo with an animation ` +
      `selected, and "without skin" is enough`
  );
  process.exit(2);
}

const pairs = mapping(loaded, model);
if (!pairs.length) {
  const theirs = [];
  loaded.traverse((n) => {
    if (n.name && theirs.length < 12) theirs.push(strip(n.name));
  });
  console.error(
    `none of the bones in ${basename(animationPath)} match a joint in this model; it names ` +
      `${theirs.join(", ")} and the model has no joint the studio knows those by`
  );
  process.exit(2);
}

const fps = Number(flags.get("fps") || 30);
const wanted = flags.get("name");
const landed = [];
for (const clip of clips) {
  const name = wanted || nameFor(clip, animationPath, landed.length);
  const made = retargetClip(THREE, {
    sourceRoot: loaded,
    targetRoot: model,
    clip,
    pairs,
    fps,
    name,
  });
  if (!made.tracks.some(moves)) {
    console.error(
      `${name} came back holding still, so the animation and this rig share no joint that moves`
    );
    process.exit(2);
  }
  landed.push(made);
  if (wanted) break;
}

const kept = held.filter((clip) => !landed.some((one) => one.name === clip.name));
const all = [...kept, ...landed];

model.updateMatrixWorld(true);
const exporter = new GLTFExporter();
const glb = await exporter.parseAsync(model, {
  binary: true,
  onlyVisible: false,
  animations: all,
});
const bytes = glb instanceof ArrayBuffer ? Buffer.from(glb) : Buffer.from(JSON.stringify(glb));
await mkdir(dirname(resolve(outPath)), { recursive: true });
await writeFile(resolve(outPath), bytes);

let meshes = 0;
model.traverse((o) => {
  if (o.isMesh) meshes++;
});

const sidecar = flags.get("clips");
if (sidecar && sidecar !== "1") {
  await mkdir(dirname(resolve(sidecar)), { recursive: true });
  await writeFile(
    resolve(sidecar),
    JSON.stringify(all.map((clip) => THREE.AnimationClip.toJSON(clip)), null, 2)
  );
  console.log(`clips json: ${sidecar}`);
}

console.log(`wrote ${outPath} (${bytes.length} bytes, ${meshes} mesh(es), ${all.length} clip(s))`);
console.log(`clips: ${all.map((c) => `${c.name}=${Number(c.duration).toFixed(3)}`).join(",")}`);
console.log(`joints: ${pairs.map((p) => p.name).join(",")}`);
console.log(`retargeted: ${landed.map((c) => c.name).join(",")} from ${basename(animationPath)}`);
