use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::settings::on_path;
use crate::AppState;
use studio_engine::EngineProfile;
use studio_settings::Settings;

pub const PROGRAM: &str = "codex";
pub const SETTING_ENABLED: &str = "assets.enabled";
pub const SETTING_MODEL: &str = "assets.model";
pub const MANIFEST: &str = "assets.json";
pub const GENERATION_CAP: Duration = Duration::from_secs(600);
pub const DEFAULT_MODEL: &str = "gpt-5.6-sol";

pub fn model_in(settings: &Settings) -> String {
    settings
        .string(SETTING_MODEL)
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .unwrap_or(DEFAULT_MODEL)
        .to_string()
}

pub fn path_extensions() -> Vec<String> {
    if !cfg!(windows) {
        return Vec::new();
    }
    std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
        .split(';')
        .map(|e| e.trim().to_lowercase())
        .filter(|e| !e.is_empty())
        .collect()
}

pub fn launcher_for(found: &Path) -> (PathBuf, Vec<String>) {
    let ext = found
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if cfg!(windows) && (ext == "cmd" || ext == "bat") {
        let shell = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into());
        return (
            PathBuf::from(shell),
            vec!["/c".into(), found.to_string_lossy().into_owned()],
        );
    }
    (found.to_path_buf(), Vec::new())
}

pub fn spawnable(program: &str) -> Option<(PathBuf, Vec<String>)> {
    let raw = std::env::var_os("PATH")?;
    let extensions = path_extensions();
    for dir in std::env::split_paths(&raw) {
        for ext in &extensions {
            let candidate = dir.join(format!("{program}{ext}"));
            if candidate.is_file() {
                return Some(launcher_for(&candidate));
            }
        }
        if !cfg!(windows) {
            let bare = dir.join(program);
            if bare.is_file() {
                return Some((bare, Vec::new()));
            }
        }
    }
    None
}

pub fn inside(project: &Path, relative: &Path) -> Result<PathBuf, String> {
    for part in relative.components() {
        if !matches!(part, std::path::Component::Normal(_)) {
            return Err(format!(
                "{} is not a plain path inside the project, so nothing will be written to it",
                relative.display()
            ));
        }
    }
    Ok(project.join(relative))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    Character,
    Prop,
}

impl AssetKind {
    pub const ALL: [AssetKind; 2] = [AssetKind::Character, AssetKind::Prop];

    pub fn key(&self) -> &'static str {
        match self {
            AssetKind::Character => "character",
            AssetKind::Prop => "prop",
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            AssetKind::Character => "character",
            AssetKind::Prop => "prop",
        }
    }

    pub fn from_key(key: &str) -> Option<AssetKind> {
        AssetKind::ALL.into_iter().find(|k| k.key() == key)
    }

    pub fn shape(&self) -> &'static str {
        match self {
            AssetKind::Character => {
                "A character stands on the ground plane and reads as a rig: a root group with \
                 separately named head, torso, arm and leg parts, each positioned so an \
                 animator can rotate it about a sensible joint. It is about 1.7 units tall, \
                 its lowest point sits at y = 0, and it faces -Z."
            }
            AssetKind::Prop => {
                "A prop is a single static object with no limbs and no implied joints. It \
                 rests on the ground plane with its lowest point at y = 0, is centred on the \
                 x and z axes, and stays under 40 meshes so a level can place many of them."
            }
        }
    }
}

pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut hungry = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.extend(ch.to_lowercase());
            hungry = true;
        } else if hungry {
            out.push('_');
            hungry = false;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub engine: String,
    pub factory: PathBuf,
    pub export: Option<PathBuf>,
    pub proof: PathBuf,
}

pub fn plan_for(engine: &str, slug: &str) -> Option<Plan> {
    match engine {
        "web" => Some(Plan {
            engine: engine.to_string(),
            factory: Path::new("src").join("models").join(format!("{slug}.js")),
            export: None,
            proof: Path::new(".studio-out")
                .join("assets")
                .join(format!("{slug}.glb")),
        }),
        "godot" | "unity" | "ue5" => {
            let export = Path::new("assets")
                .join("models")
                .join(format!("{slug}.glb"));
            Some(Plan {
                engine: engine.to_string(),
                factory: Path::new("tools").join("models").join(format!("{slug}.mjs")),
                export: Some(export.clone()),
                proof: export,
            })
        }
        _ => None,
    }
}

pub const REFERENCE_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "webp", "gif"];

pub fn reference_in(project: &Path, given: &str) -> Result<PathBuf, String> {
    let given = given.trim();
    let ext = given
        .rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .unwrap_or_default();
    if !REFERENCE_EXTENSIONS.contains(&ext.as_str()) {
        return Err(format!(
            "a reference has to be an image codex can read ({}), and {given} is not one",
            REFERENCE_EXTENSIONS.join(", ")
        ));
    }
    let path = Path::new(given);
    if path.is_absolute() || given.contains("..") || given.contains(':') {
        return Err(
            "a reference is a path inside the project, so it cannot be absolute or climb out of it"
                .to_string(),
        );
    }

    let joined = project.join(path);
    let settled = joined
        .canonicalize()
        .map_err(|e| format!("there is no reference image at {}: {e}", joined.display()))?;
    let root = project
        .canonicalize()
        .map_err(|e| format!("could not resolve the project at {}: {e}", project.display()))?;
    if !settled.starts_with(&root) {
        return Err(format!(
            "{given} resolves outside the project, and only files inside it are sent to {PROGRAM}"
        ));
    }
    if !settled.is_file() {
        return Err(format!("there is no reference image at {}", settled.display()));
    }
    Ok(settled)
}

pub fn engine_of(project: &Path) -> Option<String> {
    let profiles = EngineProfile::builtin();
    studio_engine::detect(project, &profiles)
        .first()
        .map(|d| d.id.clone())
}

pub fn enabled_in(settings: &Settings) -> bool {
    settings.bool(SETTING_ENABLED, false)
}

pub fn blockers(installed: bool, enabled: bool, engine: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    if !enabled {
        out.push(format!(
            "asset generation is switched off, which is how it ships; tick it on in the assets \
             panel when you want the crew to spend Codex budget on {PROGRAM}"
        ));
    }
    if !installed {
        out.push(format!(
            "{PROGRAM} is not on PATH, so there is nothing to drive; install it with \
             `npm i -g @openai/codex` and sign in with `{PROGRAM} login`, or leave this off and \
             the art crew keeps building factories by hand exactly as it does today"
        ));
    }
    match engine {
        None => out.push(
            "no engine is known for this project, so the studio cannot say where a model would \
             go; pick a project on the floor and let it detect or scaffold an engine first"
                .to_string(),
        ),
        Some(id) if plan_for(id, "probe").is_none() => out.push(format!(
            "the {id} engine has no procedural three.js model path in this studio, so a \
             generated factory would have nowhere to land; use this on a web, godot, unity or \
             ue5 project"
        )),
        Some(_) => {}
    }
    out
}

#[derive(Debug, Clone)]
pub struct Capability {
    pub installed: bool,
    pub path: Option<PathBuf>,
    pub enabled: bool,
    pub engine: Option<String>,
    pub blockers: Vec<String>,
}

impl Capability {
    pub fn ready(&self) -> bool {
        self.blockers.is_empty()
    }
}

pub fn capability(studio_dir: &Path, project: Option<&Path>) -> Capability {
    let stored = Settings::load(&Settings::path_in(studio_dir)).unwrap_or_default();
    let found = on_path(PROGRAM);
    let engine = project.and_then(engine_of);
    let enabled = enabled_in(&stored);
    Capability {
        installed: found.is_some(),
        path: found,
        enabled,
        blockers: blockers(on_path(PROGRAM).is_some(), enabled, engine.as_deref()),
        engine,
    }
}

pub const ANSWER_SCHEMA: &str = r#"{
  "type": "object",
  "additionalProperties": false,
  "required": ["source", "notes"],
  "properties": {
    "source": { "type": "string" },
    "notes": { "type": "string" }
  }
}"#;

pub fn prompt_for(
    kind: AssetKind,
    name: &str,
    description: &str,
    plan: &Plan,
    reference: Option<&str>,
) -> String {
    let slug = slugify(name);
    let looking = match reference {
        Some(_) => "An image of this asset is attached. Read it and match its silhouette, \
                    proportions and palette; where the words below and the image disagree, the \
                    image wins."
            .to_string(),
        None => "There is no reference image, so work from the description alone.".to_string(),
    };
    let destination = match &plan.export {
        Some(glb) => format!(
            "The studio writes your source to {} and then runs \
             `node tools/model_export.mjs {} {}` to bake the .glb the {} engine imports. \
             Textures, canvases and data URIs do not survive that export, so do not reach for \
             them: solid albedo colours and PBR scalars only.",
            plan.factory.display(),
            plan.factory.display(),
            glb.display(),
            plan.engine
        ),
        None => format!(
            "The studio writes your source to {} and loads it straight into the running \
             three.js scene. There is no bundler and no npm install at that path.",
            plan.factory.display()
        ),
    };

    format!(
        "You are producing one game asset for a {} project as procedural three.js source code. \
         You write no files and run no commands; return the source in your structured answer and \
         the studio writes it where the engine expects it.\n\n\
         The asset is a {} named \"{}\", referred to in code as {}.\n\n\
         What it is: {}\n\n\
         {}\n\n\
         {}\n\n\
         The source must obey every one of these, and the studio checks the ones it can:\n\
         - Export a single default function taking THREE as its only argument and returning a \
         THREE.Group.\n\
         - Import nothing at all. THREE arrives as the argument.\n\
         - Build geometry only from THREE primitives: Box, Sphere, Cylinder, Cone, Capsule, \
         Torus, Lathe, Extrude, Shape. No loaders, no external mesh data, no textures.\n\
         - Use MeshStandardMaterial with a solid colour plus roughness and metalness.\n\
         - Give the root group the name {} and give every child a name that says what part it \
         is, so the rest of the crew can find a limb without counting indices.\n\
         - {}\n\
         - Allocate nothing per frame: the factory builds and returns, it does not animate.\n\
         - Write no comments of any kind, not one line, not one trailing note. This studio's \
         code has none.\n\n\
         Put the complete file contents in the source field and one sentence about what you \
         built in the notes field.",
        plan.engine,
        kind.title(),
        name,
        slug,
        description.trim(),
        looking,
        destination,
        slug,
        kind.shape()
    )
}

pub const SKILL_NAME: &str = "codex-assets";

pub fn skill_body(engine: &str) -> String {
    let plan = plan_for(engine, "<slug>");
    let where_it_goes = match plan.as_ref().and_then(|p| p.export.as_ref()) {
        Some(_) => format!(
            "Write the factory to `tools/models/<slug>.mjs`, then bake it with\n\
             `node tools/model_export.mjs tools/models/<slug>.mjs assets/models/<slug>.glb`.\n\
             The {engine} engine imports the .glb; it never reads the factory."
        ),
        None => "Write the factory to `src/models/<slug>.js`. The browser imports it directly, \
                 so there is no export step; prove it loads with\n`node tools/model_export.mjs \
                 src/models/<slug>.js .studio-out/assets/<slug>.glb`."
            .to_string(),
    };

    format!(
        "---\nname: {SKILL_NAME}\ndescription: Turn a described or referenced game asset into a \
         procedural three.js factory by asking the codex CLI for the source, for characters and \
         props in a {engine} project.\n---\n\n\
         # Generating an asset with codex\n\n\
         `codex` writes code. It cannot draw a picture: `-i/--image` is an input, and there is no \
         image-generating subcommand. So an asset here means procedural three.js source that \
         builds the thing out of primitives, exactly like the img2threejs skill produces, and a \
         reference image is something codex reads rather than something it makes.\n\n\
         Use this only when the studio has switched it on. If `{PROGRAM}` is not on PATH, or \
         `{SETTING_ENABLED}` is not true in `.studio/settings.json`, build the factory by hand \
         instead and say that is what you did.\n\n\
         ## Ask for the source, do not let it write files\n\n\
         ```\n{PROGRAM} exec --skip-git-repo-check --sandbox read-only --color never \\\n  \
         --output-schema <schema.json> -o <answer.json> [-i <reference.png>] \"<the brief>\"\n\
         ```\n\n\
         The schema is `{{\"type\":\"object\",\"required\":[\"source\",\"notes\"]}}` with both \
         fields strings. Read `source` out of the answer and write it yourself. Asking codex to \
         write the file is worse: its sandbox on this machine reports read-only even when told \
         otherwise, and you lose control of which path it touches.\n\n\
         ## The contract the brief must carry\n\n\
         - one default export taking THREE as its only argument, returning a THREE.Group\n\
         - no imports at all, no loaders, no textures, no canvas, no data URIs\n\
         - MeshStandardMaterial with a solid colour plus roughness and metalness\n\
         - a named root group and a name on every child, so a limb is found by name\n\
         - no comments of any kind\n\n\
         A character: {}\n\n\
         A prop: {}\n\n\
         ## Where it goes\n\n{where_it_goes}\n\n\
         A mesh count of zero means the factory built nothing; treat that as a failure, keep the \
         project as you found it, and report that the asset was not generated rather than \
         leaving a file nothing renders.\n",
        AssetKind::Character.shape(),
        AssetKind::Prop.shape()
    )
}

pub fn parse_answer(raw: &str) -> Result<(String, String), String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(format!(
            "{PROGRAM} finished without writing an answer; run it once by hand to see what it \
             says, then try again"
        ));
    }
    let parsed: Value = serde_json::from_str(trimmed).map_err(|e| {
        format!(
            "{PROGRAM} answered with something that is not the json schema it was given ({e}); \
             the first 200 characters were: {}",
            trimmed.chars().take(200).collect::<String>()
        )
    })?;
    let source = parsed
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let notes = parsed
        .get("notes")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if source.trim().is_empty() {
        return Err(format!(
            "{PROGRAM} answered with an empty source field, so there is no factory to write"
        ));
    }
    Ok((source, notes))
}

pub fn looks_like_a_factory(source: &str) -> Result<(), String> {
    if !source.contains("export default") {
        return Err(
            "the generated source has no `export default` function, so nothing could load it as \
             a model factory"
                .to_string(),
        );
    }
    for line in source.lines() {
        let head = line.trim_start();
        if head.starts_with("import ") || head.starts_with("import(") {
            return Err(format!(
                "the generated source imports something ({}), and a factory has to take THREE as \
                 its argument instead",
                head.chars().take(60).collect::<String>()
            ));
        }
        if head.starts_with("require(") || head.contains("= require(") {
            return Err(
                "the generated source calls require(), which no path in this studio resolves"
                    .to_string(),
            );
        }
    }
    Ok(())
}

pub fn parse_export_line(text: &str) -> Option<(u64, usize)> {
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with("wrote ") {
            continue;
        }
        let inside = line.split_once('(')?.1.rsplit_once(')')?.0;
        let mut bytes = None;
        let mut meshes = None;
        for part in inside.split(',') {
            let part = part.trim();
            if let Some(n) = part.strip_suffix(" bytes") {
                bytes = n.parse::<u64>().ok();
            }
            if let Some(n) = part.strip_suffix(" mesh(es)") {
                meshes = n.parse::<usize>().ok();
            }
        }
        if let (Some(bytes), Some(meshes)) = (bytes, meshes) {
            return Some((bytes, meshes));
        }
    }
    None
}

#[derive(Debug, Clone)]
pub struct Generated {
    pub kind: String,
    pub name: String,
    pub slug: String,
    pub factory: String,
    pub export: Option<String>,
    pub meshes: usize,
    pub bytes: u64,
    pub notes: String,
    pub log: String,
}

impl Generated {
    pub fn to_value(&self) -> Value {
        serde_json::json!({
            "kind": self.kind,
            "name": self.name,
            "slug": self.slug,
            "factory": self.factory,
            "export": self.export,
            "meshes": self.meshes,
            "bytes": self.bytes,
            "notes": self.notes,
            "log": self.log,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub kind: AssetKind,
    pub name: String,
    pub description: String,
    pub reference: Option<PathBuf>,
    pub model: String,
    pub overwrite: bool,
}

fn work_dir(project: &Path) -> PathBuf {
    project.join(".studio-out").join("assets")
}

pub fn verbatim_refusal(recorded: &str) -> Option<String> {
    recorded
        .lines()
        .find(|line| line.contains("not supported when using Codex"))
        .map(|line| {
            let said = line.rsplit_once("\"message\":\"").map(|(_, tail)| tail).unwrap_or(line);
            said.trim_end_matches(['}', '"', ' '])
                .split("\",\"")
                .next()
                .unwrap_or(said)
                .trim()
                .to_string()
        })
}

pub fn diagnose(recorded: &str, model: &str) -> Option<String> {
    if recorded.contains("not supported when using Codex") {
        let said = verbatim_refusal(recorded).unwrap_or_default();
        return Some(format!(
            "{PROGRAM} refused the model {model}. That is a restriction on this ChatGPT account, \
             not a fault in the studio, and codex said so in these words: \"{said}\" Pick another \
             in the assets panel: a model the catalogue merely lists is only known to exist, so \
             prefer one the studio has already seen answer"
        ));
    }
    if recorded.contains("token_expired") || recorded.contains("refresh token was already used") {
        return Some(format!(
            "{PROGRAM}'s sign-in has expired and only a person can renew it; run \
             `{PROGRAM} logout && {PROGRAM} login` and try again"
        ));
    }
    if recorded.contains("Reading additional input from stdin") {
        return Some(format!(
            "{PROGRAM} was waiting on stdin instead of working, which means it was started \
             without its input closed; this is a studio bug, not a codex one"
        ));
    }
    None
}

fn run_codex(
    project: &Path,
    req: &Request,
    brief: &Path,
    schema: &Path,
    answer: &Path,
    log: &Path,
) -> Result<String, String> {
    let out = std::fs::File::create(log)
        .map_err(|e| format!("could not open {} to record the run: {e}", log.display()))?;
    let errs = out
        .try_clone()
        .map_err(|e| format!("could not record {PROGRAM}'s diagnostics: {e}"))?;
    let asking = std::fs::File::open(brief)
        .map_err(|e| format!("could not reopen {} to send it: {e}", brief.display()))?;

    let (program, leading) = spawnable(PROGRAM).ok_or_else(|| {
        format!(
            "{PROGRAM} is on PATH but not in a form this machine can start; reinstall it with \
             `npm i -g @openai/codex` so a launcher lands on PATH"
        )
    })?;

    let mut cmd = std::process::Command::new(program);
    cmd.args(leading)
        .arg("exec")
        .arg("--skip-git-repo-check")
        .args(["--sandbox", "read-only"])
        .args(["--color", "never"])
        .arg("-C")
        .arg(project)
        .arg("--output-schema")
        .arg(schema)
        .arg("-o")
        .arg(answer);
    cmd.args(["-m", req.model.as_str()]);
    if let Some(reference) = req.reference.as_deref() {
        cmd.arg("-i").arg(reference);
    }
    cmd.stdin(std::process::Stdio::from(asking))
        .stdout(out)
        .stderr(errs);

    let mut child = cmd.spawn().map_err(|e| {
        format!("{PROGRAM} would not start: {e}; check it is still on PATH and try again")
    })?;

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let said = std::fs::read_to_string(log).unwrap_or_default();
                if !status.success() {
                    return Err(match diagnose(&said, &req.model) {
                        Some(why) => why,
                        None => format!(
                            "{PROGRAM} exited {}; its whole run is recorded in {} and usually \
                             says why",
                            status.code().unwrap_or(-1),
                            log.display()
                        ),
                    });
                }
                return Ok(said);
            }
            Ok(None) if started.elapsed() > GENERATION_CAP => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{PROGRAM} was still working after {} seconds so the studio stopped it; ask \
                     for a simpler asset or run it by hand to see where it got stuck",
                    GENERATION_CAP.as_secs()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(250)),
            Err(e) => return Err(format!("lost track of the {PROGRAM} process: {e}")),
        }
    }
}

pub fn verify(project: &Path, plan: &Plan) -> Result<(u64, usize), String> {
    let node = on_path("node").ok_or_else(|| {
        "node is not on PATH, so the studio cannot check that the generated factory loads; \
         install node 18 or newer"
            .to_string()
    })?;
    let bridge = project.join("tools").join("model_export.mjs");
    if !bridge.is_file() {
        return Err(format!(
            "{} is missing, so there is no bridge to load the factory through; open the project \
             once so the studio installs its engine helpers",
            bridge.display()
        ));
    }

    let out = std::process::Command::new(node)
        .arg(&bridge)
        .arg(project.join(&plan.factory))
        .arg(project.join(&plan.proof))
        .current_dir(project)
        .output()
        .map_err(|e| format!("could not run the model export bridge: {e}"))?;

    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !out.status.success() {
        return Err(format!(
            "the generated factory did not load: {}",
            said.trim().chars().take(600).collect::<String>()
        ));
    }
    match parse_export_line(&said) {
        Some((_, 0)) => Err(
            "the generated factory loaded but produced no meshes, so there is nothing to see"
                .to_string(),
        ),
        Some((bytes, meshes)) => Ok((bytes, meshes)),
        None => Err(format!(
            "the model export bridge said something this build could not read: {}",
            said.trim().chars().take(300).collect::<String>()
        )),
    }
}

pub fn generate(project: &Path, cap: &Capability, req: &Request) -> Result<Generated, String> {
    if !cap.ready() {
        return Err(cap.blockers.join(" "));
    }
    let slug = slugify(&req.name);
    if slug.is_empty() {
        return Err("an asset needs a name with letters or digits in it".to_string());
    }
    if req.description.trim().is_empty() {
        return Err(
            "an asset needs a description; the words are the only thing telling codex what to \
             build"
                .to_string(),
        );
    }
    let engine = cap.engine.as_deref().unwrap_or_default();
    let plan = plan_for(engine, &slug)
        .ok_or_else(|| format!("the {engine} engine has no place for a generated model"))?;
    let factory = inside(project, &plan.factory)?;
    let proof = inside(project, &plan.proof)?;
    if factory.exists() && !req.overwrite {
        return Err(format!(
            "{} already exists, and a generated asset never replaces a file that is already \
             there; tick replace in the assets panel if you did mean to overwrite it",
            plan.factory.display()
        ));
    }

    let work = work_dir(project);
    std::fs::create_dir_all(&work)
        .map_err(|e| format!("could not create {}: {e}", work.display()))?;
    let schema = work.join(format!("{slug}.schema.json"));
    let answer = work.join(format!("{slug}.answer.json"));
    let log = work.join(format!("{slug}.codex.log"));
    std::fs::write(&schema, ANSWER_SCHEMA)
        .map_err(|e| format!("could not write {}: {e}", schema.display()))?;
    let _ = std::fs::remove_file(&answer);

    let prompt = prompt_for(
        req.kind,
        &req.name,
        &req.description,
        &plan,
        req.reference.as_ref().map(|_| "attached"),
    );
    let brief = work.join(format!("{slug}.brief.txt"));
    std::fs::write(&brief, &prompt)
        .map_err(|e| format!("could not write {}: {e}", brief.display()))?;
    let said = run_codex(project, req, &brief, &schema, &answer, &log)?;
    let raw = match std::fs::read_to_string(&answer) {
        Ok(raw) => raw,
        Err(e) => {
            return Err(diagnose(&said, &req.model).unwrap_or_else(|| {
                format!(
                    "{PROGRAM} finished without leaving an answer at {} ({e}); its whole run is \
                     recorded in {}",
                    answer.display(),
                    log.display()
                )
            }))
        }
    };
    let (source, notes) = parse_answer(&raw)?;
    looks_like_a_factory(&source)?;

    if let Some(parent) = factory.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    let existed = std::fs::read_to_string(&factory).ok();
    std::fs::write(&factory, &source)
        .map_err(|e| format!("could not write {}: {e}", factory.display()))?;
    if let Some(parent) = proof.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    match verify(project, &plan) {
        Ok((bytes, meshes)) => {
            let record = Generated {
                kind: req.kind.key().to_string(),
                name: req.name.clone(),
                slug,
                factory: plan.factory.to_string_lossy().replace('\\', "/"),
                export: plan
                    .export
                    .as_ref()
                    .map(|p| p.to_string_lossy().replace('\\', "/")),
                meshes,
                bytes,
                notes,
                log: log.to_string_lossy().into_owned(),
            };
            remember(project, &record);
            Ok(record)
        }
        Err(why) => {
            match existed {
                Some(before) => {
                    let _ = std::fs::write(&factory, before);
                }
                None => {
                    let _ = std::fs::remove_file(&factory);
                }
            }
            Err(format!(
                "{why}. The project is untouched and the attempt is recorded in {}",
                log.display()
            ))
        }
    }
}

pub fn manifest_path(project: &Path) -> PathBuf {
    project.join(".studio").join(MANIFEST)
}

pub fn recorded(project: &Path) -> Vec<Value> {
    std::fs::read_to_string(manifest_path(project))
        .ok()
        .and_then(|body| serde_json::from_str::<Value>(&body).ok())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
}

fn remember(project: &Path, record: &Generated) {
    let mut rows = recorded(project);
    rows.retain(|r| r.get("slug").and_then(Value::as_str) != Some(record.slug.as_str()));
    rows.push(record.to_value());
    let path = manifest_path(project);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(body) = serde_json::to_string_pretty(&Value::Array(rows)) {
        let _ = std::fs::write(path, body);
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/assets", get(overview))
        .route("/assets/generate", post(generate_asset))
}

fn kinds_value() -> Vec<Value> {
    AssetKind::ALL
        .into_iter()
        .map(|k| serde_json::json!({"key": k.key(), "title": k.title(), "shape": k.shape()}))
        .collect()
}

fn capability_value(cap: &Capability, plan: Option<&Plan>) -> Value {
    serde_json::json!({
        "program": PROGRAM,
        "installed": cap.installed,
        "path": cap.path.as_ref().map(|p| p.to_string_lossy().into_owned()),
        "enabled": cap.enabled,
        "engine": cap.engine,
        "ready": cap.ready(),
        "blockers": cap.blockers,
        "setting": SETTING_ENABLED,
        "kinds": kinds_value(),
        "makes": plan.map(|p| serde_json::json!({
            "factory": p.factory.to_string_lossy().replace('\\', "/"),
            "export": p.export.as_ref().map(|e| e.to_string_lossy().replace('\\', "/")),
        })),
        "how": "codex cannot draw a picture; it reads an optional reference image and writes procedural three.js source, which the studio saves where the engine loads models from",
    })
}

async fn overview(State(state): State<AppState>, Query(q): Query<HashMap<String, String>>) -> Response {
    let root = q.get("project").and_then(|id| crate::project_root(&state, id));
    let cap = capability(&state.studio_dir, root.as_deref());
    let plan = cap
        .engine
        .as_deref()
        .and_then(|engine| plan_for(engine, "example"));

    let stored = Settings::load(&Settings::path_in(&state.studio_dir)).unwrap_or_default();
    let mut body = capability_value(&cap, plan.as_ref());
    body["model"] = Value::String(model_in(&stored));
    body["default_model"] = Value::String(DEFAULT_MODEL.to_string());
    body["model_setting"] = Value::String(SETTING_MODEL.to_string());
    body["model_note"] = Value::String(format!(
        "the studio always passes -m explicitly, because a codex config default can name a model \
         the account has retired; unset means {DEFAULT_MODEL}"
    ));
    body["assets"] = Value::Array(root.as_deref().map(recorded).unwrap_or_default());
    body["project"] = match root.as_deref() {
        Some(p) => Value::String(p.to_string_lossy().into_owned()),
        None => Value::Null,
    };
    axum::Json(body).into_response()
}

#[derive(Debug, Deserialize)]
pub struct GenerateRequest {
    pub project: String,
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub reference: Option<String>,
    #[serde(default)]
    pub overwrite: bool,
}

async fn generate_asset(State(state): State<AppState>, body: String) -> Response {
    let req: GenerateRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("an asset request needs project, kind and name: {e}"),
            )
                .into_response()
        }
    };
    let Some(kind) = AssetKind::from_key(&req.kind) else {
        return (
            StatusCode::BAD_REQUEST,
            format!(
                "{} is not an asset this studio makes; ask for one of {}",
                req.kind,
                AssetKind::ALL.map(|k| k.key()).join(" or ")
            ),
        )
            .into_response();
    };
    let Some(root) = crate::project_root(&state, &req.project) else {
        return (StatusCode::NOT_FOUND, "no such project".to_string()).into_response();
    };

    let stored = Settings::load(&Settings::path_in(&state.studio_dir)).unwrap_or_default();
    let model = model_in(&stored);
    let cap = capability(&state.studio_dir, Some(&root));
    let asked_for = req.reference.as_deref().map(str::trim).filter(|r| !r.is_empty());
    let reference = match asked_for.map(|r| reference_in(&root, r)) {
        Some(Ok(image)) => Some(image),
        Some(Err(why)) => return (StatusCode::BAD_REQUEST, why).into_response(),
        None => None,
    };

    let asked = Request {
        kind,
        name: req.name.clone(),
        description: req.description.clone(),
        reference,
        model,
        overwrite: req.overwrite,
    };
    let done = tokio::task::spawn_blocking(move || generate(&root, &cap, &asked)).await;

    match done {
        Ok(Ok(record)) => {
            let mut body = record.to_value();
            body["ok"] = Value::Bool(true);
            axum::Json(body).into_response()
        }
        Ok(Err(why)) => axum::Json(serde_json::json!({"ok": false, "reason": why})).into_response(),
        Err(e) => axum::Json(serde_json::json!({
            "ok": false,
            "reason": format!("the asset generator stopped unexpectedly: {e}"),
        }))
        .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use studio_store::Store;
    use tower::ServiceExt;

    fn state_in(slug: &str) -> AppState {
        let dir = std::env::temp_dir().join(format!("studio-assets-{slug}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = Arc::new(Store::open(dir.join("s.db")).unwrap());
        AppState::new(store).with_studio_dir(dir)
    }

    async fn get(state: AppState, uri: &str) -> (StatusCode, Value) {
        let req = axum::http::Request::builder()
            .uri(uri)
            .body(axum::body::Body::empty())
            .unwrap();
        let res = crate::router(state).oneshot(req).await.unwrap();
        let status = res.status();
        let raw = axum::body::to_bytes(res.into_body(), 4_000_000).await.unwrap();
        (status, serde_json::from_slice(&raw).unwrap_or(Value::Null))
    }

    async fn post(state: AppState, uri: &str, body: &str) -> (StatusCode, Value) {
        let req = axum::http::Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body.to_string()))
            .unwrap();
        let res = crate::router(state).oneshot(req).await.unwrap();
        let status = res.status();
        let raw = axum::body::to_bytes(res.into_body(), 4_000_000).await.unwrap();
        (status, serde_json::from_slice(&raw).unwrap_or(Value::Null))
    }

    #[test]
    fn a_missing_codex_is_reported_as_the_reason_with_the_command_that_installs_it() {
        let said = blockers(false, true, Some("web"));
        assert_eq!(said.len(), 1, "only the missing binary is wrong here");
        assert!(said[0].contains("not on PATH"));
        assert!(said[0].contains("npm i -g @openai/codex"));
    }

    #[test]
    fn the_feature_is_off_until_somebody_turns_it_on() {
        let fresh = Settings::new();
        assert!(!enabled_in(&fresh), "asset generation must ship switched off");

        let said = blockers(true, false, Some("web"));
        assert_eq!(said.len(), 1);
        assert!(said[0].contains("switched off"));
    }

    #[test]
    fn an_installed_and_enabled_codex_on_a_web_project_has_nothing_standing_in_its_way() {
        assert!(blockers(true, true, Some("web")).is_empty());
    }

    #[test]
    fn an_engine_with_no_model_path_says_so_rather_than_writing_a_file_nothing_reads() {
        let said = blockers(true, true, Some("python"));
        assert_eq!(said.len(), 1);
        assert!(said[0].contains("python"));

        let none = blockers(true, true, None);
        assert_eq!(none.len(), 1);
        assert!(none[0].contains("no engine is known"));
    }

    #[test]
    fn a_web_factory_lands_where_the_engine_profile_says_models_live() {
        let plan = plan_for("web", "scrap_scout").unwrap();
        assert_eq!(plan.factory, Path::new("src").join("models").join("scrap_scout.js"));
        assert!(plan.export.is_none(), "the browser loads the factory itself");
    }

    #[test]
    fn a_godot_factory_is_baked_to_the_glb_the_engine_imports() {
        for engine in ["godot", "unity", "ue5"] {
            let plan = plan_for(engine, "crate").unwrap();
            assert_eq!(plan.factory, Path::new("tools").join("models").join("crate.mjs"));
            assert_eq!(
                plan.export.as_deref(),
                Some(Path::new("assets").join("models").join("crate.glb").as_path())
            );
            assert_eq!(Some(plan.proof.as_path()), plan.export.as_deref());
        }
        assert!(plan_for("python", "crate").is_none());
    }

    #[test]
    fn an_asset_name_becomes_an_identifier_the_rest_of_the_crew_can_type() {
        assert_eq!(slugify("Scrapyard Scout"), "scrapyard_scout");
        assert_eq!(slugify("  Rusty  Crate!! "), "rusty_crate");
        assert_eq!(slugify("Mk-2 Turret"), "mk_2_turret");
        assert_eq!(slugify("???"), "");
    }

    #[test]
    fn the_prompt_tells_codex_the_destination_and_forbids_the_things_that_break_the_engine() {
        let plan = plan_for("web", "scout").unwrap();
        let prompt = prompt_for(AssetKind::Character, "Scout", "a wiry salvager", &plan, None);
        assert!(prompt.contains("src/models/scout.js") || prompt.contains("src\\models\\scout.js"));
        assert!(prompt.contains("no reference image"));
        assert!(prompt.contains("Import nothing"));
        assert!(prompt.contains("no comments"));
        assert!(prompt.contains("export a single default function") || prompt.contains("Export a single default function"));
    }

    #[test]
    fn a_reference_image_outranks_the_words_when_one_is_attached() {
        let plan = plan_for("web", "scout").unwrap();
        let prompt = prompt_for(AssetKind::Prop, "Scout", "a crate", &plan, Some("attached"));
        assert!(prompt.contains("the image wins"));
        assert!(!prompt.contains("no reference image"));
    }

    #[test]
    fn a_glb_engine_is_told_that_textures_do_not_survive_the_export() {
        let plan = plan_for("godot", "crate").unwrap();
        let prompt = prompt_for(AssetKind::Prop, "Crate", "a crate", &plan, None);
        assert!(prompt.contains("model_export.mjs"));
        assert!(prompt.contains("do not survive"));
    }

    #[test]
    fn a_character_and_a_prop_are_asked_for_different_shapes() {
        assert!(AssetKind::Character.shape().contains("rig"));
        assert!(AssetKind::Prop.shape().contains("static"));
        assert_eq!(AssetKind::from_key("character"), Some(AssetKind::Character));
        assert_eq!(AssetKind::from_key("tileset"), None);
    }

    #[test]
    fn an_answer_that_is_not_the_schema_is_refused_with_what_arrived_instead() {
        let err = parse_answer("I built you a lovely robot!").unwrap_err();
        assert!(err.contains("not the json schema"));
        assert!(err.contains("lovely robot"));

        assert!(parse_answer("").unwrap_err().contains("without writing an answer"));
        assert!(parse_answer(r#"{"source":"  ","notes":"x"}"#).unwrap_err().contains("empty source"));

        let (source, notes) = parse_answer(r#"{"source":"export default (T) => new T.Group();","notes":"a group"}"#).unwrap();
        assert!(source.contains("export default"));
        assert_eq!(notes, "a group");
    }

    #[test]
    fn source_that_could_never_load_is_caught_before_it_is_written_to_the_project() {
        assert!(looks_like_a_factory("export default function make(THREE) { return new THREE.Group(); }").is_ok());

        let imported = looks_like_a_factory("import * as THREE from 'three';\nexport default () => {}");
        assert!(imported.unwrap_err().contains("imports something"));

        let required = looks_like_a_factory("const THREE = require('three');\nexport default () => {}");
        assert!(required.unwrap_err().contains("require()"));

        let nothing = looks_like_a_factory("const group = new THREE.Group();");
        assert!(nothing.unwrap_err().contains("export default"));
    }

    #[test]
    fn the_export_bridge_report_is_read_for_the_mesh_count_that_proves_the_model_exists() {
        let said = "wrote assets/models/crate.glb (2048 bytes, 7 mesh(es))";
        assert_eq!(parse_export_line(said), Some((2048, 7)));
        assert_eq!(parse_export_line("wrote out.glb (900 bytes, 0 mesh(es))"), Some((900, 0)));
        assert_eq!(parse_export_line("nothing useful here"), None);
        assert_eq!(parse_export_line("wrote out.glb"), None);
    }

    #[test]
    fn a_reference_that_climbs_out_of_the_project_is_never_uploaded_to_codex() {
        let root = std::env::temp_dir().join("studio-assets-reference");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("reference")).unwrap();
        std::fs::write(root.join("reference").join("scout.png"), b"pretend png").unwrap();
        std::fs::write(std::env::temp_dir().join("studio-assets-secret.png"), b"not yours").unwrap();

        assert!(reference_in(&root, "reference/scout.png").is_ok());

        for climbing in [
            "../studio-assets-secret.png",
            "reference/../../studio-assets-secret.png",
        ] {
            let err = reference_in(&root, climbing).unwrap_err();
            assert!(err.contains("climb out of it"), "{climbing} was allowed: {err}");
        }

        let absolute = std::env::temp_dir().join("studio-assets-secret.png");
        assert!(reference_in(&root, &absolute.to_string_lossy()).is_err());
    }

    #[test]
    fn a_reference_that_is_not_an_image_is_refused_before_anything_reads_it() {
        let root = std::env::temp_dir().join("studio-assets-reference-kind");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("settings.json"), b"{}").unwrap();

        let err = reference_in(&root, "settings.json").unwrap_err();
        assert!(err.contains("image codex can read"));
        assert!(err.contains("png"));
    }

    #[test]
    fn a_reference_that_is_simply_absent_says_so_with_the_path_it_looked_at() {
        let root = std::env::temp_dir().join("studio-assets-reference-gone");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let err = reference_in(&root, "reference/nobody.png").unwrap_err();
        assert!(err.contains("no reference image at"));
    }

    #[test]
    fn a_refused_model_is_named_and_blamed_on_the_account_rather_than_on_the_studio() {
        let said = r#"ERROR: {"type":"error","status":400,"error":{"type":"invalid_request_error","message":"The 'gpt-5.2-codex' model is not supported when using Codex with a ChatGPT account."}}"#;
        let why = diagnose(said, "gpt-5.2-codex").unwrap();
        assert!(why.contains("gpt-5.2-codex"), "the user has to know which model: {why}");
        assert!(why.contains("restriction on this ChatGPT account"));
        assert!(why.contains("not a fault in the studio"));
        assert!(
            why.contains("The 'gpt-5.2-codex' model is not supported when using Codex with a ChatGPT account."),
            "codex's own words have to survive verbatim: {why}"
        );
        assert!(
            why.contains("only known to exist"),
            "a catalogued model is not a usable one, and this is where that bites: {why}"
        );
    }

    #[test]
    fn an_expired_sign_in_says_a_person_has_to_renew_it_rather_than_retrying_forever() {
        let said = "ERROR: Your access token could not be refreshed because your refresh token was already used. Please log out and sign in again.";
        let why = diagnose(said, DEFAULT_MODEL).unwrap();
        assert!(why.contains("only a person can renew it"));
        assert!(why.contains("codex login"));
    }

    #[test]
    fn codex_left_waiting_on_stdin_is_named_as_a_studio_bug_because_that_is_whose_bug_it_is() {
        let why = diagnose("Reading additional input from stdin...", DEFAULT_MODEL).unwrap();
        assert!(why.contains("studio bug"));
        assert!(diagnose("everything went fine", DEFAULT_MODEL).is_none());
    }

    #[test]
    fn an_unrunning_mcp_server_in_the_users_config_is_not_mistaken_for_a_failure() {
        let noise = "ERROR rmcp::transport::worker: worker quit with fatal: Transport channel closed, when UnexpectedServerResponse(\"HTTP 404: Cannot POST /mcp\")";
        assert!(
            diagnose(noise, DEFAULT_MODEL).is_none(),
            "a dead unityMCP server is the user's config, not this feature's problem"
        );
    }

    #[test]
    fn the_model_defaults_to_codexs_own_default_and_is_never_left_to_the_config_file() {
        assert_eq!(model_in(&Settings::new()), DEFAULT_MODEL);
        assert_eq!(DEFAULT_MODEL, "gpt-5.6-sol");

        let mut chosen = Settings::new();
        chosen.set(SETTING_MODEL, "gpt-5.6-luna".into());
        assert_eq!(model_in(&chosen), "gpt-5.6-luna");

        let mut blanked = Settings::new();
        blanked.set(SETTING_MODEL, "   ".into());
        assert_eq!(
            model_in(&blanked),
            DEFAULT_MODEL,
            "whitespace is unset, not a model name"
        );
    }

    #[test]
    fn the_codex_on_path_is_resolved_into_something_this_machine_can_actually_start() {
        let (program, leading) = match spawnable(PROGRAM) {
            Some(found) => found,
            None => return,
        };
        assert!(
            program.is_absolute() || program.file_name().is_some(),
            "the launcher has to be a real program, not a bare name the OS may not resolve"
        );
        if cfg!(windows) {
            let ext = program
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            assert!(
                !ext.is_empty(),
                "CreateProcess will not run an extensionless script, so {} would fail to spawn",
                program.display()
            );
            if !leading.is_empty() {
                assert_eq!(leading[0], "/c", "a .cmd has to go through the shell");
            }
        }
    }

    #[test]
    fn a_batch_launcher_is_run_through_the_shell_because_createprocess_cannot_exec_one() {
        let (program, leading) = launcher_for(Path::new(r"C:\npm\codex.cmd"));
        if cfg!(windows) {
            assert_eq!(leading, vec!["/c".to_string(), r"C:\npm\codex.cmd".to_string()]);
            assert!(program.to_string_lossy().to_lowercase().contains("cmd"));
        }

        let (direct, none) = launcher_for(Path::new(r"C:\npm\codex.exe"));
        assert_eq!(direct, Path::new(r"C:\npm\codex.exe"));
        assert!(none.is_empty(), "an exe is started directly");
    }

    #[test]
    fn a_destination_is_always_a_plain_path_under_the_project_and_never_climbs_out() {
        let root = Path::new("C:").join("games").join("scrapyard");
        assert_eq!(
            inside(&root, Path::new("src/models/scout.js")).unwrap(),
            root.join("src").join("models").join("scout.js")
        );

        for escaping in ["../elsewhere.js", "src/../../elsewhere.js", "/etc/passwd"] {
            assert!(
                inside(&root, Path::new(escaping)).is_err(),
                "{escaping} was allowed out of the project"
            );
        }
    }

    #[test]
    fn a_slug_cannot_be_crafted_to_write_outside_the_models_directory() {
        for hostile in ["../../evil", "..\\..\\evil", "/etc/passwd", "a/b/c"] {
            let slug = slugify(hostile);
            assert!(
                !slug.contains('/') && !slug.contains('\\') && !slug.contains(".."),
                "{hostile} slugified to {slug}"
            );
            let plan = plan_for("web", &slug).unwrap();
            assert!(inside(Path::new("C:").join("games").as_path(), &plan.factory).is_ok());
        }
    }

    #[test]
    fn a_generated_asset_never_replaces_hand_written_art_unless_it_was_asked_to() {
        let dir = std::env::temp_dir().join("studio-assets-noclobber");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src").join("models")).unwrap();
        std::fs::write(dir.join("index.html"), "<html></html>").unwrap();
        let precious = dir.join("src").join("models").join("scout.js");
        let by_hand = "export default function scout(THREE) { return new THREE.Group(); }";
        std::fs::write(&precious, by_hand).unwrap();

        let cap = Capability {
            installed: true,
            path: Some(PathBuf::from("codex")),
            enabled: true,
            engine: Some("web".into()),
            blockers: Vec::new(),
        };
        let asked = Request {
            kind: AssetKind::Character,
            name: "Scout".into(),
            description: "a wiry salvager".into(),
            reference: None,
            model: DEFAULT_MODEL.into(),
            overwrite: false,
        };

        let err = generate(&dir, &cap, &asked).unwrap_err();
        assert!(err.contains("already exists"), "{err}");
        assert!(err.contains("tick replace"));
        assert_eq!(
            std::fs::read_to_string(&precious).unwrap(),
            by_hand,
            "the hand-written factory must be byte-identical after a refusal"
        );
        assert!(
            !dir.join(".studio-out").exists(),
            "the refusal comes before codex is paid, so no working files appear either"
        );
    }

    #[tokio::test]
    async fn the_floor_is_told_the_feature_is_off_and_why_before_anything_is_installed() {
        let (status, body) = get(state_in("overview"), "/assets").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["enabled"], false);
        assert_eq!(body["ready"], false);
        assert_eq!(body["setting"], SETTING_ENABLED);
        assert!(!body["blockers"].as_array().unwrap().is_empty());
        assert_eq!(body["kinds"].as_array().unwrap().len(), AssetKind::ALL.len());
    }

    #[tokio::test]
    async fn the_overview_says_codex_writes_code_rather_than_pictures() {
        let (_, body) = get(state_in("honesty"), "/assets").await;
        let how = body["how"].as_str().unwrap();
        assert!(how.contains("cannot draw"));
        assert!(how.contains("procedural"));
    }

    #[tokio::test]
    async fn a_kind_the_studio_does_not_make_is_refused_by_name() {
        let (status, _) = post(
            state_in("badkind"),
            "/assets/generate",
            r#"{"project":"p","kind":"tileset","name":"grass"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn generating_while_the_feature_is_off_refuses_without_spending_anything() {
        let state = state_in("offrefuses");
        let project = state.studio_dir.join("game");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(project.join("index.html"), "<html></html>").unwrap();
        state
            .store
            .insert_project(
                studio_store::ProjectRow {
                    id: "p1".into(),
                    name: "game".into(),
                    root: project.to_string_lossy().into_owned(),
                    engine: "web".into(),
                    git: false,
                },
                &crate::now_rfc3339(),
            )
            .unwrap();

        let (status, body) = post(
            state,
            "/assets/generate",
            r#"{"project":"p1","kind":"character","name":"Scout","description":"a wiry salvager"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "a refusal is an answer, not a server error");
        assert_eq!(body["ok"], false);
        assert!(body["reason"].as_str().unwrap().contains("switched off"));
        assert!(
            !project.join("src").join("models").exists(),
            "nothing may be written into the project when the feature is off"
        );
    }

    #[test]
    fn a_generation_that_cannot_run_leaves_the_project_exactly_as_it_found_it() {
        let dir = std::env::temp_dir().join("studio-assets-degrade");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let cap = Capability {
            installed: false,
            path: None,
            enabled: true,
            engine: Some("web".into()),
            blockers: blockers(false, true, Some("web")),
        };
        let asked = Request {
            kind: AssetKind::Character,
            name: "Scout".into(),
            description: "a wiry salvager".into(),
            reference: None,
            model: DEFAULT_MODEL.into(),
            overwrite: false,
        };

        let err = generate(&dir, &cap, &asked).unwrap_err();
        assert!(err.contains("not on PATH"));
        assert!(!dir.join("src").exists(), "a refusal writes nothing");
        assert!(!dir.join(".studio-out").exists(), "and leaves no working files behind");
    }

    #[test]
    fn an_asset_with_no_description_is_refused_before_codex_is_paid_to_guess() {
        let dir = std::env::temp_dir().join("studio-assets-nodesc");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let cap = Capability {
            installed: true,
            path: Some(PathBuf::from("codex")),
            enabled: true,
            engine: Some("web".into()),
            blockers: Vec::new(),
        };
        let asked = Request {
            kind: AssetKind::Prop,
            name: "Crate".into(),
            description: "   ".into(),
            reference: None,
            model: DEFAULT_MODEL.into(),
            overwrite: false,
        };
        assert!(generate(&dir, &cap, &asked).unwrap_err().contains("needs a description"));
    }

    #[test]
    fn a_generated_asset_is_remembered_so_the_panel_can_list_it_after_a_reload() {
        let dir = std::env::temp_dir().join("studio-assets-manifest");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let record = Generated {
            kind: "character".into(),
            name: "Scout".into(),
            slug: "scout".into(),
            factory: "src/models/scout.js".into(),
            export: None,
            meshes: 12,
            bytes: 4096,
            notes: "a wiry salvager".into(),
            log: "somewhere.log".into(),
        };
        remember(&dir, &record);
        remember(&dir, &record);

        let rows = recorded(&dir);
        assert_eq!(rows.len(), 1, "regenerating an asset replaces its row instead of doubling it");
        assert_eq!(rows[0]["factory"], "src/models/scout.js");
        assert_eq!(rows[0]["meshes"], 12);
    }

    #[test]
    #[ignore]
    fn a_real_codex_generates_a_prop_that_loads_through_the_export_bridge() {
        if std::env::var("STUDIO_REAL_CODEX").is_err() {
            println!(
                "set STUDIO_REAL_CODEX=1 to spend one real Codex request on a generated prop; \
                 this test is ignored by default because it bills the user's subscription"
            );
            return;
        }

        let dir = std::env::temp_dir().join("studio-assets-real-codex");
        let _ = std::fs::remove_dir_all(&dir);
        let studio = dir.join(".studio");
        let project = dir.join("game");
        std::fs::create_dir_all(&studio).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("index.html"), "<html></html>").unwrap();

        let profiles = studio_engine::EngineProfile::builtin();
        let web = profiles.iter().find(|p| p.id == "web").unwrap();
        studio_engine::install_helpers(web, &project).unwrap();

        let mut stored = Settings::new();
        stored.set(SETTING_ENABLED, true.into());
        stored.set(
            SETTING_MODEL,
            std::env::var("STUDIO_REAL_CODEX_MODEL")
                .unwrap_or_else(|_| "gpt-5.4-mini".into())
                .into(),
        );
        stored.save(&Settings::path_in(&studio)).unwrap();

        let cap = capability(&studio, Some(&project));
        assert!(cap.ready(), "codex is not usable here: {:?}", cap.blockers);

        let asked = Request {
            kind: AssetKind::Prop,
            name: "Wooden Crate".into(),
            description: "a plain wooden shipping crate, planks with visible seams and iron \
                          corner brackets"
                .into(),
            reference: None,
            model: model_in(&stored),
            overwrite: false,
        };

        let made = match generate(&project, &cap, &asked) {
            Ok(made) => made,
            Err(why) => panic!("the real generation failed: {why}"),
        };

        println!("factory   {}", project.join(&made.factory).display());
        println!("meshes    {}", made.meshes);
        println!("glb bytes {}", made.bytes);
        println!("notes     {}", made.notes);
        println!("log       {}", made.log);

        assert_eq!(made.slug, "wooden_crate");
        assert_eq!(made.factory, "src/models/wooden_crate.js");
        assert!(made.meshes > 0, "a prop with no meshes is not a prop");
        assert!(project.join("src").join("models").join("wooden_crate.js").is_file());

        let source = std::fs::read_to_string(project.join(&made.factory)).unwrap();
        assert!(looks_like_a_factory(&source).is_ok());

        let rows = recorded(&project);
        assert_eq!(rows.len(), 1, "the generated prop has to be remembered");
        assert_eq!(rows[0]["kind"], "prop");

        let again = generate(&project, &cap, &asked).unwrap_err();
        assert!(
            again.contains("already exists"),
            "a second ask must refuse before spending again: {again}"
        );
    }

    #[test]
    fn verification_needs_the_export_bridge_and_says_so_when_it_is_absent() {
        let dir = std::env::temp_dir().join("studio-assets-nobridge");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let plan = plan_for("web", "scout").unwrap();
        let err = verify(&dir, &plan).unwrap_err();
        assert!(err.contains("model_export.mjs"));
    }
}
