import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom, descendants, findNode } from "./floor-dom.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "..");
const webDir = process.argv[2] || join(repo, "crates", "studio-server", "web");

const failures = [];

function check(what, ok, detail) {
  console.log(`  ${ok ? "ok  " : "FAIL"}  ${what}${detail ? "  " + detail : ""}`);
  if (!ok) failures.push(what);
}

const ROLES = [{ id: "artist", title: "artist", tier: 3 }];
const PROVIDERS = [
  {
    id: "claude",
    title: "Claude Code",
    program: "claude",
    installed: true,
    path: "C:/claude.exe",
    flags_verified: true,
    capabilities: {},
    blockers: [],
    plan_blockers: [],
  },
];
const CATALOGUE = {
  probe: { cost: "one short question per model" },
  providers: [
    {
      provider: "claude",
      title: "Claude Code",
      program: "claude",
      installed: true,
      probeable: true,
      has_catalogue: false,
      catalogue_read: false,
      provenance: "the shipped table",
      discovery: "one question each",
      candidates: [
        { id: "opus", label: "", sources: [], verdict: "unknown", efforts: [] },
        { id: "sonnet", label: "", sources: [], verdict: "unknown", efforts: [] },
      ],
    },
  ],
};

const calls = [];
let probed = 0;

function fakeBus() {
  return `
const store = new Map([["models.tier3", "opus"], ["models.role.artist", "opus"]]);
export const settings = {
  get: (k, fallback) => (store.has(k) ? store.get(k) : fallback),
  set: (k, v) => store.set(k, v),
  all: () => Object.fromEntries(store),
  load: async () => ({}),
  save: async () => ({}),
};
export function toast(m) { return m; }
export function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "class") node.className = v;
    else if (k === "text") node.textContent = v;
    else if (k.startsWith("on") && typeof v === "function") node.addEventListener(k.slice(2), v);
    else if (v !== null && v !== undefined && v !== false) node[k] = v === true ? "" : v;
  }
  for (const c of children.flat()) {
    if (c === null || c === undefined || c === false) continue;
    node.append(c);
  }
  return node;
}
export async function api(path, opts) { return globalThis.__answer(path, opts); }
`;
}

globalThis.__answer = async (path, opts) => {
  calls.push(path);
  if (path === "/roles") return ROLES;
  if (path === "/providers") return PROVIDERS;
  if (path === "/models") return CATALOGUE;
  if (path === "/engines") return [];
  if (path === "/limits") {
    return { account: { known: false, reason: "" }, windows: [], note: "", ledger: { known: false } };
  }
  if (path === "/music") return { folder: "C:/music", exists: false, tracks: [], playable: [] };
  if (path === "/models/probe") {
    probed += 1;
    for (const c of CATALOGUE.providers[0].candidates) {
      if (opts.body.models.includes(c.id)) {
        c.verdict = "working";
        c.checked_at = "2026-07-26T10:00:00Z";
      }
    }
    return { checked: opts.body.models.map((id) => ({ id, verdict: "working" })) };
  }
  throw new Error("the panel asked for something the probe does not serve: " + path);
};

function settle() {
  return new Promise((done) => setTimeout(done, 0));
}

console.log("settings panel repaint check (static; no browser is involved)");

const dom = installDom();
dom.getElementById = () => null;

const dir = mkdtempSync(join(tmpdir(), "settings-probe-"));
writeFileSync(join(dir, "bus.js"), fakeBus());
writeFileSync(
  join(dir, "settings.js"),
  readFileSync(join(webDir, "settings.js"), "utf8").replace(/from\s+"\/bus\.js"/, 'from "./bus.js"')
);

const panel = await import(pathToFileURL(join(dir, "settings.js")).href);
const host = document.createElement("div");
panel.mount(host);
for (let i = 0; i < 20; i += 1) await settle();

check("the panel drew its sections", host.children.length >= 5, `${host.children.length} panes`);

const structure = () =>
  host.children
    .filter((_, i) => i !== 1)
    .flatMap((pane) => [pane, ...pane.children, ...pane.children.flatMap((c) => c.children)]);

const before = host.children.map((pane) => pane.children);
const outsideBefore = structure();

const tick = findNode(host, (n) => n.tagName === "INPUT" && n.type === "checkbox" && !n.disabled);
check("a model can be ticked", Boolean(tick));
tick.checked = true;
tick.dispatchEvent({ type: "change", target: tick });

const button = findNode(host, (n) => n.tagName === "BUTTON" && String(n.textContent).startsWith("check "));
check("the check button counts what is ticked", Boolean(button), button && button.textContent);
button.dispatchEvent({ type: "click", target: button });
for (let i = 0; i < 20; i += 1) await settle();

check("the check reached the daemon", probed === 1, `${probed} probe call(s)`);

const outsideAfter = structure();

check(
  "the models pane was repainted",
  host.children[1].children !== before[1],
  "its children were replaced"
);
check(
  "every other pane was left alone",
  outsideAfter.length === outsideBefore.length &&
    outsideAfter.every((node, i) => node === outsideBefore[i]),
  `${outsideBefore.length} nodes before, ${outsideAfter.length} after`
);
check(
  "the crew verdict badges took the new answer",
  descendants(host.children[0]).some((n) => String(n.textContent).includes("answered when checked")),
  "the crew pane reads the same catalogue"
);
check(
  "the panel did not re-fetch the whole studio",
  calls.filter((p) => p === "/roles").length === 1,
  calls.join(" ")
);

const stillTicked = findNode(host.children[1], (n) => n.tagName === "INPUT" && n.checked);
check("the tick survived the repaint", Boolean(stillTicked));

console.log(failures.length ? `\n${failures.length} check(s) failed` : "\nall checks passed");
process.exit(failures.length ? 1 : 0);
