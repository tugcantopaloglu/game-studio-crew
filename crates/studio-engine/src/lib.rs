use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const GODOT_PROFILE: &str = include_str!("../profiles/godot.toml");
pub const UNITY_PROFILE: &str = include_str!("../profiles/unity.toml");
pub const UE5_PROFILE: &str = include_str!("../profiles/ue5.toml");

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("profile parse failed: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("unsupported profile schema_version {0}; this daemon speaks 1")]
    SchemaVersion(u32),

    #[error("profile {profile} is missing command '{command}'")]
    MissingCommand { profile: String, command: String },

    #[error("command '{command}' has an unsubstituted placeholder {{{placeholder}}}")]
    UnboundPlaceholder { command: String, placeholder: String },

    #[error("could not resolve the {0} binary; set {1} or put it on PATH")]
    BinaryNotFound(String, String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, EngineError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifyScope {
    Compile,
    TestFast,
    TestFull,
    Import,
    Export,
}

impl VerifyScope {
    pub fn key(&self) -> &'static str {
        match self {
            VerifyScope::Compile => "compile",
            VerifyScope::TestFast => "test_fast",
            VerifyScope::TestFull => "test_full",
            VerifyScope::Import => "import",
            VerifyScope::Export => "export",
        }
    }

    pub const ALL: [VerifyScope; 5] = [
        VerifyScope::Compile,
        VerifyScope::TestFast,
        VerifyScope::TestFull,
        VerifyScope::Import,
        VerifyScope::Export,
    ];
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Detect {
    pub markers: Vec<String>,
    pub precedence: i32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Tooling {
    pub resolver: String,
    pub binary_env: String,
    #[serde(default)]
    pub binary_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Report {
    pub format: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Prose {
    pub profile: String,
    #[serde(default)]
    pub capabilities: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct EngineProfile {
    pub schema_version: u32,
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub min_editor_version: String,
    pub detect: Detect,
    pub tooling: Tooling,
    pub commands: BTreeMap<String, String>,
    #[serde(default)]
    pub reports: BTreeMap<String, Report>,
    pub prose: Prose,
}

impl EngineProfile {
    pub fn parse(toml_src: &str) -> Result<Self> {
        let p: EngineProfile = toml::from_str(toml_src)?;
        if p.schema_version != 1 {
            return Err(EngineError::SchemaVersion(p.schema_version));
        }
        Ok(p)
    }

    pub fn builtin() -> Vec<EngineProfile> {
        [GODOT_PROFILE, UNITY_PROFILE, UE5_PROFILE]
            .iter()
            .map(|s| EngineProfile::parse(s).expect("builtin profile must parse"))
            .collect()
    }

    pub fn command(&self, scope: VerifyScope) -> Result<&str> {
        self.commands
            .get(scope.key())
            .map(String::as_str)
            .ok_or_else(|| EngineError::MissingCommand {
                profile: self.id.clone(),
                command: scope.key().to_string(),
            })
    }

    pub fn report(&self, scope: VerifyScope) -> Option<&Report> {
        self.reports.get(scope.key())
    }

    pub fn capability_overlays(&self, task_text: &str) -> Vec<(&str, &str)> {
        let haystack = task_text.to_lowercase();
        self.prose
            .capabilities
            .iter()
            .filter(|(trigger, _)| haystack.contains(&trigger.to_lowercase()))
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DetectedEngine {
    pub id: String,
    pub root: PathBuf,
    pub precedence: i32,
}

pub fn detect(root: &Path, profiles: &[EngineProfile]) -> Vec<DetectedEngine> {
    let mut found: Vec<DetectedEngine> = profiles
        .iter()
        .filter(|p| p.detect.markers.iter().all(|m| marker_matches(root, m)))
        .map(|p| DetectedEngine {
            id: p.id.clone(),
            root: root.to_path_buf(),
            precedence: p.detect.precedence,
        })
        .collect();
    found.sort_by(|a, b| b.precedence.cmp(&a.precedence).then(a.id.cmp(&b.id)));
    found
}

fn marker_matches(root: &Path, marker: &str) -> bool {
    if let Some(ext) = marker.strip_prefix("*.") {
        return std::fs::read_dir(root)
            .map(|entries| {
                entries.flatten().any(|e| {
                    e.path()
                        .extension()
                        .map(|x| x.eq_ignore_ascii_case(ext))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
    }
    root.join(marker).exists()
}

pub fn resolve_binary(profile: &EngineProfile) -> Result<PathBuf> {
    if let Ok(p) = std::env::var(&profile.tooling.binary_env) {
        let path = PathBuf::from(&p);
        if path.exists() {
            return Ok(path);
        }
    }
    for name in &profile.tooling.binary_names {
        if let Some(found) = which(name) {
            return Ok(found);
        }
    }
    Err(EngineError::BinaryNotFound(
        profile.id.clone(),
        profile.tooling.binary_env.clone(),
    ))
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

#[derive(Debug, Clone, Default)]
pub struct Substitutions(BTreeMap<String, String>);

impl Substitutions {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    pub fn set(mut self, key: &str, value: impl Into<String>) -> Self {
        self.0.insert(key.to_string(), value.into());
        self
    }

    pub fn apply(&self, template: &str) -> Result<String> {
        let mut out = template.to_string();
        for (k, v) in &self.0 {
            out = out.replace(&format!("{{{k}}}"), v);
        }
        if let Some(start) = out.find('{') {
            if let Some(end) = out[start..].find('}') {
                let placeholder = &out[start + 1..start + end];
                return Err(EngineError::UnboundPlaceholder {
                    command: template.to_string(),
                    placeholder: placeholder.to_string(),
                });
            }
        }
        Ok(out)
    }
}

pub fn render_command(template: &str, subs: &Substitutions) -> Result<Vec<String>> {
    let mut args = Vec::new();
    for token in split_command(template) {
        args.push(subs.apply(&token)?);
    }
    Ok(args)
}

pub fn split_command(rendered: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in rendered.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_profile_parses() {
        let profiles = EngineProfile::builtin();
        assert_eq!(profiles.len(), 3);
        let ids: Vec<&str> = profiles.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"godot"));
        assert!(ids.contains(&"unity"));
        assert!(ids.contains(&"ue5"));
    }

    #[test]
    fn every_profile_fills_all_five_commands() {
        for p in EngineProfile::builtin() {
            for scope in VerifyScope::ALL {
                assert!(
                    p.command(scope).is_ok(),
                    "profile {} is missing {}",
                    p.id,
                    scope.key()
                );
            }
        }
    }

    #[test]
    fn every_declared_report_format_has_a_parser_in_studio_verify() {
        let known = ["junit", "nunit3", "ue_automation_json", "unity_buildreport"];
        for p in EngineProfile::builtin() {
            for (scope, report) in &p.reports {
                assert!(
                    known.contains(&report.format.as_str()),
                    "profile {} scope {} names format {} which has no parser",
                    p.id,
                    scope,
                    report.format
                );
            }
        }
    }

    #[test]
    fn a_future_schema_version_is_refused() {
        let src = GODOT_PROFILE.replace("schema_version = 1", "schema_version = 2");
        assert!(matches!(
            EngineProfile::parse(&src).unwrap_err(),
            EngineError::SchemaVersion(2)
        ));
    }

    #[test]
    fn engine_prose_carries_no_command_lines_or_versions() {
        for p in EngineProfile::builtin() {
            let prose = &p.prose.profile;
            assert!(!prose.contains("--headless"), "{} prose leaks a command line", p.id);
            assert!(!prose.contains("-batchmode"), "{} prose leaks a command line", p.id);
            assert!(!prose.contains("{"), "{} prose carries a placeholder", p.id);
        }
    }

    #[test]
    fn capability_overlays_trigger_on_task_text() {
        let godot = EngineProfile::parse(GODOT_PROFILE).unwrap();
        let hits = godot.capability_overlays("Add netcode for the dash ability");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, "netcode");

        assert!(godot.capability_overlays("Add a pause menu").is_empty());
    }

    #[test]
    fn capability_overlays_never_appear_in_the_frozen_prose() {
        for p in EngineProfile::builtin() {
            for text in p.prose.capabilities.values() {
                assert!(
                    !p.prose.profile.contains(text.as_str()),
                    "{} folds a capability overlay into the frozen prefix",
                    p.id
                );
            }
        }
    }

    #[test]
    fn detection_finds_godot_by_its_marker() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("project.godot"), "").unwrap();
        let found = detect(dir.path(), &EngineProfile::builtin());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "godot");
    }

    #[test]
    fn detection_finds_unreal_by_glob_marker() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("MyGame.uproject"), "{}").unwrap();
        let found = detect(dir.path(), &EngineProfile::builtin());
        assert_eq!(found[0].id, "ue5");
    }

    #[test]
    fn detection_requires_every_marker_not_just_one() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("Assets")).unwrap();
        assert!(
            detect(dir.path(), &EngineProfile::builtin()).is_empty(),
            "an Assets directory alone is not a Unity project"
        );
    }

    #[test]
    fn precedence_breaks_ties_when_two_engines_match() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("project.godot"), "").unwrap();
        std::fs::write(dir.path().join("Game.uproject"), "{}").unwrap();

        let found = detect(dir.path(), &EngineProfile::builtin());
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].id, "ue5", "ue5 has the higher precedence");
        assert_eq!(found[1].id, "godot");
    }

    #[test]
    fn an_empty_directory_detects_nothing_rather_than_guessing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(detect(dir.path(), &EngineProfile::builtin()).is_empty());
    }

    #[test]
    fn substitution_fills_every_placeholder() {
        let godot = EngineProfile::parse(GODOT_PROFILE).unwrap();
        let subs = Substitutions::new()
            .set("engine", "C:/godot.exe")
            .set("project", "C:/game")
            .set("out", "C:/out");

        let rendered = subs.apply(godot.command(VerifyScope::Compile).unwrap()).unwrap();
        assert_eq!(
            rendered,
            "C:/godot.exe --headless --path C:/game -s addons/studio/studio_ci.gd"
        );
    }

    #[test]
    fn an_unbound_placeholder_fails_loudly_rather_than_running() {
        let godot = EngineProfile::parse(GODOT_PROFILE).unwrap();
        let subs = Substitutions::new().set("engine", "godot");
        let err = subs.apply(godot.command(VerifyScope::Compile).unwrap()).unwrap_err();
        assert!(matches!(
            err,
            EngineError::UnboundPlaceholder { ref placeholder, .. } if placeholder == "project"
        ));
    }

    #[test]
    fn command_splitting_keeps_quoted_arguments_together() {
        let args = split_command(r#"editor game.uproject -ExecCmds="Automation RunTests X; Quit" -unattended"#);
        assert_eq!(args[0], "editor");
        assert_eq!(args[2], "-ExecCmds=Automation RunTests X; Quit");
        assert_eq!(args[3], "-unattended");
    }

    #[test]
    fn resolving_a_missing_binary_names_the_env_var_to_set() {
        let mut p = EngineProfile::parse(GODOT_PROFILE).unwrap();
        p.tooling.binary_env = "STUDIO_NO_SUCH_ENGINE_VAR".into();
        p.tooling.binary_names = vec!["studio-no-such-engine-binary".into()];
        let err = resolve_binary(&p).unwrap_err();
        assert!(format!("{err}").contains("STUDIO_NO_SUCH_ENGINE_VAR"));
    }
}

pub const GODOT_CI_HELPER: &str = include_str!("../helpers/studio_ci.gd");

pub fn install_helpers(profile: &EngineProfile, project: &Path) -> Result<Vec<PathBuf>> {
    let mut installed = Vec::new();
    if profile.id == "godot" {
        let dir = project.join("addons").join("studio");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("studio_ci.gd");
        let needs_write = match std::fs::read_to_string(&path) {
            Ok(existing) => existing != GODOT_CI_HELPER,
            Err(_) => true,
        };
        if needs_write {
            std::fs::write(&path, GODOT_CI_HELPER)?;
        }
        installed.push(path);
    }
    Ok(installed)
}

#[cfg(test)]
mod bootstrap_tests {
    use super::*;

    #[test]
    fn installing_the_godot_helper_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let godot = EngineProfile::parse(GODOT_PROFILE).unwrap();

        let first = install_helpers(&godot, dir.path()).unwrap();
        assert_eq!(first.len(), 1);
        assert!(first[0].exists());

        let before = std::fs::metadata(&first[0]).unwrap().modified().unwrap();
        let second = install_helpers(&godot, dir.path()).unwrap();
        let after = std::fs::metadata(&second[0]).unwrap().modified().unwrap();
        assert_eq!(before, after, "an unchanged helper must not be rewritten");
    }

    #[test]
    fn a_tampered_helper_is_restored() {
        let dir = tempfile::tempdir().unwrap();
        let godot = EngineProfile::parse(GODOT_PROFILE).unwrap();
        let path = install_helpers(&godot, dir.path()).unwrap().remove(0);

        std::fs::write(&path, "extends SceneTree\nfunc _init(): quit(0)\n").unwrap();
        install_helpers(&godot, dir.path()).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), GODOT_CI_HELPER);
    }

    #[test]
    fn the_helper_the_compile_command_invokes_is_the_one_bootstrap_installs() {
        let godot = EngineProfile::parse(GODOT_PROFILE).unwrap();
        let cmd = godot.command(VerifyScope::Compile).unwrap();
        assert!(
            cmd.contains("addons/studio/studio_ci.gd"),
            "the compile command must invoke the helper bootstrap installs"
        );
    }

    #[test]
    fn engines_without_a_code_helper_install_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let ue5 = EngineProfile::parse(UE5_PROFILE).unwrap();
        assert!(install_helpers(&ue5, dir.path()).unwrap().is_empty());
    }
}
