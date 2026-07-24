import http from "node:http";
import { readFile, mkdir } from "node:fs/promises";
import { existsSync } from "node:fs";
import { extname, join, normalize, resolve, sep, dirname } from "node:path";
import { spawn } from "node:child_process";

const root = resolve(process.argv[2] || process.cwd());
const out = resolve(process.argv[3] || join(root, ".studio-out", "shots", "latest.png"));
const delayMs = Number(process.argv[4] || 3000);
const PORT = 8764;

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
    const body = await readFile(file);
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
  console.error("no chromium-based browser on this machine; set EDGE_BIN");
  process.exit(1);
}

await mkdir(dirname(out), { recursive: true });

server.listen(PORT, "127.0.0.1", () => {
  const child = spawn(
    browser,
    [
      "--headless=new", "--disable-gpu", "--no-first-run", "--mute-audio",
      "--window-size=1280,720", "--hide-scrollbars",
      `--virtual-time-budget=${delayMs + 2000}`,
      `--screenshot=${out}`, `http://127.0.0.1:${PORT}/`,
    ],
    { stdio: ["ignore", "ignore", "pipe"] }
  );

  let err = "";
  child.stderr.on("data", (d) => { err += d; });
  const watchdog = setTimeout(() => child.kill(), 60000);

  child.on("close", () => {
    clearTimeout(watchdog);
    server.close();
    if (existsSync(out)) {
      console.log(`wrote ${out}`);
      process.exit(0);
    }
    console.error(String(err || "screenshot failed").split("\n").slice(-3).join("\n"));
    process.exit(1);
  });
});
