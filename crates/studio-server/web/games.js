import { api, el, onEvent, onProject, project, setProject, toast } from "/bus.js";
import { folderPicker } from "/browse.js";

function ago(iso) {
  if (!iso) return "never";
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return "never";
  const seconds = Math.max(0, (Date.now() - then) / 1000);
  if (seconds < 60) return "just now";
  const minutes = seconds / 60;
  if (minutes < 60) return Math.floor(minutes) + "m ago";
  const hours = minutes / 60;
  if (hours < 24) return Math.floor(hours) + "h ago";
  const days = hours / 24;
  if (days < 30) return Math.floor(days) + "d ago";
  const months = days / 30;
  if (months < 12) return Math.floor(months) + "mo ago";
  return Math.floor(months / 12) + "y ago";
}

function originLabel(game) {
  if (game.origin === "adopted") return "adopted";
  if (game.origin === "built") return "built here";
  return "origin unknown";
}

function historyLine(game) {
  if (!game.exists) return "the folder this game lived in is gone";
  if (!game.git) return "not a git repo, so nothing can be reverted";
  if (game.commits === undefined) return "a git repo";
  const commits = game.commits === 1 ? "1 commit" : game.commits + " commits";
  return commits + " · last worked " + ago(game.last_worked);
}

function summaryBlock(game, onRead) {
  const box = el("div", { style: "display:grid;gap:5px" });
  const s = game.summary;

  if (!s) {
    box.append(el("div", { class: "hint", text: "nobody has read this game yet" }));
  } else {
    box.append(el("div", { style: "font-size:12px;line-height:1.45", text: s.text }));
    if (s.mechanics && s.mechanics.length) {
      const row = el("div", { style: "display:flex;flex-wrap:wrap;gap:4px" });
      for (const m of s.mechanics) {
        row.append(
          el("span", {
            class: "k",
            title: m.note || "",
            style: "border:1px solid var(--line-2);border-radius:6px;padding:1px 6px",
            text: m.name,
          })
        );
      }
      box.append(row);
    }
    box.append(
      el("div", {
        class: s.fresh === false ? "warn" : "k",
        style: "font-size:10.5px",
        text:
          s.fresh === false
            ? "stale: the game has changed since this was read " + ago(s.generated)
            : "read " + ago(s.generated),
      })
    );
  }

  const button = el("button", {
    text: s ? "re-read" : "summarise",
    onclick: () => onRead(button),
  });
  if (!game.exists) button.disabled = true;
  box.append(el("div", { class: "row" }, el("span", {}), button));
  return box;
}

function selectEverywhere(game) {
  setProject(game.id);
  const select = document.getElementById("project");
  if (!select) return;
  if (!Array.from(select.options).some((o) => o.value === game.id)) {
    select.append(el("option", { value: game.id, title: game.root, text: game.name }));
  }
  select.classList.remove("none");
  select.value = game.id;
}

function card(game, onRead) {
  const node = el("div", { class: "card" });
  const selected = project() === game.id;
  node.style.cursor = "pointer";
  node.style.borderColor = selected ? "var(--running)" : "";

  const head = el("div", { class: "row" });
  head.append(el("b", { text: game.name }));
  head.append(
    el("span", {
      class: "k",
      text: game.engine + " · " + originLabel(game),
    })
  );
  node.append(head);
  node.append(el("div", { class: "k", text: game.root }));
  node.append(
    el("div", { class: game.exists ? "k" : "bad", text: historyLine(game) })
  );
  node.append(summaryBlock(game, onRead));

  node.addEventListener("click", (ev) => {
    if (ev.target.tagName === "BUTTON") return;
    selectEverywhere(game);
  });
  return node;
}

export function mount(root) {
  root.innerHTML = "";

  root.append(el("div", { class: "sec", text: "games" }));
  root.append(
    el("div", {
      class: "hint",
      text: "every game the studio knows. Pick one to point the whole floor at it.",
    })
  );

  const list = el("div", { style: "display:grid;gap:8px" });
  root.append(list);

  root.append(el("div", { class: "sec", text: "adopt a game" }));
  root.append(
    el("div", {
      class: "hint",
      text: "point at a game the crew did not build. Nothing in the folder is touched.",
    })
  );

  const name = el("input", { type: "text", placeholder: "what to call it" });
  const chosen = el("div", {
    class: "k",
    style: "overflow-wrap:anywhere",
    text: "no folder chosen yet",
  });
  const engine = el("select");
  engine.append(el("option", { value: "auto", text: "detect the engine" }));
  const git = el("input", { type: "checkbox" });
  const adopt = el("button", { text: "adopt" });
  const browse = el("button", { text: "browse" });
  let chosenPath = "";
  let pickerBox = null;

  const where = el("div", { class: "row" }, chosen, browse);
  root.append(el("div", { class: "field" }, el("label", { text: "name" }), name));
  root.append(where);
  root.append(el("div", { class: "field" }, el("label", { text: "engine" }), engine));
  root.append(
    el("label", { class: "check" }, git, el("span", { text: "track it with git" }))
  );
  root.append(
    el("div", { class: "hint", text: "if the folder is not a repo yet, the crew makes one" })
  );
  root.append(adopt);

  browse.onclick = () => {
    if (pickerBox) {
      pickerBox.remove();
      pickerBox = null;
      return;
    }
    pickerBox = folderPicker({
      label: "folder",
      onPick: (path) => {
        chosenPath = path;
        chosen.textContent = path;
        if (!name.value.trim()) {
          name.value = path.split(/[\\/]/).filter(Boolean).pop() || "";
        }
        pickerBox.remove();
        pickerBox = null;
      },
    }).node;
    where.after(pickerBox);
  };

  async function read(game, button) {
    button.disabled = true;
    button.textContent = "reading";
    try {
      const answer = await api("/games/summarize", { body: { project: game.id } });
      toast(
        typeof answer === "object" && answer.cached
          ? game.name + " was already read; nothing was spent"
          : "the designer is reading " + game.name
      );
    } catch (err) {
      toast(err.message);
    }
    await refresh();
  }

  function draw(games) {
    list.innerHTML = "";
    if (!games.length) {
      list.append(
        el("div", { class: "hint", text: "no games yet. Adopt one, or create a project." })
      );
      return;
    }
    for (const game of games) {
      list.append(card(game, (button) => read(game, button)));
    }
  }

  async function fillIn(games) {
    let details;
    try {
      details = await api("/games/detail");
    } catch (err) {
      return;
    }
    const byId = new Map(details.map((d) => [d.id, d]));
    let moved = false;
    for (const game of games) {
      const d = byId.get(game.id);
      if (!d) continue;
      game.commits = d.commits;
      game.last_worked = d.last_worked;
      if (d.origin) game.origin = d.origin;
      if (game.summary && d.fresh !== null) game.summary.fresh = d.fresh;
      moved = true;
    }
    if (moved) draw(games);
  }

  async function refresh() {
    let games;
    try {
      games = await api("/games");
    } catch (err) {
      list.innerHTML = "";
      list.append(el("div", { class: "bad", text: err.message }));
      return;
    }
    draw(games);
    if (games.length) await fillIn(games);
  }

  adopt.onclick = async () => {
    if (!name.value.trim() || !chosenPath) {
      toast("a game needs a name and a folder");
      return;
    }
    adopt.disabled = true;
    try {
      const made = await api("/games/adopt", {
        body: {
          name: name.value.trim(),
          root: chosenPath,
          engine: engine.value,
          git: git.checked,
        },
      });
      setProject(made.id);
      toast(
        made.note || made.name + " adopted as a " + made.engine + " game"
      );
      name.value = "";
      chosenPath = "";
      chosen.textContent = "no folder chosen yet";
      await refresh();
    } catch (err) {
      toast(err.message);
    }
    adopt.disabled = false;
  };

  api("/games/engines")
    .then((rows) => {
      for (const e of rows) {
        engine.append(
          el("option", {
            value: e.id,
            title: "looks for " + e.markers.join(" and "),
            text: e.display_name,
          })
        );
      }
    })
    .catch(() => {});

  onProject(() => refresh());
  onEvent((ev) => {
    if (ev.type === "game_summarized") refresh();
  });
  refresh();
}
