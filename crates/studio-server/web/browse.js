import { api, el } from "/bus.js";

export function folderPicker(opts = {}) {
  const box = el("div", { class: opts.class || "field" });
  const here = el("div", { class: "k", text: "" });
  const roots = el("select");
  const up = el("button", { text: "up" });
  const list = el("div", { style: "display:grid;gap:2px;max-height:170px;overflow:auto" });
  const state = { path: "", parent: null, separator: "\\", chosen: "" };
  let items = [];

  function under(name) {
    const sep = state.separator || "\\";
    return state.path.endsWith(sep) ? state.path + name : state.path + sep + name;
  }

  function chosenPath() {
    return state.chosen ? under(state.chosen) : "";
  }

  function tell() {
    if (opts.onChange) opts.onChange({ path: state.path, chosen: chosenPath() });
  }

  function highlight() {
    for (const item of items) item.className = item.textContent === state.chosen ? "ok" : "";
  }

  function choose(name) {
    state.chosen = state.chosen === name ? "" : name;
    highlight();
    tell();
  }

  function paint(dirs) {
    items = dirs.slice(0, 200).map((name) =>
      el("button", {
        style: "text-align:left",
        text: name,
        onclick: opts.choose ? () => choose(name) : () => go(under(name)),
        ondblclick: opts.choose ? () => go(under(name)) : null,
      })
    );
    list.replaceChildren(
      ...(items.length ? items : [el("div", { class: "hint", text: "no subfolders here" })])
    );
  }

  async function go(path) {
    let view;
    try {
      view = await api("/fs/browse?path=" + encodeURIComponent(path || ""));
    } catch (err) {
      list.replaceChildren(el("div", { class: "bad", text: err.message }));
      return;
    }
    state.path = view.path;
    state.parent = view.parent;
    state.separator = view.separator;
    state.chosen = "";
    here.textContent = view.path;
    up.disabled = !view.parent;
    roots.replaceChildren(...view.roots.map((r) => el("option", { value: r, text: r })));
    roots.value = view.roots.find((r) => view.path.startsWith(r)) || view.roots[0] || "";
    paint(view.dirs);
    tell();
  }

  roots.onchange = () => go(roots.value);
  up.onclick = () => go(state.parent || "");

  if (opts.label) box.append(el("label", { text: opts.label }));
  box.append(here, el("div", { class: "row" }, roots, up), list);
  if (opts.onPick) {
    box.append(el("button", { text: "use this folder", onclick: () => opts.onPick(state.path) }));
  }
  go(opts.start || "");

  return { node: box, path: () => state.path, chosen: chosenPath, join: under };
}
