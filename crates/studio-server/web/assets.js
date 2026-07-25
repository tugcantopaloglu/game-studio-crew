import { api, el, onProject, project, settings, toast } from "/bus.js";

const ENABLED = "assets.enabled";
const MODEL = "assets.model";

let host = null;
let busy = false;
let lastResult = null;

const asked = {
  kind: "character",
  name: "",
  description: "",
  reference: "",
  overwrite: false,
};

function firstArray(value) {
  if (Array.isArray(value)) return value;
  if (!value || typeof value !== "object") return null;
  for (const key of ["models", "candidates", "entries"]) {
    if (Array.isArray(value[key])) return value[key];
  }
  return null;
}

function statusOf(row) {
  let raw = row.verdict;
  if (raw === undefined) raw = row.status;
  if (raw === undefined) raw = row.state;
  const text = String(raw === undefined ? "" : raw).toLowerCase();
  if (raw === true || text.includes("working") || text.includes("verified")) return "verified";
  if (text.includes("refus") || text.includes("denied") || raw === false) return "refused";
  return "unknown";
}

function sourcesOf(row) {
  const sources = row.sources;
  if (!Array.isArray(sources)) return "";
  return sources
    .map((s) => (typeof s === "string" ? s : s && (s.id || s.name)))
    .filter(Boolean)
    .join(", ");
}

export function codexModels(payload) {
  if (!payload) return [];
  let rows = null;

  const providers = firstArray(payload.providers) || (Array.isArray(payload) ? payload : null);
  if (providers) {
    const codex = providers.find(
      (p) => p && (p.id === "codex" || p.provider === "codex" || p.program === "codex")
    );
    if (codex) rows = firstArray(codex) || firstArray(codex.models);
    if (!rows && providers.some((p) => p && p.provider === "codex")) {
      rows = providers.filter((p) => p.provider === "codex");
    }
  }
  if (!rows) rows = firstArray(payload.codex);
  if (!rows) return [];

  const out = [];
  for (const row of rows) {
    if (!row) continue;
    if (typeof row === "string") {
      out.push({ model: row, status: "unknown", reason: "", checked: "", sources: "" });
      continue;
    }
    const name = row.id || row.model || row.name;
    if (!name) continue;
    out.push({
      model: String(name),
      label: row.label || "",
      status: statusOf(row),
      reason: row.detail || row.reason || row.why || "",
      checked: row.checked_at || row.last_checked || row.checked || "",
      sources: sourcesOf(row),
      cost: row.cost_usd === undefined || row.cost_usd === null ? "" : row.cost_usd,
      seconds: row.seconds === undefined || row.seconds === null ? "" : row.seconds,
    });
  }
  return out;
}

function suggestionTitle(entry) {
  const parts = [];
  if (entry.label) parts.push(entry.label);
  if (entry.reason) parts.push(entry.reason);
  if (entry.sources) parts.push("named by " + entry.sources);
  if (entry.checked) parts.push("checked " + entry.checked);
  if (entry.cost !== "" && entry.cost !== undefined) parts.push("cost $" + entry.cost);
  else if (entry.seconds !== "" && entry.seconds !== undefined && entry.status !== "unknown") {
    parts.push(entry.seconds + "s to answer");
  }
  return parts.join(" · ");
}

function statusLabel(entry) {
  if (entry.status === "verified") return "verified";
  if (entry.status === "refused") return "refused";
  return "not checked";
}

function statusClass(entry) {
  if (entry.status === "verified") return "ok";
  if (entry.status === "refused") return "bad";
  return "hint";
}

function store(key, value) {
  settings.set(key, value);
  settings.save().then(() => toast("settings saved"));
}

function section(title, hint) {
  const out = [el("div", { class: "sec", text: title })];
  if (hint) out.push(el("div", { class: "hint", text: hint }));
  return out;
}

function field(label, node) {
  return el("div", { class: "field" }, el("label", { text: label }), node);
}

function switchRow(onToggle) {
  const input = el("input", { type: "checkbox" });
  input.checked = Boolean(settings.get(ENABLED, false));
  input.onchange = () => {
    store(ENABLED, input.checked);
    onToggle();
  };
  return el(
    "label",
    { class: "check" },
    input,
    el("span", { text: "let the crew generate assets with codex" })
  );
}

function blockerList(blockers) {
  const box = el("div", { style: "display:grid;gap:4px" });
  for (const why of blockers) {
    box.append(el("div", { class: "warn", style: "font-size:11.5px;line-height:1.45", text: why }));
  }
  return box;
}

function capabilityCard(view) {
  const card = el("div", { class: "card", style: "display:grid;gap:6px" });
  card.append(
    el(
      "div",
      { class: "row" },
      el("span", { text: view.program }),
      el("span", {
        class: view.installed ? "ok" : "bad",
        text: view.installed ? "on PATH" : "not installed",
      }),
      el("span", {
        class: view.ready ? "ok" : "warn",
        text: view.ready ? "ready" : "not ready",
      })
    )
  );
  if (view.path) {
    card.append(el("div", { class: "hint", style: "word-break:break-all", text: view.path }));
  }
  card.append(el("div", { class: "hint", style: "line-height:1.45", text: view.how }));
  if (view.blockers && view.blockers.length) card.append(blockerList(view.blockers));
  return card;
}

function kindPicker(kinds) {
  const node = el("select", {
    onchange: (e) => {
      asked.kind = e.target.value;
      redraw();
    },
  });
  for (const k of kinds) {
    const option = el("option", { value: k.key, text: k.title });
    if (k.key === asked.kind) option.selected = true;
    node.append(option);
  }
  return node;
}

function shapeOf(view) {
  const found = (view.kinds || []).find((k) => k.key === asked.kind);
  return found ? found.shape : "";
}

function destinationLine(view) {
  if (!view.makes) return "pick a project with an engine and this says where the file will land";
  if (view.makes.export) {
    return "lands at " + view.makes.factory + " and bakes to " + view.makes.export;
  }
  return "lands at " + view.makes.factory + " and loads straight into the scene";
}

function resultCard(result) {
  if (!result) return null;
  if (!result.ok) {
    return el(
      "div",
      { class: "card", style: "display:grid;gap:5px" },
      el("div", { class: "bad", text: "not generated" }),
      el("div", {
        class: "hint",
        style: "line-height:1.45;word-break:break-word",
        text: result.reason,
      })
    );
  }
  const card = el("div", { class: "card", style: "display:grid;gap:5px" });
  card.append(
    el(
      "div",
      { class: "row" },
      el("span", { class: "ok", text: "generated" }),
      el("span", { text: result.name })
    )
  );
  card.append(el("div", { style: "font-size:12px;word-break:break-all", text: result.factory }));
  if (result.export) {
    card.append(el("div", { style: "font-size:12px;word-break:break-all", text: result.export }));
  }
  card.append(
    el("div", {
      class: "hint",
      text: result.meshes + " mesh(es), " + result.bytes + " bytes when exported",
    })
  );
  if (result.notes) card.append(el("div", { class: "hint", text: result.notes }));
  return card;
}

function madeList(rows) {
  if (!rows || !rows.length) {
    return el("div", { class: "hint", text: "no assets generated in this project yet" });
  }
  const box = el("div", { style: "display:grid;gap:5px" });
  for (const row of rows) {
    box.append(
      el(
        "div",
        { class: "card", style: "display:grid;gap:3px" },
        el(
          "div",
          { class: "row" },
          el("span", { text: row.name }),
          el("span", { class: "k", text: row.kind })
        ),
        el("div", { class: "hint", style: "word-break:break-all", text: row.factory }),
        el("div", { class: "hint", text: row.meshes + " mesh(es)" })
      )
    );
  }
  return box;
}

async function submit(view, button) {
  if (busy) return;
  if (!asked.name.trim()) {
    toast("an asset needs a name");
    return;
  }
  if (!asked.description.trim()) {
    toast("describe the asset; the words are all codex gets");
    return;
  }
  busy = true;
  button.disabled = true;
  button.textContent = "asking codex...";
  toast("generating " + asked.name + "; this takes a minute or two");
  try {
    lastResult = await api("/assets/generate", {
      body: {
        project: view.projectId,
        kind: asked.kind,
        name: asked.name,
        description: asked.description,
        reference: asked.reference,
        overwrite: asked.overwrite,
      },
    });
    toast(lastResult.ok ? "generated " + lastResult.name : "codex could not do it");
  } catch (err) {
    lastResult = { ok: false, reason: err.message };
    toast("asset generation failed");
  }
  busy = false;
  redraw();
}

function modelField(view) {
  const box = el("div", { style: "display:grid;gap:5px" });
  const known = view.models || [];
  const listId = "codex-model-suggestions";

  const input = el("input", {
    type: "text",
    list: listId,
    placeholder: view.default_model || "",
    value: settings.get(MODEL, "") || "",
    onchange: (e) => store(MODEL, e.target.value),
  });
  box.append(field("codex model", input));

  if (known.length) {
    const list = el("datalist", { id: listId });
    for (const entry of known) list.append(el("option", { value: entry.model }));
    box.append(list);

    const row = el("div", { class: "row", style: "flex-wrap:wrap;gap:4px" });
    for (const entry of known) {
      row.append(
        el("span", {
          class: statusClass(entry),
          style: "font-size:10.5px",
          title: suggestionTitle(entry),
          text: entry.model + " · " + statusLabel(entry),
        })
      );
    }
    box.append(row);
    box.append(
      el("div", {
        class: "hint",
        text: "a model listed but not checked is only known to exist; the settings panel probes one for real",
      })
    );
  } else {
    box.append(
      el("div", {
        class: "hint",
        text: "no model probe has reported yet, so type a model name; codex debug models lists what this account may use",
      })
    );
  }

  box.append(
    el("div", {
      class: "hint",
      style: "line-height:1.45",
      text: view.model_note
        ? view.model_note + ". In force now: " + view.model
        : "in force now: " + view.model,
    })
  );
  return box;
}

function form(view) {
  const box = el("div", { style: "display:grid;gap:6px" });

  box.append(field("what to make", kindPicker(view.kinds || [])));
  if (shapeOf(view)) {
    box.append(el("div", { class: "hint", style: "line-height:1.45", text: shapeOf(view) }));
  }

  const name = el("input", {
    type: "text",
    placeholder: "Scrapyard Scout",
    value: asked.name,
    oninput: (e) => {
      asked.name = e.target.value;
    },
  });
  box.append(field("name", name));

  const description = el("textarea", {
    rows: 4,
    placeholder: "a wiry teenage salvager in a patched hooded coat, goggles pushed up",
    oninput: (e) => {
      asked.description = e.target.value;
    },
  });
  description.value = asked.description;
  box.append(field("what it looks like", description));

  const reference = el("input", {
    type: "text",
    placeholder: "reference/scout.png, optional",
    value: asked.reference,
    oninput: (e) => {
      asked.reference = e.target.value;
    },
  });
  box.append(field("reference image", reference));
  box.append(
    el("div", {
      class: "hint",
      text: "a path inside the project; codex reads it, it cannot draw one",
    })
  );

  box.append(el("div", { class: "hint", text: destinationLine(view) }));
  box.append(modelField(view));

  const replace = el("input", { type: "checkbox" });
  replace.checked = asked.overwrite;
  replace.onchange = () => {
    asked.overwrite = replace.checked;
  };
  box.append(
    el(
      "label",
      { class: "check" },
      replace,
      el("span", { text: "replace the file if one is already there" })
    )
  );

  const button = el("button", {
    text: "generate",
    onclick: () => submit(view, button),
  });
  button.disabled = busy || !view.ready || !view.projectId;
  box.append(button);

  if (!view.projectId) {
    box.append(el("div", { class: "warn", text: "pick a game on the games panel first" }));
  }

  return box;
}

async function discoverModels() {
  try {
    return codexModels(await api("/models"));
  } catch (err) {
    return [];
  }
}

async function load() {
  const id = project();
  const query = id ? "/assets?project=" + encodeURIComponent(id) : "/assets";
  const view = await api(query);
  view.projectId = id;
  view.models = await discoverModels();
  return view;
}

function draw(view) {
  if (!host) return;
  host.innerHTML = "";
  host.append(
    ...section(
      "assets",
      "codex writes procedural three.js source for characters and props; off by default"
    )
  );
  host.append(switchRow(() => redraw()));
  host.append(capabilityCard(view));

  if (view.enabled) {
    host.append(...section("make one"));
    host.append(form(view));
    const result = resultCard(lastResult);
    if (result) host.append(result);
    host.append(...section("already generated"));
    host.append(madeList(view.assets));
  }
}

function redraw() {
  if (!host) return;
  load().then(draw, (err) => {
    host.innerHTML = "";
    host.append(
      el("div", { class: "bad", text: "the studio could not report on assets: " + err.message })
    );
  });
}

export function mount(box) {
  host = box;
  redraw();
  onProject(() => {
    lastResult = null;
    redraw();
  });
}
