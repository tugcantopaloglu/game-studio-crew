import http from "node:http";
import { readFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { extname, join, normalize, resolve, sep } from "node:path";
import { spawn } from "node:child_process";

const root = resolve(process.argv[2] || process.cwd());
const PORT = 8763;

const COLLECTOR = `<script>
window.__studioErrs = [];
addEventListener("error", (e) => window.__studioErrs.push((e.filename || "?") + ": " + e.message));
addEventListener("unhandledrejection", (e) =>
  window.__studioErrs.push("promise: " + ((e.reason && e.reason.message) || e.reason)));
setTimeout(() => {
  const d = document.createElement("div");
  d.id = "studio-probe";
  const tag = ["STUDIO", "PROBE"].join("_") + "_";
  d.textContent = window.__studioErrs.length
    ? tag + "FAIL\\n" + window.__studioErrs.join("\\n")
    : tag + "OK";
  document.body.appendChild(d);
}, 4000);
</script>`;

const types = {
  ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript",
  ".css": "text/css", ".json": "application/json", ".png": "image/png",
  ".jpg": "image/jpeg", ".svg": "image/svg+xml", ".wav": "audio/wav",
  ".mp3": "audio/mpeg", ".glb": "model/gltf-binary",
};

const server = http.createServer(async (req, res) => {
  const url = new URL(req.url, "http://localhost");
  let path = normalize(decodeURIComponent(url.pathname)).replace(/^[/\\]+/, "");
  if (path === "" || path === ".") path = "index.html";
  const file = resolve(join(root, path));
  if (file !== root && !file.startsWith(root + sep)) {
    res.writeHead(403);
    res.end();
    return;
  }
  try {
    let body = await readFile(file);
    if (path === "index.html") {
      body = Buffer.from(String(body).replace(/<head>/i, "<head>" + COLLECTOR));
    }
    res.writeHead(200, { "content-type": types[extname(file).toLowerCase()] || "application/octet-stream" });
    res.end(body);
  } catch {
    res.writeHead(404);
    res.end("not found");
  }
});

function findBrowser() {
  if (process.env.EDGE_BIN && existsSync(process.env.EDGE_BIN)) return process.env.EDGE_BIN;
  const candidates = [
    "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
    "C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe",
    "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
    "/usr/bin/chromium",
    "/usr/bin/google-chrome",
  ];
  return candidates.find((c) => existsSync(c));
}

const browser = findBrowser();
if (!browser) {
  console.log("STUDIO_CI_DONE runtime probe skipped: no chromium-based browser on this machine");
  process.exit(0);
}

server.listen(PORT, "127.0.0.1", () => {
  const child = spawn(
    browser,
    [
      "--headless=new", "--disable-gpu", "--no-first-run", "--mute-audio",
      "--window-size=1280,720", "--virtual-time-budget=8000",
      "--dump-dom", `http://127.0.0.1:${PORT}/`,
    ],
    { stdio: ["ignore", "pipe", "ignore"] }
  );

  let dom = "";
  child.stdout.on("data", (d) => { dom += d; });
  const watchdog = setTimeout(() => child.kill(), 60000);

  child.on("close", () => {
    clearTimeout(watchdog);
    server.close();

    const marker = dom.indexOf("STUDIO_PROBE_FAIL");
    if (marker >= 0) {
      const tail = dom
        .slice(marker)
        .split("\n")
        .slice(1, 12)
        .map((l) => l.replace(/<[^>]*>/g, "").trim())
        .filter(Boolean);
      for (const line of tail) console.error(`STUDIO_CI_FAIL: runtime: ${line}`);
      console.log("STUDIO_CI_DONE runtime probe found errors");
      process.exit(1);
    }
    if (dom.includes("STUDIO_PROBE_OK")) {
      console.log("STUDIO_CI_DONE runtime probe clean after 4s of virtual play");
      process.exit(0);
    }
    console.error("STUDIO_CI_FAIL: runtime: the page never reached the probe marker; it likely crashed during load");
    console.log("STUDIO_CI_DONE runtime probe did not complete");
    process.exit(1);
  });
});
