use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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
}

impl Kind {
    pub fn heading(&self) -> &'static str {
        match self {
            Kind::CodingCli => "coding CLIs (at least one is required)",
            Kind::Toolchain => "toolchain (optional)",
            Kind::Engine => "engines (optional)",
        }
    }

    pub const ALL: [Kind; 3] = [Kind::CodingCli, Kind::Toolchain, Kind::Engine];
}

#[derive(Debug, Clone, Serialize)]
pub struct Tool {
    pub name: String,
    pub label: String,
    pub kind: Kind,
    pub present: bool,
    pub version: Option<String>,
}

impl Tool {
    pub fn found(name: &str, label: &str, kind: Kind, version: Option<String>) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind,
            present: true,
            version,
        }
    }

    pub fn absent(name: &str, label: &str, kind: Kind) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind,
            present: false,
            version: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Requirements {
    pub app_version: String,
    pub os: String,
    pub ready: bool,
    pub tools: Vec<Tool>,
}

impl Requirements {
    pub fn new(tools: Vec<Tool>) -> Self {
        let ready = tools
            .iter()
            .any(|t| t.kind == Kind::CodingCli && t.present);
        Self {
            app_version: env!("CARGO_PKG_VERSION").into(),
            os: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            ready,
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
    let tools = probing.into_iter().filter_map(|p| p.join().ok()).collect();
    Requirements::new(tools)
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
    let mut child = Command::new(path)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;

    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => return None,
        }
    }

    let out = child.wait_with_output().ok()?;
    let text = if out.stdout.is_empty() { out.stderr } else { out.stdout };
    first_useful_line(&String::from_utf8_lossy(&text))
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
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
            .split(';')
            .map(|s| s.to_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };

    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for ext in &exts {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
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
        assert!(json.contains("\"kind\":\"coding_cli\""));
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
    fn a_version_line_never_carries_a_whole_banner_into_the_report() {
        let banner = "\n  Godot Engine v4.5.1.stable.official\nlots of other noise\n";
        assert_eq!(
            first_useful_line(banner).unwrap(),
            "Godot Engine v4.5.1.stable.official"
        );
        assert!(first_useful_line("   \n\n").is_none());
    }
}
