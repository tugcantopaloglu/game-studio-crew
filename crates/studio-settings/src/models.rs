use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::{Settings, DEFAULT_PROVIDER};

pub const PROBE_FILE: &str = "model-probes.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Working,
    Refused,
    Unknown,
}

impl Verdict {
    pub fn as_str(&self) -> &'static str {
        match self {
            Verdict::Working => "working",
            Verdict::Refused => "refused",
            Verdict::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    CliHelp,
    Picker,
    UserConfig,
    Settings,
    Probe,
}

impl Source {
    pub fn as_str(&self) -> &'static str {
        match self {
            Source::CliHelp => "cli_help",
            Source::Picker => "picker",
            Source::UserConfig => "user_config",
            Source::Settings => "settings",
            Source::Probe => "probe",
        }
    }

    pub fn explain(&self) -> &'static str {
        match self {
            Source::CliHelp => "named in the CLI's own --help output",
            Source::Picker => "listed by the CLI's own model picker",
            Source::UserConfig => "found in this machine's config file for that CLI",
            Source::Settings => "you typed it into the studio settings",
            Source::Probe => "the studio has probed this name before",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbeRecord {
    pub provider: String,
    pub model: String,
    pub verdict: Verdict,
    #[serde(default)]
    pub detail: Option<String>,
    pub checked_at: String,
    #[serde(default)]
    pub seconds: f64,
    #[serde(default)]
    pub cost_usd: Option<f64>,
    #[serde(default)]
    pub tokens: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProbeLog {
    #[serde(default)]
    pub records: Vec<ProbeRecord>,
}

impl ProbeLog {
    pub fn path_in(studio_dir: &Path) -> PathBuf {
        studio_dir.join(PROBE_FILE)
    }

    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, path: &Path) -> crate::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn record(&mut self, entry: ProbeRecord) {
        self.records
            .retain(|r| !(r.provider == entry.provider && r.model == entry.model));
        self.records.push(entry);
    }

    pub fn find(&self, provider: &str, model: &str) -> Option<&ProbeRecord> {
        self.records
            .iter()
            .find(|r| r.provider == provider && r.model == model)
    }

    pub fn models_seen(&self, provider: &str) -> Vec<String> {
        self.records
            .iter()
            .filter(|r| r.provider == provider)
            .map(|r| r.model.clone())
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub id: String,
    pub label: Option<String>,
    pub sources: Vec<Source>,
}

pub fn shipped(provider: &str) -> Vec<(&'static str, Option<&'static str>, Source)> {
    match provider {
        "claude" => vec![
            ("fable", Some("tier one seat, twice the price of opus"), Source::CliHelp),
            ("opus", Some("tier two and three seats"), Source::CliHelp),
            ("sonnet", None, Source::CliHelp),
            ("haiku", Some("cheapest tier, used for sprint rollups"), Source::CliHelp),
        ],
        "codex" => vec![
            ("gpt-5.6-sol", Some("latest frontier agentic coding model"), Source::Picker),
            ("gpt-5.6-terra", Some("balanced, everyday work"), Source::Picker),
            ("gpt-5.6-luna", Some("fast and affordable"), Source::Picker),
            ("gpt-5.5", Some("frontier, complex coding and research"), Source::Picker),
            ("gpt-5.4", Some("strong, everyday coding"), Source::Picker),
            ("gpt-5.4-mini", Some("small, fast, cost-efficient for simpler tasks"), Source::Picker),
        ],
        "copilot" => vec![
            ("auto", Some("let Copilot pick"), Source::CliHelp),
            ("gpt-5.4", None, Source::CliHelp),
        ],
        _ => Vec::new(),
    }
}

pub fn provenance(provider: &str) -> &'static str {
    match provider {
        "claude" => "claude has no subcommand that lists models, so these are the aliases its own --help names; type any other name and the studio will check it",
        "codex" => "codex has no subcommand that lists models, so these are the ids and descriptions from its own model picker, plus anything found in this machine's ~/.codex/config.toml",
        "copilot" => "copilot has no subcommand that lists models; these two are the only names its own --help mentions, so treat the list as a starting point rather than a catalogue",
        "gemini" => "gemini's --help names no model at all and it has no subcommand that lists them, so the studio ships no list for it; type a name and probe it",
        _ => "the studio has never read this CLI, so it offers no models for it",
    }
}

pub fn codex_config_path() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(home).join(".codex").join("config.toml"))
}

pub fn from_codex_config(text: &str) -> Vec<String> {
    let Ok(parsed) = text.parse::<toml::Value>() else {
        return Vec::new();
    };
    let mut out = Vec::new();

    if let Some(pinned) = parsed.get("model").and_then(toml::Value::as_str) {
        out.push(pinned.to_string());
    }
    if let Some(shown) = parsed
        .get("tui")
        .and_then(|t| t.get("model_availability_nux"))
        .and_then(toml::Value::as_table)
    {
        out.extend(shown.keys().cloned());
    }
    if let Some(moves) = parsed
        .get("notice")
        .and_then(|n| n.get("model_migrations"))
        .and_then(toml::Value::as_table)
    {
        for (from, to) in moves {
            out.push(from.clone());
            if let Some(to) = to.as_str() {
                out.push(to.to_string());
            }
        }
    }

    out
}

pub fn candidates(
    provider: &str,
    settings: &Settings,
    log: &ProbeLog,
    codex_config: Option<&str>,
) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();

    let mut add = |id: String, label: Option<&'static str>, source: Source| {
        if id.trim().is_empty() {
            return;
        }
        match out.iter_mut().find(|c| c.id == id) {
            Some(existing) => {
                if !existing.sources.contains(&source) {
                    existing.sources.push(source);
                }
                if existing.label.is_none() {
                    existing.label = label.map(str::to_string);
                }
            }
            None => out.push(Candidate {
                id,
                label: label.map(str::to_string),
                sources: vec![source],
            }),
        }
    };

    for (id, label, source) in shipped(provider) {
        add(id.to_string(), label, source);
    }
    if provider == "codex" {
        for id in codex_config.map(from_codex_config).unwrap_or_default() {
            add(id, None, Source::UserConfig);
        }
    }
    for id in settings.named_models(provider) {
        add(id, None, Source::Settings);
    }
    for id in log.models_seen(provider) {
        add(id, None, Source::Probe);
    }

    out
}

impl Settings {
    pub fn named_models(&self, provider: &str) -> Vec<String> {
        let prefix = if provider == DEFAULT_PROVIDER {
            "models.".to_string()
        } else {
            format!("models.{provider}.")
        };

        let mut out: Vec<String> = Vec::new();
        for (key, value) in self.as_map() {
            let Some(rest) = key.strip_prefix(&prefix) else {
                continue;
            };
            let scope_only = rest.starts_with("tier") || rest.starts_with("role.");
            if !scope_only {
                continue;
            }
            if provider == DEFAULT_PROVIDER && rest.split('.').count() > 2 {
                continue;
            }
            if let Some(name) = value.as_str().map(str::trim).filter(|s| !s.is_empty()) {
                if !out.iter().any(|m| m == name) {
                    out.push(name.to_string());
                }
            }
        }
        out.sort();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const REAL_CODEX_CONFIG: &str = r#"
windows_wsl_setup_acknowledged = true
model = "gpt-5.2-codex"
model_reasoning_effort = "high"

[mcp_servers.unityMCP]
url = "http://127.0.0.1:8080/mcp"

[notice]
"hide_gpt-5.1-codex-max_migration_prompt" = true

[notice.model_migrations]
"gpt-5.1-codex-max" = "gpt-5.2-codex"

[tui.model_availability_nux]
"gpt-5.6-sol" = 3
"#;

    fn settings(pairs: &[(&str, &str)]) -> Settings {
        let mut s = Settings::new();
        for (k, v) in pairs {
            s.set(k, Value::String((*v).into()));
        }
        s
    }

    #[test]
    fn the_model_a_user_pinned_in_their_own_codex_config_becomes_a_candidate() {
        let found = from_codex_config(REAL_CODEX_CONFIG);
        assert!(
            found.iter().any(|m| m == "gpt-5.2-codex"),
            "the whole point is noticing a pinned name the account no longer accepts"
        );
    }

    #[test]
    fn a_codex_config_also_gives_up_the_models_its_own_ui_has_offered() {
        let found = from_codex_config(REAL_CODEX_CONFIG);
        assert!(found.iter().any(|m| m == "gpt-5.6-sol"));
        assert!(found.iter().any(|m| m == "gpt-5.1-codex-max"));
    }

    #[test]
    fn a_config_file_that_does_not_parse_yields_nothing_instead_of_failing() {
        assert!(from_codex_config("this is not toml [[[").is_empty());
    }

    #[test]
    fn a_pinned_model_is_offered_even_though_nobody_has_checked_it_yet() {
        let list = candidates("codex", &Settings::new(), &ProbeLog::default(), Some(REAL_CODEX_CONFIG));
        let pinned = list.iter().find(|c| c.id == "gpt-5.2-codex").unwrap();
        assert_eq!(pinned.sources, vec![Source::UserConfig]);
    }

    #[test]
    fn a_model_named_by_the_picker_and_by_the_config_records_both_places() {
        let list = candidates("codex", &Settings::new(), &ProbeLog::default(), Some(REAL_CODEX_CONFIG));
        let sol = list.iter().find(|c| c.id == "gpt-5.6-sol").unwrap();
        assert!(sol.sources.contains(&Source::Picker));
        assert!(sol.sources.contains(&Source::UserConfig));
        assert_eq!(sol.label.as_deref(), Some("latest frontier agentic coding model"));
    }

    #[test]
    fn a_model_the_user_typed_is_offered_back_even_if_no_catalogue_names_it() {
        let s = settings(&[("models.gemini.tier3", "gemini-3-pro")]);
        let list = candidates("gemini", &s, &ProbeLog::default(), None);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "gemini-3-pro");
        assert_eq!(list[0].sources, vec![Source::Settings]);
    }

    #[test]
    fn claude_model_keys_are_not_confused_with_another_providers_keys() {
        let s = settings(&[
            ("models.tier1", "fable"),
            ("models.role.artist", "haiku"),
            ("models.gemini.tier3", "gemini-3-pro"),
        ]);
        assert_eq!(s.named_models("claude"), vec!["fable", "haiku"]);
        assert_eq!(s.named_models("gemini"), vec!["gemini-3-pro"]);
    }

    #[test]
    fn gemini_ships_no_model_list_because_its_help_names_none() {
        assert!(shipped("gemini").is_empty());
        assert!(provenance("gemini").contains("no model at all"));
    }

    #[test]
    fn a_probe_result_replaces_the_earlier_one_for_the_same_model() {
        let mut log = ProbeLog::default();
        log.record(ProbeRecord {
            provider: "codex".into(),
            model: "gpt-5.4".into(),
            verdict: Verdict::Refused,
            detail: Some("nope".into()),
            checked_at: "monday".into(),
            seconds: 1.0,
            cost_usd: None,
            tokens: None,
        });
        log.record(ProbeRecord {
            provider: "codex".into(),
            model: "gpt-5.4".into(),
            verdict: Verdict::Working,
            detail: None,
            checked_at: "tuesday".into(),
            seconds: 2.0,
            cost_usd: None,
            tokens: Some(1668),
        });

        assert_eq!(log.records.len(), 1);
        assert_eq!(log.find("codex", "gpt-5.4").unwrap().verdict, Verdict::Working);
    }

    #[test]
    fn a_cached_refusal_survives_a_save_and_load_because_it_is_worth_as_much_as_a_success() {
        let dir = std::env::temp_dir().join("studio-probe-log");
        let _ = std::fs::create_dir_all(&dir);
        let path = ProbeLog::path_in(&dir);

        let mut log = ProbeLog::default();
        log.record(ProbeRecord {
            provider: "codex".into(),
            model: "gpt-5.2-codex".into(),
            verdict: Verdict::Refused,
            detail: Some("The 'gpt-5.2-codex' model is not supported when using Codex with a ChatGPT account.".into()),
            checked_at: "2026-07-25T12:06:57Z".into(),
            seconds: 4.2,
            cost_usd: None,
            tokens: None,
        });
        log.save(&path).unwrap();

        let back = ProbeLog::load(&path);
        let found = back.find("codex", "gpt-5.2-codex").unwrap();
        assert_eq!(found.verdict, Verdict::Refused);
        assert!(found.detail.as_deref().unwrap().contains("not supported"));
    }

    #[test]
    fn a_probe_log_that_is_missing_or_corrupt_reads_as_nothing_checked_yet() {
        let dir = std::env::temp_dir().join("studio-probe-log-broken");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("broken.json");
        std::fs::write(&path, "{ not json").unwrap();

        assert!(ProbeLog::load(&path).records.is_empty());
        assert!(ProbeLog::load(&dir.join("absent.json")).records.is_empty());
    }
}
