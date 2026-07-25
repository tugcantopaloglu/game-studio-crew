import { api, el, onProject, project, settings, toast } from "/bus.js";

const ENABLED = "assets.enabled";
const MODEL = "assets.model";

let host = null;
let busy = false;
let lastResult = null;

const asked = { kind: "character", name: "", description: "", reference: "" };

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

  const model = el("input", {
    type: "text",
    placeholder: "the codex default",
    value: settings.get(MODEL, "") || "",
    onchange: (e) => store(MODEL, e.target.value),
  });
  box.append(field("codex model", model));

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

async function load() {
  const id = project();
  const query = id ? "/assets?project=" + encodeURIComponent(id) : "/assets";
  const view = await api(query);
  view.projectId = id;
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
