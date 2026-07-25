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
    Catalogue,
    CliHelp,
    UserConfig,
    Settings,
    Probe,
}

impl Source {
    pub fn as_str(&self) -> &'static str {
        match self {
            Source::Catalogue => "catalogue",
            Source::CliHelp => "cli_help",
            Source::UserConfig => "user_config",
            Source::Settings => "settings",
            Source::Probe => "probe",
        }
    }

    pub fn explain(&self) -> &'static str {
        match self {
            Source::Catalogue => "the CLI listed it in its own machine-readable catalogue",
            Source::CliHelp => "named in the CLI's own --help output",
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
    pub efforts: Vec<String>,
    pub default_effort: Option<String>,
    pub context_window: Option<u64>,
}

impl Candidate {
    fn named(id: String, label: Option<String>, source: Source) -> Self {
        Self {
            id,
            label,
            sources: vec![source],
            efforts: Vec::new(),
            default_effort: None,
            context_window: None,
        }
    }
}

pub const CATALOGUE_COMMAND: [&str; 3] = ["debug", "models", "--"];

pub fn catalogue_argv(provider: &str) -> Option<Vec<String>> {
    match provider {
        "codex" => Some(vec!["debug".into(), "models".into()]),
        _ => None,
    }
}

pub fn from_codex_catalogue(json: &str) -> Vec<Candidate> {
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let Some(rows) = parsed.get("models").and_then(|m| m.as_array()) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for row in rows {
        let Some(slug) = row.get("slug").and_then(|s| s.as_str()) else {
            continue;
        };
        if row.get("visibility").and_then(|v| v.as_str()) != Some("list") {
            continue;
        }
        if row.get("supported_in_api").and_then(|v| v.as_bool()) == Some(false) {
            continue;
        }

        let efforts = row
            .get("supported_reasoning_levels")
            .and_then(|l| l.as_array())
            .map(|levels| {
                levels
                    .iter()
                    .filter_map(|l| l.get("effort").and_then(|e| e.as_str()))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();

        out.push(Candidate {
            id: slug.to_string(),
            label: row
                .get("description")
                .and_then(|d| d.as_str())
                .map(str::to_string),
            sources: vec![Source::Catalogue],
            efforts,
            default_effort: row
                .get("default_reasoning_level")
                .and_then(|d| d.as_str())
                .map(str::to_string),
            context_window: row.get("context_window").and_then(|c| c.as_u64()),
        });
    }
    out
}

pub fn shipped(provider: &str) -> Vec<(&'static str, Option<&'static str>, Source)> {
    match provider {
        "claude" => vec![
            ("fable", Some("tier one seat, twice the price of opus"), Source::CliHelp),
            ("opus", Some("tier two and three seats"), Source::CliHelp),
            ("sonnet", None, Source::CliHelp),
            ("haiku", Some("cheapest tier, used for sprint rollups"), Source::CliHelp),
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
        "claude" => "claude has no subcommand that lists models, so these are the aliases its own --help names and the only way to tell whether one runs is to ask it; type any other name and the studio will check it",
        "codex" => "codex renders its own model catalogue with `codex debug models`, which is free and local, so this list is read rather than guessed; models it marks as hidden are not offered",
        "copilot" => "copilot has no subcommand that lists models; these two are the only names its own --help mentions, so treat the list as a starting point rather than a catalogue",
        "gemini" => "gemini's --help names no model at all and it has no subcommand that lists them, so the studio ships no list for it; type a name and check it",
        _ => "the studio has never read this CLI, so it offers no models for it",
    }
}

pub fn discovery(provider: &str) -> &'static str {
    if catalogue_argv(provider).is_some() {
        "a free local catalogue call"
    } else {
        "one real billed request per model"
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
    catalogue: Option<&str>,
) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();

    let mut add = |found: Candidate| {
        if found.id.trim().is_empty() {
            return;
        }
        match out.iter_mut().find(|c| c.id == found.id) {
            Some(existing) => {
                for source in found.sources {
                    if !existing.sources.contains(&source) {
                        existing.sources.push(source);
                    }
                }
                if existing.label.is_none() {
                    existing.label = found.label;
                }
                if existing.efforts.is_empty() {
                    existing.efforts = found.efforts;
                }
                if existing.default_effort.is_none() {
                    existing.default_effort = found.default_effort;
                }
                if existing.context_window.is_none() {
                    existing.context_window = found.context_window;
                }
            }
            None => out.push(found),
        }
    };

    if provider == "codex" {
        for found in catalogue.map(from_codex_catalogue).unwrap_or_default() {
            add(found);
        }
    }
    for (id, label, source) in shipped(provider) {
        add(Candidate::named(id.to_string(), label.map(str::to_string), source));
    }
    if provider == "codex" {
        for id in codex_config.map(from_codex_config).unwrap_or_default() {
            add(Candidate::named(id, None, Source::UserConfig));
        }
    }
    for id in settings.named_models(provider) {
        add(Candidate::named(id, None, Source::Settings));
    }
    for id in log.models_seen(provider) {
        add(Candidate::named(id, None, Source::Probe));
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

    const REAL_CATALOGUE: &str = include_str!("../testdata/codex-models.json");

    #[test]
    fn a_pinned_model_is_offered_even_though_nobody_has_checked_it_yet() {
        let list = candidates("codex", &Settings::new(), &ProbeLog::default(), Some(REAL_CODEX_CONFIG), None);
        let pinned = list.iter().find(|c| c.id == "gpt-5.2-codex").unwrap();
        assert_eq!(pinned.sources, vec![Source::UserConfig]);
    }

    #[test]
    fn a_model_named_by_the_catalogue_and_by_the_config_records_both_places() {
        let list = candidates(
            "codex",
            &Settings::new(),
            &ProbeLog::default(),
            Some(REAL_CODEX_CONFIG),
            Some(REAL_CATALOGUE),
        );
        let sol = list.iter().find(|c| c.id == "gpt-5.6-sol").unwrap();
        assert!(sol.sources.contains(&Source::Catalogue));
        assert!(sol.sources.contains(&Source::UserConfig));
    }

    #[test]
    fn the_codex_catalogue_is_read_rather_than_shipped_as_a_constant() {
        assert!(
            shipped("codex").is_empty(),
            "codex names come from its own catalogue call, so hardcoding them would only rot"
        );
        assert_eq!(catalogue_argv("codex"), Some(vec!["debug".into(), "models".into()]));
        assert_eq!(catalogue_argv("claude"), None, "claude has no catalogue subcommand");
        assert_eq!(discovery("codex"), "a free local catalogue call");
        assert_eq!(discovery("claude"), "one real billed request per model");
    }

    #[test]
    fn the_real_catalogue_yields_the_six_listed_models_and_hides_the_hidden_one() {
        let found = from_codex_catalogue(REAL_CATALOGUE);
        let ids: Vec<&str> = found.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna", "gpt-5.5", "gpt-5.4", "gpt-5.4-mini"]
        );
        assert!(
            !ids.contains(&"codex-auto-review"),
            "the catalogue marks it hidden; offering it would put a model in front of the user that codex itself does not"
        );
    }

    #[test]
    fn the_catalogue_carries_the_reasoning_levels_each_model_actually_takes() {
        let found = from_codex_catalogue(REAL_CATALOGUE);
        let by = |slug: &str| found.iter().find(|c| c.id == slug).unwrap().clone();

        assert_eq!(
            by("gpt-5.6-sol").efforts,
            vec!["low", "medium", "high", "xhigh", "max", "ultra"]
        );
        assert_eq!(by("gpt-5.6-luna").efforts, vec!["low", "medium", "high", "xhigh", "max"]);
        assert_eq!(by("gpt-5.4").efforts, vec!["low", "medium", "high", "xhigh"]);
        assert_eq!(
            by("gpt-5.6-sol").default_effort.as_deref(),
            Some("low"),
            "the catalogue's own default, not the studio's guess"
        );
    }

    #[test]
    fn the_levels_differ_between_models_so_effort_cannot_be_chosen_alone() {
        let found = from_codex_catalogue(REAL_CATALOGUE);
        let sol = found.iter().find(|c| c.id == "gpt-5.6-sol").unwrap();
        let older = found.iter().find(|c| c.id == "gpt-5.4").unwrap();
        assert!(sol.efforts.contains(&"max".to_string()));
        assert!(
            !older.efforts.contains(&"max".to_string()),
            "asking gpt-5.4 for max would be rejected at request time"
        );
    }

    #[test]
    fn a_catalogue_that_does_not_parse_yields_nothing_rather_than_a_guess() {
        assert!(from_codex_catalogue("not json at all {").is_empty());
        assert!(from_codex_catalogue("{}").is_empty());
        assert!(from_codex_catalogue(r#"{"models":[]}"#).is_empty());
    }

    #[test]
    fn a_model_the_user_typed_is_offered_back_even_if_no_catalogue_names_it() {
        let s = settings(&[("models.gemini.tier3", "gemini-3-pro")]);
        let list = candidates("gemini", &s, &ProbeLog::default(), None, None);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "gemini-3-pro");
        assert_eq!(list[0].sources, vec![Source::Settings]);
    }

    #[test]
    fn claude_still_offers_sonnet_now_that_the_studio_can_express_it() {
        let list = candidates("claude", &Settings::new(), &ProbeLog::default(), None, None);
        let ids: Vec<&str> = list.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["fable", "opus", "sonnet", "haiku"]);
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
