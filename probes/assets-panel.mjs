import { join } from "node:path";
import { pathToFileURL } from "node:url";

import { installDom, stageModules } from "./floor-dom.mjs";

const webDir = join(process.cwd(), "crates", "studio-server", "web");
const failures = [];

function check(what, passed, detail) {
  if (passed) {
    console.log(`  ok   ${what}`);
    return;
  }
  failures.push(what);
  console.log(`  FAIL ${what}${detail ? ` (${detail})` : ""}`);
}

function walk(node, out = []) {
  if (!node || typeof node !== "object") return out;
  out.push(node);
  for (const child of node.children || []) walk(child, out);
  return out;
}

function textOf(node) {
  return walk(node)
    .map((n) => n.textContent || "")
    .join(" ");
}

function kindsFor(ready) {
  return [
    {
      key: "character",
      title: "character",
      shape: "a rig",
      draws: false,
      cuts_out: false,
      ready: ready.character !== false,
      blockers: ready.character === false ? ["no engine is known for this project"] : [],
      makes: "assets/concept/example.png",
    },
    {
      key: "prop",
      title: "prop",
      shape: "one static object",
      draws: false,
      cuts_out: false,
      ready: ready.prop !== false,
      blockers: [],
      makes: "assets/concept/example.png",
    },
    {
      key: "sprite",
      title: "sprite",
      shape: "one subject lifted off its background",
      draws: true,
      cuts_out: true,
      ready: ready.sprite !== false,
      blockers: ready.sprite === false ? ["python is not on PATH"] : [],
      makes: "assets/sprites/example.png",
    },
    {
      key: "texture",
      title: "texture",
      shape: "a surface that tiles",
      draws: true,
      cuts_out: false,
      ready: true,
      blockers: [],
      makes: "assets/textures/example.png",
    },
  ];
}

function overview(options = {}) {
  return {
    program: "codex",
    installed: true,
    path: "C:/npm/codex.cmd",
    enabled: options.enabled !== false,
    engine: options.engine === undefined ? "web" : options.engine,
    ready: true,
    can_draw: options.can_draw !== false,
    can_model: options.can_model !== false,
    blockers: options.blockers || [],
    image_blockers: [],
    model_blockers: [],
    setting: "assets.enabled",
    concept_setting: "assets.concept",
    kinds: kindsFor(options.ready || {}),
    makes: options.engine === null ? null : { factory: "src/models/example.js", export: null },
    how: "codex draws raster sprites and textures with its built-in image tool",
    model: "gpt-5.6-sol",
    concept: true,
    default_model: "gpt-5.6-sol",
    assets: options.assets || [],
    project: "C:/games/demo",
  };
}

let asked = null;
let answer = null;

function serve(url, init) {
  const path = String(url);
  if (path.startsWith("/assets/generate")) {
    asked = JSON.parse(init.body);
    return answer;
  }
  if (path.startsWith("/assets")) return serve.view;
  if (path.startsWith("/models")) return { providers: [] };
  return {};
}

async function main() {
  installDom();
  globalThis.localStorage.setItem("studio.project", "proj_demo");
  globalThis.fetch = async (url, init = {}) => ({
    ok: true,
    status: 200,
    json: async () => serve(url, init),
    text: async () => JSON.stringify(serve(url, init)),
  });

  const dir = stageModules(webDir);
  const bus = await import(pathToFileURL(join(dir, "bus.js")).href);
  const panel = await import(pathToFileURL(join(dir, "assets.js")).href);
  bus.setProject("proj_demo");

  console.log("assets panel");

  serve.view = overview();
  const host = globalThis.document.createElement("div");
  panel.mount(host);
  await new Promise((r) => setTimeout(r, 30));

  const body = textOf(host);
  check("the panel no longer claims codex cannot draw", !/cannot draw/.test(body), body.slice(0, 120));
  check("it offers every kind the daemon reports", /sprite/.test(body) && /texture/.test(body));
  check(
    "a sprite says it lands with the sprites",
    /assets\/sprites\/example\.png/.test(body),
    body.slice(0, 200)
  );

  const options = walk(host).filter((n) => n.tagName === "OPTION");
  check("the picker lists four kinds", options.length === 4, `saw ${options.length}`);

  const view = overview();
  check(
    "a model kind shows the engine path rather than the image path",
    panel.destinationOf(view, "character").includes("src/models/example.js"),
    panel.destinationOf(view, "character")
  );
  check(
    "a texture kind shows its own folder",
    panel.destinationOf(view, "texture") === "lands at assets/textures/example.png",
    panel.destinationOf(view, "texture")
  );

  check(
    "the image url names the project and the path",
    panel.imageUrl("proj_demo", "assets/sprites/a.png").startsWith(
      "/assets/image?project=proj_demo&path=assets%2Fsprites%2Fa.png"
    ),
    panel.imageUrl("proj_demo", "assets/sprites/a.png")
  );

  serve.view = overview({ ready: { sprite: false } });
  panel.mount(host);
  await new Promise((r) => setTimeout(r, 30));
  const blocked = walk(host);
  const button = blocked.find((n) => n.tagName === "BUTTON");
  check("a blocked kind disables the generate button", button && button.disabled === true);
  check(
    "and says why in the form",
    /python is not on PATH/.test(textOf(host)),
    textOf(host).slice(0, 160)
  );

  serve.view = overview({
    assets: [
      {
        kind: "sprite",
        name: "Health Potion",
        slug: "health_potion",
        image: "assets/sprites/health_potion.png",
        factory: null,
        width: 1024,
        height: 1024,
        transparent: true,
        meshes: 0,
      },
    ],
  });
  panel.mount(host);
  await new Promise((r) => setTimeout(r, 30));
  const images = walk(host).filter((n) => n.tagName === "IMG");
  check("a generated sprite is previewed, not just named", images.length === 1, `saw ${images.length}`);
  check(
    "and the preview points at the serving route",
    images[0] && String(images[0].src).includes("/assets/image?project=proj_demo"),
    images[0] && images[0].src
  );

  serve.view = overview({ enabled: false });
  panel.mount(host);
  await new Promise((r) => setTimeout(r, 30));
  check(
    "switching it off hides the form",
    !walk(host).some((n) => n.tagName === "TEXTAREA"),
    textOf(host).slice(0, 120)
  );

  globalThis.fetch = async () => {
    throw new Error("the daemon is not answering");
  };
  panel.mount(host);
  await new Promise((r) => setTimeout(r, 30));
  check(
    "a failed read is reported rather than swallowed",
    /could not report on assets/.test(textOf(host)),
    textOf(host).slice(0, 120)
  );

  console.log(failures.length ? `\n${failures.length} failed` : "\nall panel checks passed");
  process.exit(failures.length ? 1 : 0);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
