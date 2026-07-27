import { writeFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { resolve, dirname } from "node:path";
import { mkdir } from "node:fs/promises";
import * as THREE from "./vendor/three.module.js";
import { GLTFExporter } from "./vendor/GLTFExporter.js";

const [factoryPath, outPath] = process.argv.slice(2);
if (!factoryPath || !outPath) {
  console.error("usage: node model_export.mjs <factory.mjs> <out.glb>");
  process.exit(2);
}

const EXPORTABLE = new Set(["position", "quaternion", "scale", "morphTargetInfluences"]);

function moves(track) {
  const keys = track.times ? track.times.length : 0;
  if (keys < 2 || !track.values || !track.values.length) return false;
  const stride = track.values.length / keys;
  for (let i = stride; i < track.values.length; i++) {
    if (Math.abs(track.values[i] - track.values[i % stride]) > 1e-6) return true;
  }
  return false;
}

function readGlbAnimations(buffer) {
  if (buffer.length < 20 || buffer.readUInt32LE(0) !== 0x46546c67) return [];
  let at = 12;
  while (at + 8 <= buffer.length) {
    const length = buffer.readUInt32LE(at);
    const kind = buffer.readUInt32LE(at + 4);
    const body = buffer.subarray(at + 8, at + 8 + length);
    if (kind === 0x4e4f534a) {
      try {
        const json = JSON.parse(body.toString("utf8"));
        return (json.animations || []).map((a, i) => a.name || `clip_${i}`);
      } catch (err) {
        return [];
      }
    }
    at += 8 + length;
  }
  return [];
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
          "data:" + (blob.type || "application/octet-stream") + ";base64," +
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

let made = factory(THREE);
if (made && typeof made.then === "function") made = await made;
let model = made;
let offered = [];
if (made && Array.isArray(made.clips)) offered = made.clips;
if (made && made.group instanceof THREE.Object3D) model = made.group;
if (made && made.scene instanceof THREE.Object3D) model = made.scene;
if (!(model instanceof THREE.Object3D)) {
  console.error("the factory did not return a THREE.Object3D (or {group}/{scene})");
  process.exit(2);
}

model.updateMatrixWorld(true);

if (typeof mod.clips === "function") {
  let asked = mod.clips(THREE, model);
  if (asked && typeof asked.then === "function") asked = await asked;
  if (Array.isArray(asked)) offered = asked;
}

const clips = offered.filter((clip) => clip && Array.isArray(clip.tracks));
if (offered.length !== clips.length) {
  console.error(`${offered.length - clips.length} of the offered clips are not AnimationClips`);
  process.exit(2);
}

const named = new Set();
model.traverse((o) => {
  if (o.name) named.add(o.name);
});
const unreachable = [];
const targets = new Set();
for (const clip of clips) {
  for (const track of clip.tracks) {
    const at = String(track.name).lastIndexOf(".");
    const node = at < 0 ? String(track.name) : String(track.name).slice(0, at);
    const property = at < 0 ? "" : String(track.name).slice(at + 1);
    if (!named.has(node) || !EXPORTABLE.has(property)) {
      unreachable.push(`${clip.name || "clip"}: ${track.name}`);
      continue;
    }
    targets.add(node);
  }
}
const still = clips.filter((clip) => !clip.tracks.some(moves));
if (still.length) {
  console.error(
    `these clips hold every value they start with, so playing them changes nothing: ${still
      .map((clip) => clip.name || "clip")
      .join(", ")}`
  );
  process.exit(2);
}

if (unreachable.length) {
  console.error(
    `these tracks cannot survive a glb, so the clips holding them would be dropped whole: ${unreachable
      .slice(0, 8)
      .join("; ")}`
  );
  console.error(`a glb animates ${[...EXPORTABLE].join(", ")} on a named node, and nothing else`);
  process.exit(2);
}

const exporter = new GLTFExporter();
const glb = await exporter.parseAsync(model, {
  binary: true,
  onlyVisible: false,
  animations: clips,
});
const bytes = glb instanceof ArrayBuffer ? Buffer.from(glb) : Buffer.from(JSON.stringify(glb));

await mkdir(dirname(resolve(outPath)), { recursive: true });
await writeFile(resolve(outPath), bytes);

let meshes = 0;
model.traverse((o) => { if (o.isMesh) meshes++; });

const landed = readGlbAnimations(bytes);
console.log(`wrote ${outPath} (${bytes.length} bytes, ${meshes} mesh(es), ${landed.length} clip(s))`);
if (landed.length) {
  const held = new Map(clips.map((clip) => [clip.name, clip.duration]));
  const said = landed.map((name) => `${name}=${Number(held.get(name) || 0).toFixed(3)}`);
  console.log(`clips: ${said.join(",")}`);
  console.log(`joints: ${[...targets].join(",")}`);
}
