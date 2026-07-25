import { api, el, esc, onEvent, onProject, project, setProject, settings, toast } from "/bus.js";

const ENGINES = [
  ["godot", "godot"],
  ["web", "pure js (three.js)"],
  ["python", "python"],
  ["unity", "unity"],
  ["ue5", "ue5"],
  ["auto", "adopt what is already there"],
];

const state = {
  root: root0(),
  parent: null,
  dirs: [],
  separator: "\\",
  plan: null,
  steps: [],
  approval: null,
  roles: ["gameplay_engineer"],
};

function root0() {
  try {
    return localStorage.getItem("studio.runRoot") || "";
  } catch (err) {
    return "";
  }
}

function remember(path) {
  try {
    localStorage.setItem("studio.runRoot", path || "");
  } catch (err) {
    return path;
  }
  return path;
}

function join(dir, name) {
  const sep = state.separator || "\\";
  return dir.endsWith(sep) ? dir + name : dir + sep + name;
}

export function stepsFor(form) {
  return [...form.querySelectorAll("[data-step]")].map((row) => ({
    id: row.dataset.step,
    role: row.querySelector("select").value,
    say: row.querySelector("textarea").value.trim(),
  }));
}

export function movedBy(steps, index, delta) {
  const to = index + delta;
  if (to < 0 || to >= steps.length) return steps;
  const out = steps.slice();
  const [taken] = out.splice(index, 1);
  out.splice(to, 0, taken);
  return out;
}

export function mount(root) {
  root.innerHTML = "";

  const where = el("div", { class: "field" });
  const roles = el("div");
  const planBox = el("div", { class: "field" });
  const steer = el("div", { class: "field" });
  const log = el("div", { class: "hint" });

  const name = el("input", { type: "text", placeholder: "name of the game" });
  const engine = el("select");
  for (const [id, label] of ENGINES) engine.append(el("option", { value: id, text: label }));
  const git = el("input", { type: "checkbox", checked: true });

  const here = el("div", { class: "card" });
  const list = el("select", { size: 6, style: "width:100%" });
  const up = el("button", { text: "up one" });
  const create = el("button", { text: "create the project here" });

  const picked = el("select");
  const brief = el("textarea", { placeholder: "what should the crew build?" });
  const confirm = el("input", { type: "checkbox" });
  confirm.checked = !!settings.get("run.stepConfirm");
  confirm.onchange = () => settings.set("run.stepConfirm", confirm.checked);
  const go = el("button", { text: "plan it" });

  async function browse(path) {
    try {
      const body = await api("/fs/browse?path=" + encodeURIComponent(path || ""));
      state.root = body.path;
      state.parent = body.parent;
      state.dirs = body.dirs;
      state.separator = body.separator;
      remember(body.path);
      drawWhere();
    } catch (err) {
      toast(err.message);
    }
  }

  function drawWhere() {
    here.innerHTML = "";
    here.append(
      el("b", { text: "the game will live in" }),
      el("div", { class: "k", text: state.root || "pick a folder" }),
    );
    list.innerHTML = "";
    for (const d of state.dirs) list.append(el("option", { value: d, text: d }));
    up.disabled = !state.parent;
  }

  list.ondblclick = () => {
    if (list.value) browse(join(state.root, list.value));
  };
  up.onclick = () => browse(state.parent || "");

  create.onclick = async () => {
    const chosen = list.value ? join(state.root, list.value) : state.root;
    const wanted = name.value.trim();
    if (!wanted) return toast("a game needs a name");
    if (!chosen) return toast("pick a folder for it to live in");

    const dest = list.value ? chosen : join(state.root, slug(wanted));
    create.disabled = true;
    try {
      const made = await api("/projects", {
        body: { name: wanted, root: dest, engine: engine.value, git: git.checked },
      });
      setProject(made.id);
      await drawProjects();
      toast(made.name + " lives in " + made.root);
    } catch (err) {
      toast(err.message);
    }
    create.disabled = false;
  };

  async function drawProjects() {
    try {
      const rows = await api("/projects");
      picked.innerHTML = "";
      picked.append(el("option", { value: "", text: rows.length ? "pick a game" : "no games yet" }));
      for (const p of rows) {
        picked.append(el("option", { value: p.id, text: p.name + " — " + p.root }));
      }
      picked.value = project() || "";
    } catch (err) {
      toast(err.message);
    }
  }
  picked.onchange = () => setProject(picked.value);
  onProject((id) => {
    if (picked.value !== id) picked.value = id || "";
  });

  go.onclick = async () => {
    if (!project()) return toast("pick where the game lives first");
    go.disabled = true;
    try {
      await api("/run/plan", {
        body: {
          project: project(),
          prompt: brief.value,
          step_confirm: confirm.checked,
        },
      });
      toast("the director is writing the plan");
    } catch (err) {
      toast(err.message);
    }
    go.disabled = false;
  };

  function drawPlan() {
    planBox.innerHTML = "";
    if (!state.plan) {
      planBox.append(el("div", { class: "hint", text: "no plan on the table" }));
      return;
    }

    planBox.append(el("div", { class: "sec", text: state.plan.title || "the plan" }));
    if (!state.plan.editable) {
      for (const s of state.steps) {
        planBox.append(el("div", { class: "card" }, el("b", { text: s.say }), el("div", { class: "k", text: s.role })));
      }
      return;
    }

    const form = el("div", { class: "field" });
    state.steps.forEach((s, i) => {
      const row = el("div", { class: "card" });
      row.dataset.step = s.id || "";

      const say = el("textarea");
      say.value = s.say;

      const role = el("select");
      for (const r of state.roles) role.append(el("option", { value: r, text: r }));
      role.value = s.role;

      row.append(
        say,
        el(
          "div",
          { class: "row" },
          role,
          el(
            "div",
            {},
            el("button", { text: "↑", onclick: () => reorder(form, i, -1) }),
            el("button", { text: "↓", onclick: () => reorder(form, i, 1) }),
            el("button", { text: "✕", onclick: () => drop(form, i) }),
          ),
        ),
      );
      form.append(row);
    });
    planBox.append(form);

    planBox.append(
      el(
        "div",
        { class: "row" },
        el("button", {
          text: "add a step",
          onclick: () => {
            state.steps = stepsFor(form).concat({ id: "", role: state.roles[0], say: "" });
            drawPlan();
          },
        }),
        el("button", { text: "drop the plan", onclick: () => cancelPlan() }),
      ),
    );
    planBox.append(el("button", { text: "start the crew on this", onclick: () => start(form) }));
  }

  function reorder(form, i, delta) {
    state.steps = movedBy(stepsFor(form), i, delta);
    drawPlan();
  }

  function drop(form, i) {
    state.steps = stepsFor(form).filter((_, at) => at !== i);
    drawPlan();
  }

  async function start(form) {
    const steps = stepsFor(form).filter((s) => s.say);
    if (!steps.length) return toast("a plan with no steps builds nothing");
    try {
      await api("/run/start", { body: { plan_id: state.plan.plan_id, steps } });
      state.plan = null;
      drawPlan();
      drawSteer();
      toast("the crew is on it");
    } catch (err) {
      toast(err.message);
    }
  }

  async function cancelPlan() {
    try {
      await api("/run/cancel", { body: { plan_id: state.plan.plan_id } });
      state.plan = null;
      drawPlan();
      toast("dropped before anyone was paid for");
    } catch (err) {
      toast(err.message);
    }
  }

  function drawSteer() {
    steer.innerHTML = "";
    if (state.approval) {
      const card = el("div", { class: "card" });
      const note = el("textarea", { placeholder: "what should be better?" });
      card.append(
        el("div", { class: "k", text: "step " + state.approval.step }),
        el("b", { text: state.approval.title || "the crew finished a step" }),
        el("div", { class: "hint", text: state.approval.summary || "" }),
        note,
        el(
          "div",
          { class: "row" },
          el("button", { class: "ok", text: "looks good", onclick: () => answer("approve", note) }),
          el(
            "div",
            {},
            el("button", { text: "good, but…", onclick: () => answer("improve", note) }),
            el("button", { class: "bad", text: "do it again", onclick: () => answer("redo", note) }),
          ),
        ),
      );
      steer.append(card);
      return;
    }

    const note = el("textarea", { placeholder: "say something to the crew mid-run" });
    steer.append(
      el("div", { class: "sec", text: "steer the run" }),
      note,
      el(
        "div",
        { class: "row" },
        el("button", { text: "send it into the next step", onclick: () => interrupt(false, note) }),
        el("button", { class: "bad", text: "stop", onclick: () => interrupt(true, note) }),
      ),
    );
  }

  async function answer(verdict, note) {
    const id = state.approval.approval_id;
    state.approval = null;
    drawSteer();
    try {
      toast(await api("/run/step", { body: { approval_id: id, verdict, note: note.value } }));
    } catch (err) {
      toast(err.message);
    }
  }

  async function interrupt(stop, note) {
    try {
      const said = await api("/run/interrupt", { body: { stop, note: note.value } });
      note.value = "";
      toast(said);
    } catch (err) {
      toast(err.message);
    }
  }

  where.append(
    el("div", { class: "sec", text: "a new game" }),
    el("div", { class: "row" }, name, engine),
    here,
    list,
    el("div", { class: "row" }, up, el("label", { class: "check" }, git, "git")),
    create,
  );

  roles.append(
    el("div", { class: "sec", text: "the run" }),
    picked,
    brief,
    el("label", { class: "check" }, confirm, "hold at every step for my approval"),
    go,
  );

  root.append(where, roles, planBox, steer, log);

  api("/roles")
    .then((rows) => {
      state.roles = rows.map((r) => r.id);
    })
    .catch(() => {});

  drawWhere();
  drawPlan();
  drawSteer();
  drawProjects();
  browse(state.root);

  onEvent((ev) => {
    if (ev.type === "plan_proposed") {
      state.plan = ev.data;
      state.steps = (ev.data.steps || []).map((s) => ({ id: s.id, role: s.role, say: s.say }));
      state.approval = null;
      drawPlan();
      drawSteer();
      return;
    }
    if (ev.type === "step_approval_needed") {
      state.approval = ev.data;
      drawSteer();
      return;
    }
    if (ev.type === "run_interrupted") {
      const said = ev.data.note ? ": " + ev.data.note : "";
      const at = ev.data.step ? " at " + ev.data.step : "";
      log.innerHTML = esc(ev.data.reason + at + said);
      return;
    }
    if (ev.type === "workflow_ended") {
      state.approval = null;
      drawSteer();
      log.innerHTML = esc("the run " + (ev.data.outcome || "ended"));
    }
  });
}

function slug(name) {
  return name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "game";
}
