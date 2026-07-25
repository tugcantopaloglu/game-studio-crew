import { settings } from "/bus.js";
import { mount as mountRun } from "/runpanel.js";
import { mount as mountGames } from "/games.js";
import { mount as mountGit } from "/gitpanel.js";
import { mount as mountAssets } from "/assets.js";
import { mount as mountSettings } from "/settings.js";

const PANELS = [
  { id: "dispatch", label: "dispatch", mount: null },
  { id: "run", label: "run", mount: mountRun },
  { id: "games", label: "games", mount: mountGames },
  { id: "git", label: "git", mount: mountGit },
  { id: "assets", label: "assets", mount: mountAssets },
  { id: "settings", label: "settings", mount: mountSettings },
];

const mounted = new Set();

function show(id) {
  const compose = document.getElementById("compose");
  if (compose) compose.hidden = id !== "dispatch";

  for (const p of PANELS) {
    if (p.id === "dispatch") continue;
    const box = document.getElementById("panel-" + p.id);
    if (!box) continue;
    const on = p.id === id;
    box.hidden = !on;
    if (on && p.mount && !mounted.has(p.id)) {
      mounted.add(p.id);
      try {
        p.mount(box);
      } catch (err) {
        box.textContent = "this panel failed to load: " + err.message;
      }
    }
  }

  for (const btn of document.querySelectorAll("#tabs button")) {
    btn.classList.toggle("on", btn.dataset.panel === id);
  }

  try {
    localStorage.setItem("studio.panel", id);
  } catch (err) {
    return id;
  }
  return id;
}

function start() {
  const tabs = document.getElementById("tabs");
  if (!tabs) return;

  tabs.innerHTML = "";
  for (const p of PANELS) {
    const btn = document.createElement("button");
    btn.dataset.panel = p.id;
    btn.textContent = p.label;
    btn.onclick = () => show(p.id);
    tabs.append(btn);
  }

  let remembered = "dispatch";
  try {
    remembered = localStorage.getItem("studio.panel") || "dispatch";
  } catch (err) {
    remembered = "dispatch";
  }
  if (!PANELS.some((p) => p.id === remembered)) remembered = "dispatch";
  show(remembered);
}

settings.load();

if (document.readyState === "loading") {
  addEventListener("DOMContentLoaded", start);
} else {
  start();
}

export { show };
