use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use studio_core::{Provider, RoleNeeds};

use crate::AppState;

pub const CODING_CLIS: [&str; 5] = ["claude", "codex", "gemini", "copilot", "kimi"];
pub const TOOLCHAIN: [&str; 3] = ["git", "cargo", "rustc"];
const SAFE_TO_ASK_FOR_A_VERSION: [&str; 3] = ["godot", "web", "python"];
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    CodingCli,
    Toolchain,
    Engine,
    Asset,
}

impl Kind {
    pub fn heading(&self) -> &'static str {
        match self {
            Kind::CodingCli => "coding CLIs (at least one is required)",
            Kind::Toolchain => "toolchain (optional)",
            Kind::Engine => "engines (optional)",
            Kind::Asset => "asset pipeline (optional; art the crew generates for itself)",
        }
    }

    pub const ALL: [Kind; 4] = [Kind::CodingCli, Kind::Toolchain, Kind::Engine, Kind::Asset];
}

#[derive(Debug, Clone, Serialize)]
pub struct Tool {
    pub name: String,
    pub label: String,
    pub kind: Kind,
    pub present: bool,
    pub version: Option<String>,
    pub drivable: bool,
    pub cannot_drive: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub needed_for: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install: Option<Remedy>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Remedy {
    pub says: String,
    pub run: Option<Vec<String>>,
}

impl Remedy {
    pub fn told(says: &str) -> Self {
        Self {
            says: says.into(),
            run: None,
        }
    }

    pub fn runnable(says: &str, run: &[&str]) -> Self {
        Self {
            says: says.into(),
            run: Some(run.iter().map(|s| s.to_string()).collect()),
        }
    }
}

impl Tool {
    pub fn found(name: &str, label: &str, kind: Kind, version: Option<String>) -> Self {
        let (drivable, cannot_drive) = drive_status(name, kind);
        Self {
            name: name.into(),
            label: label.into(),
            kind,
            present: true,
            version,
            drivable,
            cannot_drive,
            needed_for: None,
            install: None,
        }
    }

    pub fn absent(name: &str, label: &str, kind: Kind) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind,
            present: false,
            version: None,
            drivable: false,
            cannot_drive: None,
            needed_for: None,
            install: None,
        }
    }

    pub fn for_what(mut self, needed_for: &str) -> Self {
        self.needed_for = Some(needed_for.into());
        self
    }

    pub fn fixed_by(mut self, remedy: Remedy) -> Self {
        if !self.present {
            self.install = Some(remedy);
        }
        self
    }
}

fn drive_status(name: &str, kind: Kind) -> (bool, Option<String>) {
    if kind != Kind::CodingCli {
        return (false, None);
    }
    match Provider::from_id(name) {
        None => (
            false,
            Some(format!("the studio has no provider for {name}")),
        ),
        Some(provider) => match provider.blockers(RoleNeeds::default()).first() {
            None => (true, None),
            Some(why) => (false, Some((*why).to_string())),
        },
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Requirements {
    pub app_version: String,
    pub os: String,
    pub ready: bool,
    pub can_spawn: bool,
    pub tools: Vec<Tool>,
}

impl Requirements {
    pub fn new(tools: Vec<Tool>) -> Self {
        let installed = |t: &&Tool| t.kind == Kind::CodingCli && t.present;
        let ready = tools.iter().any(|t| installed(&t));
        let can_spawn = tools.iter().any(|t| installed(&t) && t.drivable);
        Self {
            app_version: env!("CARGO_PKG_VERSION").into(),
            os: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            ready,
            can_spawn,
            tools,
        }
    }

    pub fn of_kind(&self, kind: Kind) -> impl Iterator<Item = &Tool> {
        self.tools.iter().filter(move |t| t.kind == kind)
    }

    pub fn coding_clis_found(&self) -> Vec<&str> {
        self.of_kind(Kind::CodingCli)
            .filter(|t| t.present)
            .map(|t| t.name.as_str())
            .collect()
    }

    pub fn coding_clis_the_studio_can_drive(&self) -> Vec<&str> {
        self.of_kind(Kind::CodingCli)
            .filter(|t| t.present && t.drivable)
            .map(|t| t.name.as_str())
            .collect()
    }

    pub fn installed_but_undrivable(&self) -> Vec<(&str, &str)> {
        self.of_kind(Kind::CodingCli)
            .filter(|t| t.present && !t.drivable)
            .filter_map(|t| t.cannot_drive.as_deref().map(|why| (t.name.as_str(), why)))
            .collect()
    }
}

pub fn catalog() -> Vec<(&'static str, Kind)> {
    CODING_CLIS
        .iter()
        .map(|name| (*name, Kind::CodingCli))
        .chain(TOOLCHAIN.iter().map(|name| (*name, Kind::Toolchain)))
        .collect()
}

pub fn probe() -> Requirements {
    let mut probing = Vec::new();
    for (name, kind) in catalog() {
        probing.push(std::thread::spawn(move || probe_command(name, name, kind)));
    }
    for profile in studio_engine::EngineProfile::builtin() {
        probing.push(std::thread::spawn(move || probe_engine(&profile)));
    }
    let mut tools: Vec<Tool> = probing.into_iter().filter_map(|p| p.join().ok()).collect();
    tools.extend(asset_pipeline());
    Requirements::new(tools)
}

pub fn node_remedy() -> Remedy {
    if cfg!(windows) {
        Remedy::runnable(
            "install node 18 or newer; on Windows winget has it",
            &[
                "winget",
                "install",
                "-e",
                "--id",
                "OpenJS.NodeJS.LTS",
                "--accept-package-agreements",
                "--accept-source-agreements",
            ],
        )
    } else {
        Remedy::told("install node 18 or newer from nodejs.org or your package manager")
    }
}

pub fn python_remedy() -> Remedy {
    if cfg!(windows) {
        Remedy::runnable(
            "install python 3.10 or newer; on Windows winget has it, and note that the \
             `python3` already on PATH may be Windows' Store shortcut rather than an interpreter",
            &[
                "winget",
                "install",
                "-e",
                "--id",
                "Python.Python.3.13",
                "--accept-package-agreements",
                "--accept-source-agreements",
            ],
        )
    } else {
        Remedy::told("install python 3.10 or newer from your package manager")
    }
}

pub fn pillow_remedy(python: &str) -> Remedy {
    Remedy::runnable(
        "pillow is what turns the flat background codex draws into transparency",
        &[python, "-m", "pip", "install", "pillow"],
    )
}

pub fn codex_remedy() -> Remedy {
    Remedy::runnable(
        "install the codex CLI and sign in with `codex login` afterwards",
        &["npm", "install", "-g", "@openai/codex"],
    )
}

pub fn asset_pipeline() -> Vec<Tool> {
    let mut out = Vec::new();

    let node = probe_command("node", "node", Kind::Asset)
        .for_what("baking a model into the .glb an engine imports, and reading a mixamo clip")
        .fixed_by(node_remedy());
    out.push(node);

    let codex = which("codex");
    out.push(
        Tool {
            name: "codex".into(),
            label: "codex (for art)".into(),
            kind: Kind::Asset,
            present: codex.is_some(),
            version: None,
            drivable: codex.is_some(),
            cannot_drive: None,
            needed_for: Some("drawing sprites and textures, and writing model source".into()),
            install: None,
        }
        .fixed_by(codex_remedy()),
    );

    match crate::imagegen::python() {
        Ok(found) => out.push(
            Tool::found(
                "python",
                "python with pillow",
                Kind::Asset,
                Some(found.to_string_lossy().into_owned()),
            )
            .for_what("removing the background from a generated sprite"),
        ),
        Err(why) => {
            let remedy = match crate::imagegen::interpreter_without_pillow() {
                Some(python) => pillow_remedy(&python.to_string_lossy()),
                None => python_remedy(),
            };
            let mut absent = Tool::absent("python", "python with pillow", Kind::Asset)
                .for_what("removing the background from a generated sprite");
            absent.cannot_drive = Some(why);
            out.push(absent.fixed_by(remedy));
        }
    }

    let script = crate::imagegen::cutout_script();
    let skill = if script.is_file() {
        Tool::found("imagegen", "codex imagegen skill", Kind::Asset, None)
    } else {
        Tool::absent("imagegen", "codex imagegen skill", Kind::Asset).fixed_by(Remedy::told(
            "codex ships this and unpacks it the first time it runs; install or update codex and \
             open it once",
        ))
    }
    .for_what("the background remover codex ships beside its own image skill");
    out.push(skill);

    out
}

fn probe_command(name: &str, label: &str, kind: Kind) -> Tool {
    match which(name) {
        Some(path) => Tool::found(name, label, kind, version_of(&path)),
        None => Tool::absent(name, label, kind),
    }
}

fn probe_engine(profile: &studio_engine::EngineProfile) -> Tool {
    match studio_engine::resolve_binary(profile) {
        Ok(path) => {
            let version = if SAFE_TO_ASK_FOR_A_VERSION.contains(&profile.id.as_str()) {
                version_of(&path)
            } else {
                None
            };
            Tool::found(&profile.id, &profile.display_name, Kind::Engine, version)
        }
        Err(_) => Tool::absent(&profile.id, &profile.display_name, Kind::Engine),
    }
}

fn version_of(path: &Path) -> Option<String> {
    let mut child = studio_core::command(path)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;

    let out_pump = drain(child.stdout.take());
    let err_pump = drain(child.stderr.take());

    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => return None,
        }
    }

    let stdout = out_pump.join().unwrap_or_default();
    let text = if stdout.trim().is_empty() {
        err_pump.join().unwrap_or_default()
    } else {
        stdout
    };
    first_useful_line(&text)
}

fn drain<R: std::io::Read + Send + 'static>(
    pipe: Option<R>,
) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut raw = Vec::new();
        if let Some(mut p) = pipe {
            let _ = p.read_to_end(&mut raw);
        }
        String::from_utf8_lossy(&raw).into_owned()
    })
}

fn first_useful_line(text: &str) -> Option<String> {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())?
        .chars()
        .take(80)
        .collect::<String>();
    Some(line)
}

fn which(name: &str) -> Option<PathBuf> {
    studio_core::resolve(name)
}

async fn requirements() -> impl IntoResponse {
    let report = tokio::task::spawn_blocking(probe)
        .await
        .unwrap_or_else(|_| Requirements::new(Vec::new()));
    axum::Json(report)
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/health", get(requirements))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(tools: Vec<Tool>) -> Requirements {
        Requirements::new(tools)
    }

    #[test]
    fn one_coding_cli_is_enough_to_be_ready() {
        let report = with(vec![
            Tool::absent("claude", "claude", Kind::CodingCli),
            Tool::found("codex", "codex", Kind::CodingCli, Some("0.9".into())),
            Tool::absent("godot", "Godot 4", Kind::Engine),
        ]);
        assert!(report.ready, "an install with only codex can still code");
        assert_eq!(report.coding_clis_found(), vec!["codex"]);
    }

    #[test]
    fn installed_and_drivable_are_two_different_facts() {
        let report = with(vec![
            Tool::found("gemini", "gemini", Kind::CodingCli, Some("0.4".into())),
            Tool::found("codex", "codex", Kind::CodingCli, Some("0.9".into())),
        ]);
        assert!(report.ready, "the user chose to install on any coding CLI");
        assert!(
            !report.can_spawn,
            "neither can take a frozen charter, so no worker can start"
        );
        assert!(report.coding_clis_the_studio_can_drive().is_empty());

        let excuses = report.installed_but_undrivable();
        assert_eq!(excuses.len(), 2);
        for (name, why) in &excuses {
            assert!(
                !why.trim().is_empty(),
                "{name} is refused without saying why"
            );
        }
        assert!(
            excuses.iter().any(|(name, why)| *name == "gemini" && why.contains("system prompt")),
            "gemini's blocker must come from the provider table: {excuses:?}"
        );
    }

    #[test]
    fn a_cli_the_provider_table_has_never_heard_of_says_so() {
        let unknown = "no-such-coding-cli";
        assert!(Provider::from_id(unknown).is_none());
        let tool = Tool::found(unknown, unknown, Kind::CodingCli, Some("1.0".into()));
        assert!(!tool.drivable);
        assert!(tool.cannot_drive.unwrap().contains("no provider"));
    }

    #[test]
    fn the_drive_verdict_comes_from_the_provider_table_not_from_here() {
        for provider in Provider::ALL {
            let tool = Tool::found(provider.id(), provider.title(), Kind::CodingCli, None);
            assert_eq!(
                tool.drivable,
                provider.can_serve(RoleNeeds::default()),
                "{} disagrees with studio_core::Provider",
                provider.id()
            );
            assert_eq!(tool.drivable, tool.cannot_drive.is_none());
        }
    }

    #[test]
    fn an_absent_cli_is_not_explained_away_as_undrivable() {
        let report = with(vec![Tool::absent("gemini", "gemini", Kind::CodingCli)]);
        assert!(report.installed_but_undrivable().is_empty());
    }

    #[test]
    fn only_a_coding_cli_is_ever_judged_drivable() {
        let node = Tool::found("web", "Pure JavaScript", Kind::Engine, Some("v22".into()));
        assert!(!node.drivable);
        assert!(node.cannot_drive.is_none(), "an engine is not a worker CLI");
    }

    #[test]
    fn every_optional_tool_present_is_still_nothing_to_code_with() {
        let report = with(vec![
            Tool::absent("claude", "claude", Kind::CodingCli),
            Tool::found("git", "git", Kind::Toolchain, Some("2.47".into())),
            Tool::found("godot", "Godot 4", Kind::Engine, Some("4.5".into())),
        ]);
        assert!(!report.ready, "engines cannot write code on their own");
    }

    #[test]
    fn every_coding_cli_and_engine_the_studio_can_drive_is_in_the_catalog() {
        let catalog = catalog();
        for name in CODING_CLIS.iter().chain(TOOLCHAIN.iter()) {
            assert!(
                catalog.iter().any(|(n, _)| n == name),
                "{name} would never be probed"
            );
        }
        let engines = studio_engine::EngineProfile::builtin();
        for id in ["godot", "web", "python"] {
            assert!(
                engines.iter().any(|p| p.id == id),
                "the {id} engine is not reported"
            );
        }
    }

    #[test]
    fn probing_a_tool_this_machine_certainly_has_reports_its_version() {
        let cargo = probe_command("cargo", "cargo", Kind::Toolchain);
        assert!(cargo.present, "the test suite is running under cargo");
        assert!(
            cargo.version.unwrap_or_default().contains("cargo"),
            "a present tool reports the version it found"
        );
    }

    #[test]
    fn probing_a_tool_nobody_installs_reports_it_absent_rather_than_failing() {
        let missing = probe_command("studiod-no-such-binary", "nothing", Kind::Engine);
        assert!(!missing.present);
        assert!(missing.version.is_none());
    }

    #[test]
    fn the_report_serializes_the_readiness_the_shell_reads() {
        let json = serde_json::to_string(&with(vec![Tool::found(
            "claude",
            "claude",
            Kind::CodingCli,
            None,
        )]))
        .unwrap();
        assert!(json.contains("\"ready\":true"));
        assert!(json.contains("\"can_spawn\":true"));
        assert!(json.contains("\"kind\":\"coding_cli\""));
        assert!(json.contains("\"drivable\":true"));
    }

    #[tokio::test]
    async fn the_shell_reads_the_doctors_report_over_http() {
        use tower::ServiceExt;

        let dir = std::env::temp_dir().join("studio-health-route");
        let _ = std::fs::create_dir_all(&dir);
        let store = std::sync::Arc::new(studio_store::Store::open(dir.join("s.db")).unwrap());
        let app = crate::router(crate::AppState::new(store));

        let request = axum::http::Request::builder()
            .uri("/health")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("\"ready\""), "{text}");
        assert!(text.contains("\"claude\""), "{text}");
    }

    #[test]
    fn a_tool_that_only_exits_once_its_output_is_read_still_reports_a_version() {
        let script = "process.stdout.write('probe 9.9\\n'.repeat(20000));";
        let node = match which("node") {
            Some(n) => n,
            None => return,
        };
        let mut child = studio_core::command(&node)
            .args(["-e", script])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        let pump = drain(child.stdout.take());
        let _ = child.wait();
        let text = pump.join().unwrap();
        assert!(
            text.len() > 64 * 1024,
            "a probe that waits for exit before reading fills the pipe buffer and both sides \
             stop; godot never exits until its output is taken, so the doctor called it nameless"
        );
        assert_eq!(first_useful_line(&text).unwrap(), "probe 9.9");
        drop(child.stderr.take());
    }

    #[test]
    fn a_version_line_never_carries_a_whole_banner_into_the_report() {
        let banner = "\n  Godot Engine v4.5.1.stable.official\nlots of other noise\n";
        assert_eq!(
            first_useful_line(banner).unwrap(),
            "Godot Engine v4.5.1.stable.official"
        );
        assert!(first_useful_line("   \n\n").is_none());
    }
}
