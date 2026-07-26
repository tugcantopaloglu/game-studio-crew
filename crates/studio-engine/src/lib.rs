use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

pub const GODOT_PROFILE: &str = include_str!("../profiles/godot.toml");
pub const UNITY_PROFILE: &str = include_str!("../profiles/unity.toml");
pub const UE5_PROFILE: &str = include_str!("../profiles/ue5.toml");
pub const WEB_PROFILE: &str = include_str!("../profiles/web.toml");
pub const PYTHON_PROFILE: &str = include_str!("../profiles/python.toml");

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

    #[error("could not resolve the {engine} binary; name it in settings, set {env_var}, or put it on PATH")]
    BinaryNotFound { engine: String, env_var: String },

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
    Runtime,
}

impl VerifyScope {
    pub fn key(&self) -> &'static str {
        match self {
            VerifyScope::Compile => "compile",
            VerifyScope::TestFast => "test_fast",
            VerifyScope::TestFull => "test_full",
            VerifyScope::Import => "import",
            VerifyScope::Export => "export",
            VerifyScope::Runtime => "runtime",
        }
    }

    pub const ALL: [VerifyScope; 6] = [
        VerifyScope::Compile,
        VerifyScope::TestFast,
        VerifyScope::TestFull,
        VerifyScope::Import,
        VerifyScope::Export,
        VerifyScope::Runtime,
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
    #[serde(default)]
    pub search: Vec<String>,
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
        [GODOT_PROFILE, UNITY_PROFILE, UE5_PROFILE, WEB_PROFILE, PYTHON_PROFILE]
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

static NAMED_IN_SETTINGS: RwLock<BTreeMap<String, PathBuf>> = RwLock::new(BTreeMap::new());

pub fn name_the_binary(engine_id: &str, path: Option<&str>) {
    let Ok(mut held) = NAMED_IN_SETTINGS.write() else {
        return;
    };
    match path.map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => held.insert(engine_id.to_string(), PathBuf::from(p)),
        None => held.remove(engine_id),
    };
}

pub fn binary_named_in_settings(engine_id: &str) -> Option<PathBuf> {
    NAMED_IN_SETTINGS.read().ok()?.get(engine_id).cloned()
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BinaryFound {
    pub path: PathBuf,
    pub how: &'static str,
}

pub fn find_binary(profile: &EngineProfile) -> Option<BinaryFound> {
    if let Some(named) = binary_named_in_settings(&profile.id) {
        if let Some(path) = usable(&named.to_string_lossy()) {
            return Some(BinaryFound { path, how: "named in settings" });
        }
    }
    if let Ok(from_env) = std::env::var(&profile.tooling.binary_env) {
        if let Some(path) = usable(&from_env) {
            return Some(BinaryFound { path, how: "from the environment" });
        }
    }
    for name in &profile.tooling.binary_names {
        if let Some(path) = which(name) {
            return Some(BinaryFound { path, how: "on PATH" });
        }
    }
    for pattern in &profile.tooling.search {
        if let Some(path) = matching_paths(pattern).into_iter().next() {
            return Some(BinaryFound { path, how: "where its installer puts it" });
        }
    }
    None
}

pub fn resolve_binary(profile: &EngineProfile) -> Result<PathBuf> {
    find_binary(profile)
        .map(|found| found.path)
        .ok_or_else(|| EngineError::BinaryNotFound {
            engine: profile.id.clone(),
            env_var: profile.tooling.binary_env.clone(),
        })
}

pub fn places_searched(profile: &EngineProfile) -> Vec<String> {
    let mut places = vec![format!(
        "the engine.{} setting, then the {} environment variable",
        profile.id, profile.tooling.binary_env
    )];
    if !profile.tooling.binary_names.is_empty() {
        places.push(format!(
            "PATH, for {}",
            profile.tooling.binary_names.join(" or ")
        ));
    }
    places.extend(
        profile
            .tooling
            .search
            .iter()
            .filter_map(|p| fill_places(p))
            .filter(|p| within_reach(p))
            .map(|p| as_this_os_writes_it(&p)),
    );
    places
}

fn within_reach(pattern: &str) -> bool {
    let head = pattern.split('*').next().unwrap_or("");
    match head.rfind(['/', '\\']) {
        Some(cut) if cut > 0 => Path::new(&head[..cut]).is_dir(),
        _ => false,
    }
}

fn as_this_os_writes_it(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        path.to_string()
    }
}

fn usable(named: &str) -> Option<PathBuf> {
    let direct = PathBuf::from(named);
    if direct.is_file() {
        return Some(direct);
    }
    which(named)
}

fn which(name: &str) -> Option<PathBuf> {
    studio_core::resolve(name)
}

fn fill_places(pattern: &str) -> Option<String> {
    let home = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    let mut out = pattern.to_string();
    for (token, var) in [
        ("{localappdata}", "LOCALAPPDATA"),
        ("{programfiles}", "ProgramFiles"),
        ("{programfiles86}", "ProgramFiles(x86)"),
        ("{programdata}", "ProgramData"),
        ("{home}", home),
    ] {
        if !out.contains(token) {
            continue;
        }
        let value = std::env::var(var).ok()?;
        out = out.replace(token, value.trim_end_matches(['/', '\\']));
    }
    (!out.contains('{')).then_some(out)
}

fn matching_paths(pattern: &str) -> Vec<PathBuf> {
    let Some(filled) = fill_places(pattern) else {
        return Vec::new();
    };
    let parts: Vec<&str> = filled.split(['/', '\\']).collect();
    let Some(first_glob) = parts.iter().position(|p| p.contains('*')) else {
        let whole = PathBuf::from(&filled);
        return if whole.is_file() { vec![whole] } else { Vec::new() };
    };
    if first_glob == 0 {
        return Vec::new();
    }

    let head: usize = parts[..first_glob].iter().map(|p| p.len()).sum::<usize>() + first_glob - 1;
    let mut reached = vec![PathBuf::from(&filled[..head])];

    for (offset, segment) in parts[first_glob..].iter().enumerate() {
        let last = first_glob + offset == parts.len() - 1;
        let keep = |p: &Path| if last { p.is_file() } else { p.is_dir() };
        let mut next = Vec::new();

        for dir in &reached {
            if !segment.contains('*') {
                let step = dir.join(segment);
                if keep(&step) {
                    next.push(step);
                }
                continue;
            }
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                if !glob_matches(segment, &entry.file_name().to_string_lossy()) {
                    continue;
                }
                let step = entry.path();
                if keep(&step) {
                    next.push(step);
                }
            }
        }

        next.sort();
        next.reverse();
        reached = next;
    }

    reached
}

fn glob_matches(pattern: &str, name: &str) -> bool {
    let fold = |s: &str| {
        if cfg!(windows) {
            s.to_lowercase()
        } else {
            s.to_string()
        }
    };
    let (pattern, name) = (fold(pattern), fold(name));
    if !pattern.contains('*') {
        return pattern == name;
    }

    let mut pieces: Vec<&str> = pattern.split('*').collect();
    let tail = pieces.pop().unwrap_or("");
    let head = pieces.remove(0);
    if !name.starts_with(head) || !name.ends_with(tail) || name.len() < head.len() + tail.len() {
        return false;
    }

    let mut rest = &name[head.len()..name.len() - tail.len()];
    for piece in pieces {
        match rest.find(piece) {
            Some(at) => rest = &rest[at + piece.len()..],
            None => return false,
        }
    }
    true
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
        assert_eq!(profiles.len(), 5);
        let ids: Vec<&str> = profiles.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"godot"));
        assert!(ids.contains(&"unity"));
        assert!(ids.contains(&"ue5"));
        assert!(ids.contains(&"web"));
        assert!(ids.contains(&"python"));
    }

    #[test]
    fn every_profile_fills_all_five_commands() {
        for p in EngineProfile::builtin() {
            for scope in VerifyScope::ALL {
                if scope == VerifyScope::Runtime {
                    continue;
                }
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
    fn probed_engines_carry_a_runtime_command() {
        for p in EngineProfile::builtin() {
            let has_runtime = p.command(VerifyScope::Runtime).is_ok();
            match p.id.as_str() {
                "godot" | "web" | "python" => {
                    assert!(has_runtime, "{} must probe the running game", p.id)
                }
                _ => assert!(!has_runtime, "{} has no verified runtime probe yet", p.id),
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

    fn nowhere_engine() -> EngineProfile {
        let mut p = EngineProfile::parse(GODOT_PROFILE).unwrap();
        p.id = "studio-test-engine".into();
        p.tooling.binary_env = "STUDIO_NO_SUCH_ENGINE_VAR".into();
        p.tooling.binary_names = vec!["studio-no-such-engine-binary".into()];
        p.tooling.search = Vec::new();
        p
    }

    #[test]
    fn resolving_a_missing_binary_names_every_way_of_pointing_at_it() {
        let p = nowhere_engine();
        let err = format!("{}", resolve_binary(&p).unwrap_err());
        assert!(err.contains("STUDIO_NO_SUCH_ENGINE_VAR"), "{err}");
        assert!(err.contains("settings"), "{err}");
        assert!(err.contains("PATH"), "{err}");
    }

    #[test]
    fn a_binary_named_in_settings_is_taken_over_anything_on_path() {
        let dir = tempfile::tempdir().unwrap();
        let mine = dir.path().join("my-own-godot.exe");
        std::fs::write(&mine, "").unwrap();

        let mut p = nowhere_engine();
        p.id = "studio-test-engine-named".into();
        name_the_binary(&p.id, Some(&mine.to_string_lossy()));

        let found = find_binary(&p).unwrap();
        assert_eq!(found.path, mine);
        assert_eq!(found.how, "named in settings");

        name_the_binary(&p.id, Some("   "));
        assert!(
            resolve_binary(&p).is_err(),
            "a blank setting must fall through rather than pin the engine to nothing"
        );
    }

    #[test]
    fn a_setting_that_points_at_nothing_does_not_hide_the_engine_on_path() {
        let mut p = nowhere_engine();
        p.id = "studio-test-engine-stale".into();
        p.tooling.binary_names = vec!["cargo".into()];
        name_the_binary(&p.id, Some("C:/gone/godot.exe"));

        let found = find_binary(&p).expect("a stale setting is not a dead end");
        assert_eq!(found.how, "on PATH");
        name_the_binary(&p.id, None);
    }

    #[test]
    fn an_installer_directory_is_searched_when_nothing_is_on_path() {
        let dir = tempfile::tempdir().unwrap();
        let packages = dir.path().join("Packages").join("GodotEngine.GodotEngine_x8");
        std::fs::create_dir_all(&packages).unwrap();
        std::fs::write(packages.join("Godot_v4.2-stable_win64.exe"), "").unwrap();
        std::fs::write(packages.join("Godot_v4.7.1-stable_win64.exe"), "").unwrap();
        std::fs::write(packages.join("Godot_v4.7.1-stable_win64_console.exe"), "").unwrap();

        let pattern = format!(
            "{}/Packages/GodotEngine.GodotEngine*/Godot_v*_win64.exe",
            dir.path().display()
        );
        let hits = matching_paths(&pattern);
        assert_eq!(hits.len(), 2, "the console build is a different file name: {hits:?}");
        assert!(
            hits[0].ends_with("Godot_v4.7.1-stable_win64.exe"),
            "the newest build must come first: {hits:?}"
        );
    }

    #[test]
    fn a_pattern_whose_placeholder_is_unset_is_skipped_rather_than_searched_literally() {
        assert!(fill_places("{no_such_place}/godot.exe").is_none());
        assert!(matching_paths("{no_such_place}/godot.exe").is_empty());
    }

    #[test]
    fn globbing_matches_a_name_the_way_a_shell_would() {
        assert!(glob_matches("Godot_v*_win64.exe", "Godot_v4.7.1-stable_win64.exe"));
        assert!(!glob_matches("Godot_v*_win64.exe", "Godot_v4.7.1-stable_win64_console.exe"));
        assert!(glob_matches("Python3*", "Python313"));
        assert!(!glob_matches("Python3*", "Python"));
        assert!(glob_matches("*", "anything"));
        assert!(glob_matches("godot.exe", "godot.exe"));
        assert!(!glob_matches("godot.exe", "godot4.exe"));
        assert!(glob_matches("a*b*c", "axxbyyc"));
        assert!(!glob_matches("a*b*c", "axxc"));
    }

    #[test]
    fn every_search_pattern_a_profile_ships_is_absolute_and_placeholder_only() {
        let known = ["{localappdata}", "{programfiles}", "{programfiles86}", "{programdata}", "{home}"];
        for p in EngineProfile::builtin() {
            for pattern in &p.tooling.search {
                let rooted = pattern.starts_with('/') || known.iter().any(|k| pattern.starts_with(k));
                assert!(rooted, "{} searches a relative path: {pattern}", p.id);
                for token in pattern.split('{').skip(1) {
                    let name = format!("{{{}", token.split('}').next().unwrap_or(""));
                    assert!(
                        known.contains(&format!("{name}}}").as_str()),
                        "{} uses a placeholder nothing fills: {name}}}",
                        p.id
                    );
                }
            }
        }
    }

    #[test]
    fn the_places_searched_are_reportable_so_a_missing_engine_can_be_acted_on() {
        let godot = EngineProfile::parse(GODOT_PROFILE).unwrap();
        let places = places_searched(&godot);
        assert!(places[0].contains("engine.godot"));
        assert!(places.iter().any(|p| p.contains("PATH")));
        for place in places.iter().skip(2) {
            assert!(
                within_reach(place),
                "{place} is on another OS's disk layout; listing it as somewhere the studio \
                 looked gives no one anything to act on"
            );
        }
    }
}

pub fn scaffold(engine: &str, root: &Path, name: &str) -> Result<Vec<PathBuf>> {
    match engine {
        "godot" => scaffold_godot(root, name),
        "web" => scaffold_web(root, name),
        "python" => scaffold_python(root, name),
        _ => Ok(Vec::new()),
    }
}

pub const WEB_VENDOR_THREE: &str = include_str!("../helpers/three.module.js");
pub const WEB_SERVE_HELPER: &str = include_str!("../helpers/serve.mjs");

fn scaffold_web(root: &Path, name: &str) -> Result<Vec<PathBuf>> {
    if root.join("index.html").exists() {
        return Ok(Vec::new());
    }

    let escaped = name.replace('<', "").replace('>', "").replace('&', "");
    let index = format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{escaped}</title>\n\
         <style>html,body{{margin:0;height:100%;overflow:hidden;background:#000}}canvas{{display:block}}</style>\n\
         <script type=\"importmap\">{{\"imports\":{{\"three\":\"./vendor/three.module.js\"}}}}</script>\n\
         </head>\n<body>\n<script type=\"module\" src=\"./src/main.js\"></script>\n</body>\n</html>\n"
    );

    let main = "import * as THREE from \"three\";\n\n\
        const renderer = new THREE.WebGLRenderer({ antialias: true });\n\
        renderer.setSize(innerWidth, innerHeight);\n\
        document.body.appendChild(renderer.domElement);\n\n\
        const scene = new THREE.Scene();\n\
        const camera = new THREE.PerspectiveCamera(60, innerWidth / innerHeight, 0.1, 100);\n\
        camera.position.set(0, 1.2, 4);\n\n\
        scene.add(new THREE.HemisphereLight(0xffffff, 0x223344, 1.2));\n\
        const cube = new THREE.Mesh(\n\
          new THREE.BoxGeometry(1, 1, 1),\n\
          new THREE.MeshStandardMaterial({ color: 0x4ad991 })\n\
        );\n\
        scene.add(cube);\n\n\
        addEventListener(\"resize\", () => {\n\
          renderer.setSize(innerWidth, innerHeight);\n\
          camera.aspect = innerWidth / innerHeight;\n\
          camera.updateProjectionMatrix();\n\
        });\n\n\
        renderer.setAnimationLoop((t) => {\n\
          cube.rotation.set(t / 1400, t / 900, 0);\n\
          renderer.render(scene, camera);\n\
        });\n";

    let package = format!(
        "{{\n  \"name\": \"{}\",\n  \"private\": true,\n  \"type\": \"module\"\n}}\n",
        escaped.to_lowercase().replace(' ', "-").replace('"', "")
    );

    std::fs::create_dir_all(root.join("src"))?;
    std::fs::create_dir_all(root.join("src/models"))?;
    std::fs::create_dir_all(root.join("vendor"))?;
    std::fs::create_dir_all(root.join("tools"))?;
    std::fs::write(root.join("index.html"), index)?;
    std::fs::write(root.join("src/main.js"), main)?;
    std::fs::write(root.join("package.json"), package)?;
    std::fs::write(root.join("vendor/three.module.js"), WEB_VENDOR_THREE)?;
    std::fs::write(root.join("vendor/sfx.js"), WEB_SFX_LIB)?;
    std::fs::write(root.join("tools/serve.mjs"), WEB_SERVE_HELPER)?;
    std::fs::write(root.join("tools/studio_ci.mjs"), WEB_CI_HELPER)?;
    std::fs::write(root.join("tools/runtime_probe.mjs"), WEB_RUNTIME_PROBE)?;
    std::fs::write(root.join("tools/screenshot.mjs"), WEB_SCREENSHOT_HELPER)?;
    std::fs::write(root.join(".gitignore"), ".studio-out/\nnode_modules/\n")?;

    Ok(vec![
        root.join("index.html"),
        root.join("src/main.js"),
        root.join("package.json"),
        root.join("vendor/three.module.js"),
        root.join("tools/serve.mjs"),
    ])
}

fn scaffold_python(root: &Path, name: &str) -> Result<Vec<PathBuf>> {
    if root.join("main.py").exists() {
        return Ok(Vec::new());
    }

    let escaped = name.replace('"', "");
    let main = format!(
        "import tkinter as tk\n\n\n\
         def main() -> None:\n\
         \x20   root = tk.Tk()\n\
         \x20   root.title(\"{escaped}\")\n\
         \x20   root.geometry(\"640x400\")\n\
         \x20   tk.Label(root, text=\"{escaped}\", font=(\"Segoe UI\", 24)).pack(expand=True)\n\
         \x20   root.mainloop()\n\n\n\
         if __name__ == \"__main__\":\n\
         \x20   main()\n"
    );

    std::fs::create_dir_all(root.join("src"))?;
    std::fs::create_dir_all(root.join("tests"))?;
    std::fs::write(root.join("main.py"), main)?;
    std::fs::write(root.join(".gitignore"), "__pycache__/\n.studio-out/\n")?;

    Ok(vec![root.join("main.py")])
}

fn scaffold_godot(root: &Path, name: &str) -> Result<Vec<PathBuf>> {
    if root.join("project.godot").exists() {
        return Ok(Vec::new());
    }

    let escaped = name.replace('\\', "").replace('"', "");
    let project = format!(
        "config_version=5\n\n\
         [application]\n\n\
         config/name=\"{escaped}\"\n\
         run/main_scene=\"res://scenes/main.tscn\"\n\
         config/features=PackedStringArray(\"4.2\")\n\n\
         [rendering]\n\n\
         renderer/rendering_method=\"gl_compatibility\"\n"
    );

    let main_scene = "[gd_scene format=3]\n\n[node name=\"Main\" type=\"Node2D\"]\n";

    std::fs::create_dir_all(root.join("scenes"))?;
    std::fs::create_dir_all(root.join("scripts"))?;
    std::fs::write(root.join("project.godot"), project)?;
    std::fs::write(root.join("scenes/main.tscn"), main_scene)?;

    Ok(vec![
        root.join("project.godot"),
        root.join("scenes/main.tscn"),
    ])
}

pub const GODOT_CI_HELPER: &str = include_str!("../helpers/studio_ci.gd");
pub const WEB_CI_HELPER: &str = include_str!("../helpers/studio_ci.mjs");
pub const GLTF_EXPORTER_HELPER: &str = include_str!("../helpers/GLTFExporter.js");
pub const MODEL_EXPORT_HELPER: &str = include_str!("../helpers/model_export.mjs");
pub const WEB_RUNTIME_PROBE: &str = include_str!("../helpers/runtime_probe.mjs");
pub const PYTHON_RUNTIME_PROBE: &str = include_str!("../helpers/runtime_probe.py");
pub const WEB_SCREENSHOT_HELPER: &str = include_str!("../helpers/screenshot.mjs");
pub const WEB_SFX_LIB: &str = include_str!("../helpers/sfx.js");

fn write_if_changed(path: &Path, content: &str) -> Result<bool> {
    let needs_write = match std::fs::read_to_string(path) {
        Ok(existing) => existing != content,
        Err(_) => true,
    };
    if needs_write {
        std::fs::write(path, content)?;
    }
    Ok(needs_write)
}

pub fn install_helpers(profile: &EngineProfile, project: &Path) -> Result<Vec<PathBuf>> {
    let mut installed = Vec::new();
    if profile.id == "godot" {
        let dir = project.join("addons").join("studio");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("studio_ci.gd");
        write_if_changed(&path, GODOT_CI_HELPER)?;
        installed.push(path);
    }
    if profile.id == "web" {
        let dir = project.join("tools");
        std::fs::create_dir_all(&dir)?;
        for (name, content) in [
            ("studio_ci.mjs", WEB_CI_HELPER),
            ("serve.mjs", WEB_SERVE_HELPER),
            ("runtime_probe.mjs", WEB_RUNTIME_PROBE),
            ("screenshot.mjs", WEB_SCREENSHOT_HELPER),
        ] {
            let path = dir.join(name);
            write_if_changed(&path, content)?;
            installed.push(path);
        }
        let vendor = project.join("vendor");
        std::fs::create_dir_all(&vendor)?;
        let sfx = vendor.join("sfx.js");
        write_if_changed(&sfx, WEB_SFX_LIB)?;
        installed.push(sfx);
    }
    if profile.id == "python" {
        let dir = project.join("tools");
        std::fs::create_dir_all(&dir)?;
        let probe = dir.join("runtime_probe.py");
        write_if_changed(&probe, PYTHON_RUNTIME_PROBE)?;
        installed.push(probe);
    }
    if matches!(profile.id.as_str(), "godot" | "unity" | "ue5" | "web") {
        let vendor = project.join("tools").join("vendor");
        std::fs::create_dir_all(&vendor)?;
        let export = project.join("tools").join("model_export.mjs");
        write_if_changed(&export, MODEL_EXPORT_HELPER)?;
        installed.push(export);
        let three = vendor.join("three.module.js");
        write_if_changed(&three, WEB_VENDOR_THREE)?;
        installed.push(three);
        let exporter = vendor.join("GLTFExporter.js");
        write_if_changed(&exporter, GLTF_EXPORTER_HELPER)?;
        installed.push(exporter);
    }
    Ok(installed)
}

#[cfg(test)]
mod bootstrap_tests {
    use super::*;

    #[test]
    fn installing_the_godot_helpers_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let godot = EngineProfile::parse(GODOT_PROFILE).unwrap();

        let first = install_helpers(&godot, dir.path()).unwrap();
        assert_eq!(first.len(), 4, "ci helper plus the three model-bridge files");
        assert!(first.iter().all(|p| p.exists()));

        let before: Vec<_> = first
            .iter()
            .map(|p| std::fs::metadata(p).unwrap().modified().unwrap())
            .collect();
        let second = install_helpers(&godot, dir.path()).unwrap();
        let after: Vec<_> = second
            .iter()
            .map(|p| std::fs::metadata(p).unwrap().modified().unwrap())
            .collect();
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
    fn python_installs_exactly_the_runtime_probe() {
        let dir = tempfile::tempdir().unwrap();
        let python = EngineProfile::parse(PYTHON_PROFILE).unwrap();
        let installed = install_helpers(&python, dir.path()).unwrap();
        assert_eq!(installed.len(), 1);
        assert!(installed[0].ends_with("runtime_probe.py"));
    }

    #[test]
    fn every_gltf_capable_engine_gets_the_model_bridge() {
        for src in [GODOT_PROFILE, UNITY_PROFILE, UE5_PROFILE, WEB_PROFILE] {
            let dir = tempfile::tempdir().unwrap();
            let profile = EngineProfile::parse(src).unwrap();
            let installed = install_helpers(&profile, dir.path()).unwrap();
            assert!(
                installed.iter().any(|p| p.ends_with("model_export.mjs")),
                "{} is missing the img2threejs export bridge",
                profile.id
            );
            assert!(dir.path().join("tools/vendor/GLTFExporter.js").exists());
            assert!(dir.path().join("tools/vendor/three.module.js").exists());
        }
    }
}

#[cfg(test)]
mod scaffold_tests {
    use super::*;

    #[test]
    fn a_scaffolded_godot_project_is_detected_as_godot() {
        let dir = tempfile::tempdir().unwrap();
        let made = scaffold("godot", dir.path(), "Neon Drift").unwrap();
        assert_eq!(made.len(), 2);

        let found = detect(dir.path(), &EngineProfile::builtin());
        assert_eq!(found.len(), 1, "an empty project detects no engine at all");
        assert_eq!(found[0].id, "godot");
    }

    #[test]
    fn the_scaffold_carries_the_project_name_and_a_main_scene() {
        let dir = tempfile::tempdir().unwrap();
        scaffold("godot", dir.path(), "Neon Drift").unwrap();

        let cfg = std::fs::read_to_string(dir.path().join("project.godot")).unwrap();
        assert!(cfg.contains(r#"config/name="Neon Drift""#), "{cfg}");
        assert!(cfg.contains("res://scenes/main.tscn"), "{cfg}");
        assert!(dir.path().join("scenes/main.tscn").is_file());
        assert!(dir.path().join("scripts").is_dir());
    }

    #[test]
    fn a_quote_in_the_name_cannot_break_the_config() {
        let dir = tempfile::tempdir().unwrap();
        scaffold("godot", dir.path(), r#"ev"il\"#).unwrap();
        let cfg = std::fs::read_to_string(dir.path().join("project.godot")).unwrap();
        assert_eq!(cfg.matches('"').count() % 2, 0, "unbalanced quotes:\n{cfg}");
    }

    #[test]
    fn scaffolding_never_overwrites_an_existing_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("project.godot"), "mine").unwrap();
        assert!(scaffold("godot", dir.path(), "x").unwrap().is_empty());
        assert_eq!(std::fs::read_to_string(dir.path().join("project.godot")).unwrap(), "mine");
    }

    #[test]
    fn an_unprobed_engine_scaffolds_nothing_rather_than_guessing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(scaffold("unity", dir.path(), "x").unwrap().is_empty());
        assert!(scaffold("ue5", dir.path(), "x").unwrap().is_empty());
        assert!(detect(dir.path(), &EngineProfile::builtin()).is_empty());
    }
}
