import { settings, api, el, toast } from "/bus.js";

const EFFORTS =["low", "medium", "high", "xhigh", "max"];
const TIERS = [
  [1, "tier 1 · direction"],
  [2, "tier 2 · department leads"],
  [3, "tier 3 · specialists"],
];
const REFRESH = [
  [1800, "every 30 minutes"],
  [300, "every 5 minutes"],
  [60, "every minute"],
];
const CAPABILITIES = [
  ["system_prompt_file", "frozen charter"],
  ["streaming_events", "streamed events"],
  ["usage_reporting", "token usage"],
  ["tool_restriction", "tool allowlist"],
  ["structured_output", "output schema"],
  ["session_control", "sessions"],
];

let saveTimer = null;

function store(key, value) {
  settings.set(key, value);
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    settings.save().then(() => toast("settings saved"));
  }, 250);
}

function storeNow(key, value) {
  settings.set(key, value);
  clearTimeout(saveTimer);
  return settings.save().then((saved) => {
    toast("settings saved");
    return saved;
  });
}

function read(key, fallback) {
  const value = settings.get(key, fallback);
  return value === undefined || value === null ? fallback : value;
}

function section(title, hint) {
  const box = el("div", { class: "sec", text: title });
  return hint ? [box, el("div", { class: "hint", text: hint })] : [box];
}

function choose(options, value, onPick) {
  const node = el("select", {
    onchange: (e) => onPick(e.target.value),
  });
  for (const [key, label] of options) {
    const opt = el("option", { value: key, text: label });
    if (String(key) === String(value)) opt.selected = true;
    node.append(opt);
  }
  return node;
}

function check(label, key, fallback, onToggle) {
  const input = el("input", { type: "checkbox" });
  input.checked = Boolean(read(key, fallback));
  input.onchange = () => {
    store(key, input.checked);
    if (onToggle) onToggle(input.checked);
  };
  return el("label", { class: "check" }, input, el("span", { text: label }));
}

function field(label, node) {
  return el("div", { class: "field" }, el("label", { text: label }), node);
}

function mark(ok) {
  return el("span", { class: ok ? "ok" : "bad", text: ok ? "yes" : "no" });
}

let catalogue = { providers: [], probe: {} };
let listCounter = 0;
let verdictWatchers = [];
const panes = {};
const ticked = new Map();
const probeStatus = new Map();

function tickedIn(providerId) {
  if (!ticked.has(providerId)) ticked.set(providerId, new Set());
  return ticked.get(providerId);
}

function catalogueFor(providerId) {
  return catalogue.providers.find((p) => p.provider === providerId) || null;
}

function candidatesFor(providerId) {
  const row = catalogueFor(providerId);
  return row ? row.candidates : [];
}

function shortWhen(iso) {
  if (!iso) return "never";
  const at = new Date(iso);
  if (Number.isNaN(at.getTime())) return iso;
  return at.toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

function verdictOf(providerId, id) {
  if (!id) return el("span", { class: "hint", text: "following the shipped default" });
  const found = candidatesFor(providerId).find((m) => m.id === id);
  if (!found || found.verdict === "unknown") {
    return el("span", { class: "warn", text: "never checked on this machine" });
  }
  if (found.verdict === "working") {
    return el("span", { class: "ok", text: `answered when checked ${shortWhen(found.checked_at)}` });
  }
  return el("span", {
    class: "bad",
    text: `refused ${shortWhen(found.checked_at)}: ${found.detail || "the CLI gave no reason"}`,
  });
}

function modelKey(providerId, scope) {
  return providerId === "claude" ? `models.${scope}` : `models.${providerId}.${scope}`;
}

function effortNote(providerId, modelId, chosen) {
  const found = candidatesFor(providerId).find((m) => m.id === modelId);
  const offered = found && found.efforts && found.efforts.length ? found.efforts : null;
  if (!offered) return null;

  if (chosen && !offered.includes(chosen)) {
    const usable = offered.filter((e) => EFFORTS.includes(e));
    const fallback = usable[usable.length - 1];
    return el("div", {
      class: "bad",
      text: `${modelId} does not take ${chosen}; it takes ${offered.join(", ")}${
        fallback ? `, so the studio would run it at ${fallback}` : ""
      }`,
    });
  }
  const unknownToStudio = offered.filter((e) => !EFFORTS.includes(e));
  return el("div", {
    class: "k",
    text: `${modelId} takes ${offered.join(", ")}${
      unknownToStudio.length ? ` — the studio cannot ask for ${unknownToStudio.join(", ")}` : ""
    }`,
  });
}

function modelField(label, providerId, scope) {
  const key = modelKey(providerId, scope);
  const listId = `models-${providerId}-${(listCounter += 1)}`;
  const suggestions = el("datalist", { id: listId });
  for (const c of candidatesFor(providerId)) {
    suggestions.append(el("option", { value: c.id, label: c.label || "" }));
  }

  const effortKey = `effort.${scope}`;
  const badge = el("div", { class: "hint" });
  const levels = el("div", { class: "hint" });
  const showVerdict = (named) => {
    badge.replaceChildren(verdictOf(providerId, named));
    const note = effortNote(providerId, named, read(effortKey, ""));
    levels.replaceChildren(...(note ? [note] : []));
  };
  showVerdict(read(key, ""));
  verdictWatchers.push(() => showVerdict(read(key, "")));

  const box = el("div", { class: "field" });
  const input = el("input", {
    type: "text",
    list: listId,
    value: read(key, ""),
    placeholder: providerId === "claude" ? "shipped default" : `${providerId} model name`,
    onchange: (e) => {
      const named = e.target.value.trim();
      store(key, named);
      showVerdict(named);
    },
  });

  box.append(el("label", { text: label }), input, suggestions, badge, levels);
  return box;
}

function whenIsThat(unixSeconds) {
  if (!unixSeconds) return "not given";
  const at = new Date(unixSeconds * 1000);
  const minutes = Math.round((at.getTime() - Date.now()) / 60000);
  const clock = at.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  if (minutes <= 0) return `${clock} (passed)`;
  if (minutes < 60) return `${clock} (in ${minutes} min)`;
  return `${clock} (in ${Math.floor(minutes / 60)}h ${minutes % 60}m)`;
}

function crewSection(root, roles, providers) {
  const box = el("div", { class: "sec", text: "crew" });
  const hint = el("div", {
    class: "hint",
    text: "the model each tier runs on, and any single seat you want somewhere else",
  });
  root.append(box, hint);

  const installed = providers.filter((p) => p.installed);
  const providerOptions = installed.map((p) => [p.id, p.title]);
  if (!providerOptions.length) providerOptions.push(["claude", "Claude Code"]);

  root.append(
    field(
      "provider for the whole studio",
      choose(providerOptions, read("provider", "claude"), (v) => {
        store("provider", v);
        redraw();
      })
    )
  );

  const studioProvider = read("provider", "claude");
  for (const [tier, label] of TIERS) {
    root.append(modelField(label, studioProvider, `tier${tier}`));
    root.append(
      field(
        `${label} effort`,
        choose(
          [["", "shipped effort"]].concat(EFFORTS.map((e) => [e, e])),
          read(`effort.tier${tier}`, ""),
          (v) => store(`effort.tier${tier}`, v)
        )
      )
    );
  }

  const seats = el("div", { class: "card" });
  seats.append(el("b", { text: "one seat at a time" }));
  seats.append(
    el("div", {
      class: "k",
      text: "blank means the seat follows its tier. the model is part of the prompt cache key, so moving a seat mints it a fresh prefix.",
    })
  );

  for (const r of roles) {
    const providerKey = `provider.role.${r.id}`;
    const chosenProvider = read(providerKey, "") || studioProvider;

    seats.append(modelField(`${r.title} · tier ${r.tier}`, chosenProvider, `role.${r.id}`));
    seats.append(
      field(
        `${r.title} effort`,
        choose(
          [["", "tier"]].concat(EFFORTS.map((e) => [e, e])),
          read(`effort.role.${r.id}`, ""),
          (v) => store(`effort.role.${r.id}`, v)
        )
      )
    );

    if (installed.length > 1) {
      seats.append(
        field(
          `${r.title} runs on`,
          choose(
            [["", "the studio provider"]].concat(providerOptions),
            read(providerKey, ""),
            (v) => {
              store(providerKey, v);
              redraw();
            }
          )
        )
      );
    }
  }

  root.append(seats);
}

function providerSection(root, providers) {
  root.append(...section("coding CLIs", "only the ones on your PATH can be chosen"));

  for (const p of providers) {
    const card = el("div", { class: "card" });
    card.append(el("b", { text: p.title }));
    card.append(
      el("div", { class: "k", text: p.installed ? p.path : `${p.program} is not on PATH` })
    );

    if (!p.flags_verified) {
      card.append(
        el("div", {
          class: "warn",
          text: "flags never read on this machine; the studio will not guess them",
        })
      );
    }

    const caps = el("div", { class: "hint" });
    for (const [key, label] of CAPABILITIES) {
      const line = el("div", { class: "row" });
      line.append(el("span", { text: label }), mark(p.capabilities[key]));
      caps.append(line);
    }
    card.append(caps);

    for (const reason of p.blockers) {
      card.append(el("div", { class: "bad", text: reason }));
    }
    if (!p.blockers.length && p.plan_blockers.length) {
      card.append(
        el("div", {
          class: "warn",
          text: `the studio director's plan is refused here: ${p.plan_blockers[0]}`,
        })
      );
    }
    if (!p.blockers.length && !p.plan_blockers.length) {
      card.append(el("div", { class: "ok", text: "serves every seat in the studio" }));
    }

    root.append(card);
  }
}

function providerModelsCard(row) {
  const card = el("div", { class: "card" });
  card.append(el("b", { text: row.title }));
  card.append(el("div", { class: "k", text: row.provenance }));
  card.append(
    el("div", {
      class: row.has_catalogue ? "ok" : "hint",
      text: row.has_catalogue
        ? row.catalogue_read
          ? "read from its own catalogue, at no cost"
          : "it has a catalogue but the studio could not read it"
        : `discovery here costs ${row.discovery}`,
    })
  );

  if (!row.installed) {
    card.append(el("div", { class: "warn", text: `${row.program} is not on PATH, so nothing here can be checked` }));
    return card;
  }
  if (!row.probeable) {
    card.append(el("div", { class: "warn", text: "the studio has never read this CLI's flags, so it will not invent a command to check it with" }));
    return card;
  }
  if (!row.candidates.length) {
    card.append(el("div", { class: "hint", text: "no name to offer yet; type one in a picker above and check it here" }));
    return card;
  }

  const chosen = tickedIn(row.provider);
  for (const id of [...chosen]) {
    if (!row.candidates.some((c) => c.id === id)) chosen.delete(id);
  }

  const status = el("div", { class: "hint", text: probeStatus.get(row.provider) || "" });
  const button = el("button");
  const label = () => {
    button.textContent = chosen.size
      ? `check ${chosen.size} model${chosen.size === 1 ? "" : "s"}`
      : "check the ticked models";
  };
  label();

  for (const c of row.candidates) {
    const tick = el("input", { type: "checkbox" });
    tick.checked = chosen.has(c.id);
    tick.onchange = () => {
      if (tick.checked) chosen.add(c.id);
      else chosen.delete(c.id);
      label();
    };

    const line = el("div", { class: "field" });
    line.append(
      el("label", { class: "check" }, tick, el("span", { text: c.id })),
      el("div", { class: "k", text: c.label || c.sources.map((s) => s.explain).join("; ") }),
      el("div", { class: "hint" }, verdictOf(row.provider, c.id))
    );
    if (c.efforts && c.efforts.length) {
      line.append(
        el("div", {
          class: "k",
          text: `reasoning: ${c.efforts.join(", ")}${
            c.default_effort ? ` (its default is ${c.default_effort})` : ""
          }`,
        })
      );
    }
    card.append(line);
  }

  card.append(el("div", { class: "warn", text: catalogue.probe.cost }));
  button.onclick = () => {
    if (!chosen.size) {
      note(row.provider, status, "tick a model first; nothing is checked without being asked");
      return;
    }
    const asked = [...chosen];
    button.disabled = true;
    note(
      row.provider,
      status,
      `asking ${row.title} about ${asked.length} model${asked.length === 1 ? "" : "s"}; this can take a few minutes`
    );
    api("/models/probe", { body: { provider: row.provider, models: asked } })
      .then((done) => {
        const worked = done.checked.filter((r) => r.verdict === "working").length;
        note(row.provider, status, `${worked} of ${done.checked.length} answered`);
        return reloadCatalogue();
      })
      .then(() => {
        paintModels();
        for (const refresh of verdictWatchers) refresh();
      })
      .catch((err) => {
        button.disabled = false;
        note(row.provider, status, `the check did not finish: ${err.message}`);
      });
  };
  card.append(button);
  card.append(status);
  return card;
}

function note(providerId, status, text) {
  probeStatus.set(providerId, text);
  status.textContent = text;
}

function paintModels() {
  const root = panes.models;
  if (!root) return;
  root.replaceChildren(
    ...section(
      "models",
      "no CLI here has a subcommand that lists its models, so the studio checks them by asking one"
    )
  );
  for (const row of catalogue.providers) {
    root.append(providerModelsCard(row));
  }
}

function limitsSection(root) {
  root.append(...section("claude subscription"));

  root.append(check("keep an eye on the limit windows", "limits.enabled", true, () => redraw()));
  root.append(
    field(
      "check",
      choose(REFRESH, read("limits.refreshSeconds", 1800), (v) => {
        store("limits.refreshSeconds", Number(v));
        redraw();
      })
    )
  );

  const card = el("div", { class: "card" });
  card.append(el("div", { class: "k", text: "reading the CLI" }));
  root.append(card);

  if (!read("limits.enabled", true)) {
    card.replaceChildren(el("div", { class: "hint", text: "not being checked" }));
    return;
  }

  const paint = (data) => {
    card.replaceChildren();

    if (data.account.known) {
      card.append(el("b", { text: `${data.account.plan} plan` }));
      card.append(el("div", { class: "k", text: data.account.account || "" }));
      card.append(el("div", { class: "k", text: `read from ${data.account.source}` }));
    } else {
      card.append(el("b", { class: "warn", text: "plan unknown" }));
      card.append(el("div", { class: "hint", text: data.account.reason }));
    }

    for (const w of data.windows) {
      const row = el("div", { class: "row" });
      row.append(
        el("span", { text: w.window.replace(/_/g, " ") }),
        el("span", { class: w.status === "allowed" ? "ok" : "warn", text: w.status || "unknown" })
      );
      card.append(row);
      card.append(el("div", { class: "k", text: `resets ${whenIsThat(w.resets_at)}` }));
    }

    if (!data.windows.length) {
      card.append(el("div", { class: "warn", text: "windows unavailable" }));
    }
    card.append(el("div", { class: "hint", text: data.note }));

    if (data.ledger.known) {
      card.append(
        el("div", {
          class: "k",
          text: `own ledger, last 24h: ${data.ledger.cache_read.toLocaleString()} tokens read from cache against ${data.ledger.cache_creation.toLocaleString()} written, ${Math.round(data.ledger.hit_ratio * 100)}% warm across ${data.ledger.prefixes} prefixes`,
        })
      );
    } else {
      card.append(el("div", { class: "k", text: "own ledger: nothing billed in the last 24h" }));
    }
  };

  const pull = () =>
    api("/limits")
      .then(paint)
      .catch((err) =>
        card.replaceChildren(el("div", { class: "bad", text: `could not read limits: ${err.message}` }))
      );

  pull();
  const seconds = Number(read("limits.refreshSeconds", 1800)) || 1800;
  timers.push(setInterval(pull, seconds * 1000));
}

function audioElement() {
  let node = document.getElementById("studio-music");
  if (!node) {
    node = el("audio", { id: "studio-music", preload: "none" });
    document.body.append(node);
  }
  node.volume = Number(read("music.volume", 0.35));
  return node;
}

function musicSection(root) {
  root.append(...section("music", "drop audio files in a folder and the studio will find them"));

  const audio = audioElement();
  const status = el("div", { class: "hint", text: "" });
  const list = el("select");
  const folderLine = el("div", { class: "k", text: "" });
  const picker = el("div", { class: "card" });
  picker.hidden = true;

  let tracks = [];

  const pick = (name, gesture) => {
    store("music.track", name);
    audio.src = `/music/track?name=${encodeURIComponent(name)}`;
    if (!gesture) return;
    audio
      .play()
      .then(() => (status.textContent = `playing ${name}`))
      .catch(() => {
        status.textContent = "the browser blocked playback; press play again";
      });
  };

  const step = (gesture) => {
    if (!tracks.length) return;
    const current = tracks.indexOf(read("music.track", ""));
    const next = read("music.shuffle", false)
      ? Math.floor(Math.random() * tracks.length)
      : (current + 1) % tracks.length;
    list.value = tracks[next];
    pick(tracks[next], gesture);
  };

  audio.onended = () => step(true);

  list.onchange = () => pick(list.value, !audio.paused);

  const load = () =>
    api("/music")
      .then((data) => {
        folderLine.textContent = data.folder;
        tracks = data.tracks.map((t) => t.name);
        list.replaceChildren();

        if (!data.exists) {
          status.textContent = `${data.folder} does not exist yet; make it or choose another folder`;
          return;
        }
        if (!tracks.length) {
          status.textContent = `nothing playable in ${data.folder}; it takes ${data.playable.join(", ")}`;
          return;
        }

        for (const t of data.tracks) {
          list.append(el("option", { value: t.name, text: t.name }));
        }
        const remembered = read("music.track", "");
        list.value = tracks.includes(remembered) ? remembered : tracks[0];
        status.textContent = `${tracks.length} track${tracks.length === 1 ? "" : "s"} ready`;
      })
      .catch((err) => {
        status.textContent = `could not read the music folder: ${err.message}`;
      });

  const browse = (path) =>
    api(`/fs/browse?path=${encodeURIComponent(path || "")}`)
      .then((data) => {
        picker.replaceChildren();
        picker.append(el("div", { class: "k", text: data.path }));

        const jump = el("div", { class: "row" });
        if (data.parent) {
          jump.append(el("button", { text: "up", onclick: () => browse(data.parent) }));
        }
        jump.append(
          el("button", {
            text: "use this folder",
            onclick: () => {
              store("music.folder", data.path);
              picker.hidden = true;
              load();
            },
          })
        );
        picker.append(jump);

        for (const drive of data.roots) {
          picker.append(el("button", { text: drive, onclick: () => browse(drive) }));
        }
        for (const dir of data.dirs.slice(0, 200)) {
          picker.append(
            el("button", {
              text: dir,
              onclick: () => browse(data.path + data.separator + dir),
            })
          );
        }
      })
      .catch((err) => {
        picker.replaceChildren(el("div", { class: "bad", text: err.message }));
      });

  root.append(
    check("play music on the floor", "music.enabled", false, (on) => {
      if (on) {
        const chosen = read("music.track", "") || tracks[0];
        if (chosen) pick(chosen, true);
      } else {
        audio.pause();
        status.textContent = "stopped";
      }
    })
  );

  root.append(field("track", list));

  const transport = el("div", { class: "row" });
  transport.append(
    el("button", {
      text: "play",
      onclick: () => {
        const chosen = list.value || read("music.track", "") || tracks[0];
        if (chosen) pick(chosen, true);
      },
    }),
    el("button", { text: "next", onclick: () => step(true) })
  );
  root.append(transport);
  root.append(check("shuffle", "music.shuffle", false));

  const volume = el("input", {
    type: "range",
    min: "0",
    max: "1",
    step: "0.01",
    value: String(read("music.volume", 0.35)),
    oninput: (e) => {
      audio.volume = Number(e.target.value);
      store("music.volume", Number(e.target.value));
    },
  });
  root.append(field("volume", volume));

  root.append(status);
  root.append(field("folder", folderLine));
  root.append(
    el("button", {
      text: "choose a folder",
      onclick: () => {
        picker.hidden = !picker.hidden;
        if (!picker.hidden) browse(read("music.folder", ""));
      },
    })
  );
  root.append(picker);

  load();
}

function engineCard(row, repaint) {
  const card = el("div", { class: "card" });
  card.append(el("b", { text: row.title }));

  if (row.found) {
    card.append(el("div", { class: "ok", text: `${row.path} (${row.how})` }));
    card.append(
      el("div", {
        class: "k",
        text: row.verifies.length
          ? `gates it can run: ${row.verifies.join(", ")}`
          : "no verification gate is defined for this engine yet",
      })
    );
  } else {
    card.append(
      el("div", {
        class: "warn",
        text: "not found, so every gate that needs it is skipped and nothing the crew writes gets compiled",
      })
    );
    for (const place of row.looked_in) {
      card.append(el("div", { class: "k", text: `looked in ${place}` }));
    }
  }

  const input = el("input", {
    type: "text",
    value: row.named || "",
    placeholder: `full path to the ${row.title} binary`,
    onchange: (e) => storeNow(row.key, e.target.value.trim()).then(repaint),
  });
  card.append(el("label", { text: `where ${row.title} lives` }), input);
  card.append(
    el("div", { class: "k", text: `leave it blank to search PATH and ${row.env}` })
  );
  return card;
}

function enginesSection(root) {
  const paint = () => {
    root.replaceChildren(
      ...section("engines", "what the studio compiles and runs the crew's work with")
    );
    api("/engines")
      .then((rows) => {
        for (const row of rows) root.append(engineCard(row, paint));
      })
      .catch((err) => {
        root.append(el("div", { class: "bad", text: `could not read engines: ${err.message}` }));
      });
  };
  paint();
}

function floorSection(root) {
  root.append(...section("floor"));
  root.append(check("low spec mode", "lowSpec", false));
  root.append(
    el("div", {
      class: "hint",
      text: "drops the heavy parts of the 3D floor so an older machine keeps a steady frame rate",
    })
  );
}

function aboutSection(root) {
  root.append(...section("about"));
  const card = el("div", { class: "card" });
  card.append(el("b", { text: "Tuğcan Topaloğlu" }));
  card.append(
    el("a", { href: "https://tugcan.dev", target: "_blank", rel: "noopener", text: "tugcan.dev" })
  );
  card.append(
    el("a", {
      href: "https://github.com/tugcantopaloglu",
      target: "_blank",
      rel: "noopener",
      text: "github.com/tugcantopaloglu",
    })
  );
  root.append(card);
}

let host = null;
let timers = [];

export function models(providerId) {
  return providerId ? candidatesFor(providerId) : catalogue.providers;
}

export function reloadCatalogue() {
  return api("/models")
    .then((data) => {
      catalogue = data;
      return catalogue;
    })
    .catch(() => catalogue);
}

function pane(name) {
  const box = el("div");
  panes[name] = box;
  host.append(box);
  return box;
}

function redraw() {
  if (!host) return;
  for (const t of timers) clearInterval(t);
  timers = [];
  verdictWatchers = [];
  host.replaceChildren(el("div", { class: "hint", text: "reading the studio settings" }));

  Promise.all([api("/roles"), api("/providers"), reloadCatalogue()])
    .then(([roles, providers]) => {
      host.replaceChildren();
      crewSection(pane("crew"), roles, providers);
      pane("models");
      paintModels();
      providerSection(pane("providers"), providers);
      enginesSection(pane("engines"));
      limitsSection(pane("limits"));
      musicSection(pane("music"));
      floorSection(pane("floor"));
      aboutSection(pane("about"));
    })
    .catch((err) => {
      host.replaceChildren(
        el("div", { class: "bad", text: `settings could not load: ${err.message}` })
      );
    });
}

export function mount(root) {
  host = root;
  settings.load().then(redraw);
}
