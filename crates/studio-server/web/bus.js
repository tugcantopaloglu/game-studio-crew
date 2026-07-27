const eventTaps = new Set();
const projectTaps = new Set();
const settingTaps = new Set();
const stageTaps = new Set();

let stageRef = null;

export const DEFAULTS = {
  "models.tier1": "fable",
  "models.tier2": "opus",
  "models.tier3": "opus",
  "provider": "claude",
  "music.enabled": false,
  "music.track": "",
  "music.volume": 0.35,
  "limits.enabled": true,
  "limits.refreshSeconds": 1800,
  "lowSpec": false,
  "chatter.enabled": true,
  "chatter.volume": 0.12,
  "thoughts.enabled": true,
  "run.stepConfirm": false,
};

const store = new Map(Object.entries(DEFAULTS));

try {
  const saved = JSON.parse(localStorage.getItem("studio.settings") || "{}");
  for (const [k, v] of Object.entries(saved)) store.set(k, v);
} catch (err) {
  store.clear();
  for (const [k, v] of Object.entries(DEFAULTS)) store.set(k, v);
}

function persist() {
  const out = {};
  for (const [k, v] of store) out[k] = v;
  try {
    localStorage.setItem("studio.settings", JSON.stringify(out));
  } catch (err) {
    return out;
  }
  return out;
}

function fanout(taps, ...args) {
  for (const fn of taps) {
    try {
      fn(...args);
    } catch (err) {
      continue;
    }
  }
}

export const settings = {
  get(key, fallback) {
    if (store.has(key)) return store.get(key);
    return fallback !== undefined ? fallback : DEFAULTS[key];
  },
  set(key, value) {
    if (store.get(key) === value) return value;
    store.set(key, value);
    persist();
    fanout(settingTaps, key, value);
    return value;
  },
  all() {
    const out = {};
    for (const [k, v] of store) out[k] = v;
    return out;
  },
  onChange(fn) {
    settingTaps.add(fn);
    return () => settingTaps.delete(fn);
  },
  async load() {
    try {
      const res = await fetch("/settings");
      if (res.ok) {
        const remote = await res.json();
        for (const [k, v] of Object.entries(remote || {})) store.set(k, v);
        persist();
        fanout(settingTaps, null, null);
      }
    } catch (err) {
      return this.all();
    }
    return this.all();
  },
  async save() {
    const body = persist();
    try {
      await fetch("/settings", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(body),
      });
    } catch (err) {
      return body;
    }
    return body;
  },
};

export function onEvent(fn) {
  eventTaps.add(fn);
  return () => eventTaps.delete(fn);
}

export function pushEvent(ev) {
  fanout(eventTaps, ev);
}

let currentProject = "";
try {
  currentProject = localStorage.getItem("studio.project") || "";
} catch (err) {
  currentProject = "";
}

export function project() {
  return currentProject;
}

export function setProject(id) {
  const next = id || "";
  if (next === currentProject) return next;
  currentProject = next;
  try {
    localStorage.setItem("studio.project", next);
  } catch (err) {
    fanout(projectTaps, next);
    return next;
  }
  fanout(projectTaps, next);
  return next;
}

export function onProject(fn) {
  projectTaps.add(fn);
  return () => projectTaps.delete(fn);
}

export function setStage(obj) {
  stageRef = obj;
  fanout(stageTaps, obj);
  return obj;
}

export function stage() {
  return stageRef;
}

export function onStage(fn) {
  if (stageRef) fanout([fn], stageRef);
  stageTaps.add(fn);
  return () => stageTaps.delete(fn);
}

export function esc(v) {
  return String(v == null ? "" : v)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

export async function api(path, opts = {}) {
  const init = { ...opts };
  if (init.body !== undefined && typeof init.body !== "string") {
    init.headers = { "content-type": "application/json", ...(init.headers || {}) };
    init.body = JSON.stringify(init.body);
    init.method = init.method || "POST";
  }
  const res = await fetch(path, init);
  const text = await res.text();
  let body = text;
  if (text && (text[0] === "{" || text[0] === "[")) {
    try {
      body = JSON.parse(text);
    } catch (err) {
      body = text;
    }
  }
  if (!res.ok) {
    const message = typeof body === "string" ? body : body.error || res.statusText;
    const failure = new Error(message || "request failed");
    failure.status = res.status;
    failure.body = body;
    throw failure;
  }
  return body;
}

export function toast(message) {
  const el = document.getElementById("sent");
  if (el) el.textContent = message;
  return message;
}

export function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "class") node.className = v;
    else if (k === "text") node.textContent = v;
    else if (k.startsWith("on") && typeof v === "function") node.addEventListener(k.slice(2), v);
    else if (v !== null && v !== undefined && v !== false) node.setAttribute(k, v === true ? "" : v);
  }
  for (const c of children.flat()) {
    if (c === null || c === undefined || c === false) continue;
    node.append(c);
  }
  return node;
}
