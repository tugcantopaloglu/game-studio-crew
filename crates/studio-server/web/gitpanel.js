import { api, el, project, onProject, onEvent, toast } from "/bus.js";

const LANE_W = 15;
const ROW_H = 27;
const PAGE = 60;
const LANE_INK = ["#6fa8d1", "#4ad991", "#e0bc55", "#c58ee0", "#e2636d", "#5eead4"];

const CSS = `
.gitpanel .tree { display: grid; gap: 0; margin-top: 2px; }
.gitpanel .commit { display: grid; grid-template-columns: auto 1fr; gap: 8px; align-items: stretch;
  border-radius: 7px; cursor: pointer; }
.gitpanel .commit:hover { background: rgba(148,163,184,.06); }
.gitpanel .commit.on { background: rgba(111,168,209,.12); outline: 1px solid rgba(111,168,209,.35); }
.gitpanel .commit svg { display: block; }
.gitpanel .what { min-width: 0; display: grid; align-content: center; gap: 1px; padding: 2px 4px 2px 0; }
.gitpanel .line { display: flex; align-items: baseline; gap: 6px; min-width: 0; }
.gitpanel .subject { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; }
.gitpanel .meta { color: var(--faint); font: 10.5px var(--mono); display: flex; gap: 7px;
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.gitpanel .sha { color: var(--dim); font: 11px var(--mono); flex: none; }
.gitpanel .ref { flex: none; font: 10px var(--mono); padding: 0 5px; border-radius: 999px;
  border: 1px solid var(--line-2); color: var(--dim); }
.gitpanel .ref.head { color: var(--running); border-color: rgba(74,217,145,.45); }
.gitpanel .ref.tag { color: var(--warn); border-color: rgba(224,188,85,.45); }
.gitpanel .doomed { color: var(--dim); font: 10.5px var(--mono); margin-left: 10px; }
.gitpanel .said { white-space: pre-wrap; font: 10.5px var(--mono); color: var(--dim);
  max-height: 132px; overflow-y: auto; }
`;

const state = {
  host: null,
  body: null,
  project: "",
  rows: [],
  lanes: 1,
  more: false,
  branch: null,
  head: null,
  dirty: [],
  remotes: [],
  gh: null,
  names: {},
  selected: "",
  plan: null,
  said: null,
  error: "",
  loading: false,
};

export function mount(root) {
  state.host = root;
  root.classList.add("gitpanel");
  const sheet = document.createElement("style");
  sheet.textContent = CSS;
  root.append(sheet);
  state.body = document.createElement("div");
  state.body.style.display = "grid";
  state.body.style.gap = "8px";
  root.append(state.body);

  onProject(() => {
    state.selected = "";
    state.plan = null;
    state.said = null;
    load(true);
  });
  onEvent((ev) => {
    if (ev.type === "git_action" || ev.type === "commit_recorded") load(true);
  });

  load(true);
  return root;
}

function svg(tag, attrs) {
  const node = document.createElementNS("http://www.w3.org/2000/svg", tag);
  for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, v);
  return node;
}

function ink(lane) {
  return LANE_INK[lane % LANE_INK.length];
}

function laneX(lane) {
  return lane * LANE_W + LANE_W / 2;
}

function ago(at) {
  const secs = Math.max(0, Math.floor(Date.now() / 1000) - at);
  if (secs < 60) return secs + "s";
  if (secs < 3600) return Math.floor(secs / 60) + "m";
  if (secs < 86400) return Math.floor(secs / 3600) + "h";
  if (secs < 2592000) return Math.floor(secs / 86400) + "d";
  if (secs < 31536000) return Math.floor(secs / 2592000) + "mo";
  return Math.floor(secs / 31536000) + "y";
}

function refChip(name) {
  const tag = name.startsWith("tag: ");
  const head = name.startsWith("HEAD ->");
  const label = tag ? name.slice(5) : head ? name.slice(8) : name;
  return el("span", { class: "ref" + (tag ? " tag" : head ? " head" : ""), text: label });
}

function graphic(row, incoming) {
  const width = Math.max(1, state.lanes) * LANE_W;
  const box = svg("svg", { width, height: ROW_H, viewBox: `0 0 ${width} ${ROW_H}` });
  const mid = ROW_H / 2;

  if (incoming.has(row.lane)) {
    box.append(
      svg("path", {
        d: `M ${laneX(row.lane)} 0 L ${laneX(row.lane)} ${mid}`,
        stroke: ink(row.lane),
        "stroke-width": 1.6,
        fill: "none",
      })
    );
  }

  for (const [from, to] of row.links) {
    const x1 = laneX(from);
    const x2 = laneX(to);
    const y1 = from === row.lane ? mid : 0;
    const d =
      x1 === x2
        ? `M ${x1} ${y1} L ${x2} ${ROW_H}`
        : `M ${x1} ${y1} C ${x1} ${(y1 + ROW_H) / 2}, ${x2} ${(y1 + ROW_H) / 2}, ${x2} ${ROW_H}`;
    box.append(
      svg("path", { d, stroke: ink(to), "stroke-width": 1.6, fill: "none", "stroke-linecap": "round" })
    );
  }

  const merge = row.parents.length > 1;
  box.append(
    svg("circle", {
      cx: laneX(row.lane),
      cy: mid,
      r: merge ? 4.6 : 3.6,
      fill: merge ? "var(--panel)" : ink(row.lane),
      stroke: ink(row.lane),
      "stroke-width": merge ? 2 : 1,
    })
  );
  return box;
}

function tree() {
  const box = el("div", { class: "tree" });
  let incoming = new Set();

  for (const row of state.rows) {
    const line = el(
      "div",
      {
        class: "commit" + (row.sha === state.selected ? " on" : ""),
        title: row.sha,
        onclick: () => select(row.sha),
      },
      graphic(row, incoming),
      el(
        "div",
        { class: "what" },
        el(
          "div",
          { class: "line" },
          el("span", { class: "sha", text: row.short }),
          el("span", { class: "subject", text: row.subject }),
          ...row.refs.map(refChip)
        ),
        el("div", { class: "meta" }, el("span", { text: row.author }), el("span", { text: ago(row.at) }))
      )
    );
    box.append(line);
    incoming = new Set(row.links.map(([, to]) => to));
  }

  if (state.more) {
    box.append(
      el("button", {
        text: "older commits",
        style: "margin-top:6px",
        onclick: () => load(false),
      })
    );
  }
  return box;
}

function said() {
  if (!state.said) return null;
  return el(
    "div",
    { class: "card" },
    el("b", { class: state.said.ok ? "ok" : "bad", text: state.said.title }),
    el("div", { class: "said", text: state.said.detail })
  );
}

function report(ok, title, detail) {
  state.said = { ok, title, detail: String(detail || "").trim() };
  toast(title);
}

async function act(title, path, body) {
  try {
    const answer = await api(path, { body });
    const detail =
      typeof answer === "string" ? answer : answer.detail || JSON.stringify(answer);
    report(true, title, detail);
    return answer;
  } catch (err) {
    report(false, title + " failed", err.message);
    return null;
  }
}

function remoteBox() {
  const box = el("div", { class: "sec", text: "remote" });
  const rows = [box];

  if (state.remotes.length) {
    for (const r of state.remotes) {
      rows.push(el("div", { class: "card" }, el("b", { text: r.name }), el("div", { class: "k", text: r.url })));
    }
    rows.push(
      el("button", {
        text: state.branch ? `push ${state.branch}` : "push",
        onclick: async () => {
          await act("push", "/git/push", { project: state.project });
          load(true);
        },
      })
    );
    return rows;
  }

  rows.push(el("div", { class: "hint", text: "this project has no remote yet" }));

  if (state.gh && state.gh.gh && state.gh.signed_in) {
    const name = el("input", {
      type: "text",
      value: state.names[state.project] || state.project.replace(/^proj_/, ""),
    });
    const priv = el("input", { type: "checkbox", checked: true });
    rows.push(
      el("div", { class: "field" }, el("label", { text: `create it on github as ${state.gh.login}` }), name),
      el("label", { class: "check" }, priv, el("span", { text: "private" })),
      el("button", {
        text: "create the repository and push",
        onclick: async () => {
          const made = await act("create", "/git/create", {
            project: state.project,
            name: name.value.trim(),
            private: priv.checked,
          });
          if (made === null) return;
          await act("push", "/git/push", { project: state.project });
          load(true);
        },
      })
    );
  } else {
    rows.push(
      el("div", {
        class: "hint",
        text: state.gh && state.gh.gh
          ? "the gh CLI is installed but not signed in; run gh auth login, or set a URL below"
          : "the gh CLI is not on PATH; set a remote URL below",
      })
    );
  }

  const url = el("input", { type: "text", placeholder: "https://github.com/you/your-game.git" });
  rows.push(
    el("div", { class: "field" }, el("label", { text: "or set a remote URL" }), url),
    el("button", {
      text: "set the remote",
      onclick: async () => {
        const done = await act("set the remote", "/git/remote", {
          project: state.project,
          url: url.value.trim(),
        });
        if (done !== null) load(true);
      },
    })
  );
  return rows;
}

function rollbackBox() {
  if (!state.selected) return null;
  const row = state.rows.find((r) => r.sha === state.selected);
  if (!row) return null;

  const rows = [
    el("div", { class: "sec", text: "roll back" }),
    el(
      "div",
      { class: "card" },
      el("b", { text: row.subject }),
      el("div", { class: "k", text: `${row.short} · ${row.author} · ${ago(row.at)} ago` })
    ),
  ];

  if (!state.plan || state.plan.sha !== row.sha) {
    rows.push(
      el("button", {
        text: "show what this would throw away",
        onclick: async () => {
          try {
            const answer = await api("/git/rollback", {
              body: { project: state.project, sha: row.sha },
            });
            state.plan = answer.plan;
            state.said = null;
          } catch (err) {
            report(false, "roll back failed", err.message);
          }
          draw();
        },
      })
    );
    return rows;
  }

  const plan = state.plan;
  const doomed = el("div", { class: "card" });
  doomed.append(
    el("b", {
      class: plan.discards.length || plan.dirty.length ? "warn" : "ok",
      text: plan.discards.length
        ? `${plan.discards.length} commit(s) will be thrown away`
        : "no commits will be thrown away",
    })
  );
  for (const c of plan.discards.slice(0, 12)) {
    doomed.append(el("div", { class: "doomed", text: `${c.short}  ${c.subject}` }));
  }
  if (plan.discards.length > 12) {
    doomed.append(el("div", { class: "doomed", text: `and ${plan.discards.length - 12} more` }));
  }
  if (plan.dirty.length) {
    doomed.append(
      el("b", { class: "bad", text: `${plan.dirty.length} uncommitted change(s) will be destroyed` })
    );
    for (const c of plan.dirty.slice(0, 12)) {
      doomed.append(el("div", { class: "doomed", text: `${c.code || "??"}  ${c.path}` }));
    }
    if (plan.dirty.length > 12) {
      doomed.append(el("div", { class: "doomed", text: `and ${plan.dirty.length - 12} more` }));
    }
  }
  rows.push(doomed);

  rows.push(
    el(
      "div",
      { class: "row" },
      el("button", {
        text: `yes, reset to ${plan.sha.slice(0, 7)}`,
        onclick: async () => {
          const done = await act("roll back", "/git/rollback", {
            project: state.project,
            sha: plan.sha,
            confirm: true,
          });
          state.plan = null;
          if (done && done.detail) report(true, "rolled back", done.detail);
          load(true);
        },
      }),
      el("button", {
        text: "keep it",
        onclick: () => {
          state.plan = null;
          state.selected = "";
          draw();
        },
      })
    )
  );
  return rows;
}

function head() {
  const bits = [
    el("div", { class: "sec", text: "history" }),
    el(
      "div",
      { class: "row" },
      el("div", {
        class: "hint",
        text: state.branch
          ? `${state.branch} at ${state.head || "nothing yet"}`
          : "no branch is checked out",
      }),
      el("button", { text: "refresh", onclick: () => load(true) })
    ),
  ];
  if (state.dirty.length) {
    bits.push(
      el("div", {
        class: "hint warn",
        text: `${state.dirty.length} uncommitted change(s) in the working tree`,
      })
    );
  }
  return bits;
}

function draw() {
  const body = state.body;
  if (!body) return;
  body.innerHTML = "";

  if (!state.project) {
    body.append(el("div", { class: "sec", text: "git" }), el("div", { class: "hint", text: "select a project first" }));
    return;
  }
  if (state.error) {
    body.append(
      el("div", { class: "sec", text: "git" }),
      el("div", { class: "hint bad", text: state.error }),
      el("button", { text: "try again", onclick: () => load(true) })
    );
    return;
  }

  for (const bit of head()) body.append(bit);
  const message = said();
  if (message) body.append(message);
  body.append(tree());
  const rolling = rollbackBox();
  if (rolling) for (const bit of rolling) body.append(bit);
  for (const bit of remoteBox()) body.append(bit);
}

function select(sha) {
  state.selected = state.selected === sha ? "" : sha;
  state.plan = null;
  draw();
}

async function names() {
  if (Object.keys(state.names).length) return;
  try {
    const list = await api("/projects");
    for (const p of list) state.names[p.id] = p.name;
  } catch (err) {
    state.names = {};
  }
}

async function gh() {
  if (state.gh) return;
  try {
    state.gh = await api("/git/host");
  } catch (err) {
    state.gh = { gh: false, signed_in: false, login: null };
  }
}

async function load(reset) {
  const id = project();
  if (state.project !== id) {
    state.project = id;
    state.rows = [];
    reset = true;
  }
  if (!id) {
    state.error = "";
    draw();
    return;
  }
  if (state.loading) return;
  state.loading = true;

  const skip = reset ? 0 : state.rows.length;
  try {
    const page = await api(`/git/tree?project=${encodeURIComponent(id)}&skip=${skip}&limit=${PAGE}`);
    state.error = "";
    state.rows = reset ? page.rows : state.rows.concat(page.rows);
    state.lanes = reset ? page.lanes || 1 : Math.max(state.lanes, page.lanes || 1);
    state.more = page.more;
    state.branch = page.branch;
    state.head = page.head;
    state.dirty = page.dirty || [];
    state.remotes = page.remotes || [];
  } catch (err) {
    state.error = err.message;
    state.rows = [];
  } finally {
    state.loading = false;
  }

  await Promise.all([names(), gh()]);
  draw();
}
