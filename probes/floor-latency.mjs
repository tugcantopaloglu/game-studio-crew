import { percentile } from "./floor-dom.mjs";

const base = process.env.FLOOR_URL || "http://127.0.0.1:7878";
const ROUNDS = Number(process.env.ROUNDS || 40);

const PATHS = [
  ["/", "floor document"],
  ["/floor", "floor layout"],
  ["/roles", "role registry"],
  ["/projects", "project list"],
  ["/settings", "settings panel"],
  ["/games", "games panel"],
  ["/workflows", "workflow list"],
  ["/scene.js", "scene module"],
  ["/voxel.js", "voxel module"],
  ["/vendor/three.module.js", "three.js"],
];

async function time(path) {
  const started = performance.now();
  const res = await fetch(base + path);
  const body = await res.arrayBuffer();
  return { ms: performance.now() - started, status: res.status, bytes: body.byteLength };
}

async function measure(path) {
  const samples = [];
  let last = null;
  for (let i = 0; i < ROUNDS; i++) {
    last = await time(path);
    samples.push(last.ms);
  }
  return { samples, last };
}

console.log(`endpoint latency against ${base}, ${ROUNDS} sequential requests each`);
console.log("  status  bytes      p50       p95       max   endpoint");

let firstPaintBytes = 0;
for (const [path, label] of PATHS) {
  let out;
  try {
    out = await measure(path);
  } catch (err) {
    console.log(`  ---     ---          ---       ---       ---   ${label} (${path}) unreachable`);
    continue;
  }
  const { samples, last } = out;
  if (path === "/" || path.endsWith(".js")) firstPaintBytes += last.bytes;
  console.log(
    `  ${String(last.status).padEnd(7)} ${String(last.bytes).padStart(8)} ` +
    `${percentile(samples, 50).toFixed(2).padStart(8)}  ${percentile(samples, 95).toFixed(2).padStart(8)}  ` +
    `${Math.max(...samples).toFixed(2).padStart(8)}   ${label} (${path})`
  );
}

console.log("");
console.log(`bytes on the wire before the floor can build: ${(firstPaintBytes / 1024).toFixed(0)} KiB`);

const run = process.env.RUN || "";
if (run) {
  for (const [path, label] of [
    [`/runs/${run}/snapshot`, "snapshot of a long run"],
    [`/runs/${run}/events?since_seq=0`, "events from zero"],
    [`/runs/${run}/events?since_seq=999999999`, "events from the head"],
  ]) {
    try {
      const { samples, last } = await measure(path);
      console.log(
        `  ${String(last.status).padEnd(7)} ${String(last.bytes).padStart(8)} ` +
        `${percentile(samples, 50).toFixed(2).padStart(8)}  ${percentile(samples, 95).toFixed(2).padStart(8)}  ` +
        `${Math.max(...samples).toFixed(2).padStart(8)}   ${label}`
      );
    } catch (err) {
      console.log(`  ${label}: unreachable`);
    }
  }
  for (const since of [0, 49_900, 50_000]) {
    const ms = await reconnect(run, since);
    console.log(
      `  websocket reconnect at since_seq=${String(since).padStart(6)}: ` +
      `${ms.frames} frames, first frame ${ms.first.toFixed(1)}ms, backlog drained ${ms.done.toFixed(1)}ms`
    );
  }
} else {
  console.log("set RUN=<run id> to also time the snapshot and resume endpoints");
}

async function reconnect(run, since) {
  const started = performance.now();
  const url = `${base.replace(/^http/, "ws")}/ws?run=${encodeURIComponent(run)}&since_seq=${since}`;
  const ws = new WebSocket(url);
  let first = 0;
  let frames = 0;
  let settled = 0;

  await new Promise((done) => {
    const finish = () => { try { ws.close(); } catch (err) { void err; } done(); };
    const idle = () => {
      if (performance.now() - settled > 400) finish();
      else setTimeout(idle, 120);
    };
    ws.onopen = () => { settled = performance.now(); setTimeout(idle, 500); };
    ws.onmessage = () => {
      if (!frames) first = performance.now() - started;
      frames++;
      settled = performance.now();
    };
    ws.onerror = finish;
    setTimeout(finish, 20_000);
  });

  return { frames, first, done: settled - started };
}
