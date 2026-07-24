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

let model = factory(THREE);
if (model && typeof model.then === "function") model = await model;
if (model && model.group instanceof THREE.Object3D) model = model.group;
if (model && model.scene instanceof THREE.Object3D) model = model.scene;
if (!(model instanceof THREE.Object3D)) {
  console.error("the factory did not return a THREE.Object3D (or {group}/{scene})");
  process.exit(2);
}

model.updateMatrixWorld(true);

const exporter = new GLTFExporter();
const glb = await exporter.parseAsync(model, { binary: true, onlyVisible: false });
const bytes = glb instanceof ArrayBuffer ? Buffer.from(glb) : Buffer.from(JSON.stringify(glb));

await mkdir(dirname(resolve(outPath)), { recursive: true });
await writeFile(resolve(outPath), bytes);

let meshes = 0;
model.traverse((o) => { if (o.isMesh) meshes++; });
console.log(`wrote ${outPath} (${bytes.length} bytes, ${meshes} mesh(es))`);
