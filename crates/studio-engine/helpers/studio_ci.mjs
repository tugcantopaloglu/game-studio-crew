import { readdir } from "node:fs/promises";
import { join, extname, resolve } from "node:path";
import { spawnSync } from "node:child_process";

const root = resolve(process.argv[2] || process.cwd());
const skip = new Set(["node_modules", ".git", ".claude", ".studio-out", "vendor", "tools"]);

async function walk(dir, out) {
  for (const e of await readdir(dir, { withFileTypes: true })) {
    if (e.isDirectory()) {
      if (!skip.has(e.name)) await walk(join(dir, e.name), out);
      continue;
    }
    const ext = extname(e.name).toLowerCase();
    if (ext === ".js" || ext === ".mjs") out.push(join(dir, e.name));
  }
  return out;
}

const files = await walk(root, []);
let failed = 0;
for (const f of files) {
  const r = spawnSync(process.execPath, ["--check", f], { encoding: "utf8" });
  if (r.status !== 0) {
    failed++;
    console.error(String(r.stderr || "").trim());
  }
}
console.log(`studio_ci: checked ${files.length} file(s), ${failed} failed`);
process.exit(failed ? 1 : 0);
