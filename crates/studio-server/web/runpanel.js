import { api, el, esc, onEvent, onProject, project, setProject, settings, toast } from "/bus.js";
import { folderPicker } from "/browse.js";

const ENGINES = [
  ["godot", "godot"],
  ["web", "pure js (three.js)"],
  ["python", "python"],
  ["unity", "unity"],
  ["ue5", "ue5"],
  ["auto", "adopt what is already there"],
];

const state = {
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
  const unfinished = el("div", { class: "field" });
  const steer = el("div", { class: "field" });
  const log = el("div", { class: "hint" });

  const name = el("input", { type: "text", placeholder: "name of the game" });
  const engine = el("select");
  for (const [id, label] of ENGINES) engine.append(el("option", { value: id, text: label }));
  const git = el("input", { type: "checkbox", checked: true });

  const here = el("div", { class: "card" });
  const create = el("button", { text: "create the project here" });
  const picker = folderPicker({
    start: root0(),
    choose: true,
    onChange: (at) => {
      remember(at.path);
      drawWhere();
    },
  });

  const picked = el("select");
  const brief = el("textarea", { placeholder: "what should the crew build?" });
  const confirm = el("input", { type: "checkbox" });
  confirm.checked = !!settings.get("run.stepConfirm");
  confirm.onchange = () => settings.set("run.stepConfirm", confirm.checked);
  const go = el("button", { text: "plan it" });

  function drawWhere() {
    here.replaceChildren(
      el("b", { text: "the game will live in" }),
      el("div", { class: "k", text: destination() || "pick a folder" }),
      el("div", {
        class: "hint",
        text: picker.chosen()
          ? "that folder already exists; the crew works on what is in it"
          : "double click a folder to go into it, or pick one to work on what is already there",
      }),
    );
  }

  function destination() {
    if (!picker.path()) return "";
    if (picker.chosen()) return picker.chosen();
    const wanted = name.value.trim();
    return wanted ? picker.join(slug(wanted)) : picker.path();
  }

  name.oninput = drawWhere;

  create.onclick = async () => {
    const wanted = name.value.trim();
    if (!wanted) return toast("a game needs a name");
    const dest = destination();
    if (!dest) return toast("pick a folder for it to live in");

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
  const drop = el("select");
  drop.append(
    el("option", { value: "", text: "remove this game…" }),
    el("option", { value: "forget", text: "take it off the list, keep its history" }),
    el("option", { value: "purge", text: "erase it and everything it ever ran" }),
  );

  drop.onchange = async () => {
    const how = drop.value;
    drop.value = "";
    const id = picked.value;
    if (!how) return;
    if (!id) return toast("pick a game first");

    const label = picked.options[picked.selectedIndex].text;
    const asked =
      how === "forget"
        ? `Take ${label} off the list?\n\nIts runs, capsules and decisions stay on file, and nothing in the folder is touched.`
        : `Erase ${label}?\n\nEvery run, event, capsule and ledger row it ever made is deleted and cannot be brought back.\n\nThe files in the folder itself are NOT touched.`;
    if (!window.confirm(asked)) return;

    drop.disabled = true;
    try {
      const gone = await api("/projects/" + how, { body: { id } });
      if (project() === id) setProject("");
      await drawProjects();
      toast(
        how === "forget"
          ? `${gone.name} is off the list; its history is still on file`
          : `${gone.name} erased: ${gone.tasks} task(s), ${gone.events} event(s), ${gone.capsules} capsule(s). Your files were not touched.`,
      );
    } catch (err) {
      toast(err.message);
    }
    drop.disabled = false;
  };

  picked.onchange = () => setProject(picked.value);
  onProject((id) => {
    if (picked.value !== id) picked.value = id || "";
    drawUnfinished();
  });

  async function drawUnfinished() {
    unfinished.replaceChildren();
    const id = project();
    if (!id) return;

    let held;
    try {
      held = await api("/resumable?project=" + encodeURIComponent(id));
    } catch (err) {
      return;
    }
    if (!held.resumable || project() !== id) return;

    const card = el("div", { class: "card" });
    card.append(
      el("b", { text: held.title || "a run that stopped part way" }),
      el("div", {
        class: "k",
        text: `${held.done} of ${held.steps} step(s) done, ${held.left.length} never ran`,
      }),
    );
    if (held.why) card.append(el("div", { class: "warn", text: "it stopped because " + held.why }));
    for (const step of held.say) {
      card.append(el("div", { class: "k", text: `${step.id}  ${step.role}  ${step.say}` }));
    }

    const go_on = el("button", { text: `pick up the remaining ${held.left.length} step(s)` });
    go_on.onclick = async () => {
      go_on.disabled = true;
      try {
        await api("/resume", { body: { project: id, step_confirm: confirm.checked } });
        toast("picking the run up where it stopped");
        unfinished.replaceChildren();
      } catch (err) {
        toast(err.message);
        go_on.disabled = false;
      }
    };
    card.append(go_on);
    card.append(
      el("div", {
        class: "hint",
        text: "the steps that finished are already committed; only the ones above are paid for again",
      }),
    );
    unfinished.append(card);
  }

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
      const offered = state.roles.includes(s.role) ? state.roles : [s.role, ...state.roles];
      for (const r of offered) role.append(el("option", { value: r, text: r }));
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
    picker.node,
    el("label", { class: "check" }, git, "git"),
    create,
  );

  roles.append(
    el("div", { class: "sec", text: "the run" }),
    picked,
    drop,
    brief,
    el("label", { class: "check" }, confirm, "approve every step"),
    el("div", {
      class: "hint",
      text: "the run holds after each step and waits for you before the next one",
    }),
    go,
  );

  root.append(where, roles, unfinished, planBox, steer, log);

  api("/roles")
    .then((rows) => {
      state.roles = rows.map((r) => r.id);
    })
    .catch(() => {});

  drawWhere();
  drawPlan();
  drawSteer();
  drawProjects().then(drawUnfinished);

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
      drawUnfinished();
      log.innerHTML = esc("the run " + (ev.data.outcome || "ended"));
    }
  });
}

function slug(name) {
  return name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "game";
}
