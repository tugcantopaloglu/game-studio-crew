use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("settings file is not a json object: {0}")]
    Shape(String),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, SettingsError>;

pub const FILE_NAME: &str = "settings.json";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Settings {
    values: Map<String, Value>,
}

impl Settings {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn path_in(studio_dir: &Path) -> PathBuf {
        studio_dir.join(FILE_NAME)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::new()),
            Err(e) => return Err(e.into()),
        };
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Self> {
        if text.trim().is_empty() {
            return Ok(Self::new());
        }
        match serde_json::from_str::<Value>(text)? {
            Value::Object(values) => Ok(Self { values }),
            other => Err(SettingsError::Shape(other.to_string())),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(&self.values)?)?;
        Ok(())
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    pub fn string(&self, key: &str) -> Option<&str> {
        self.values.get(key).and_then(Value::as_str)
    }

    pub fn bool(&self, key: &str, fallback: bool) -> bool {
        self.values.get(key).and_then(Value::as_bool).unwrap_or(fallback)
    }

    pub fn set(&mut self, key: &str, value: Value) -> &mut Self {
        self.values.insert(key.to_string(), value);
        self
    }

    pub fn merge(&mut self, other: &Map<String, Value>) -> &mut Self {
        for (k, v) in other {
            self.values.insert(k.clone(), v.clone());
        }
        self
    }

    pub fn as_map(&self) -> &Map<String, Value> {
        &self.values
    }

    pub fn to_value(&self) -> Value {
        Value::Object(self.values.clone())
    }

    pub fn number(&self, key: &str, fallback: f64) -> f64 {
        self.values.get(key).and_then(Value::as_f64).unwrap_or(fallback)
    }

    fn filled(&self, key: &str) -> Option<String> {
        let text = self.values.get(key).and_then(Value::as_str)?.trim();
        if text.is_empty() {
            None
        } else {
            Some(text.to_string())
        }
    }

    pub fn scoped(&self, prefix: &str, role_id: &str, tier: u8) -> Option<String> {
        self.filled(&format!("{prefix}.role.{role_id}"))
            .or_else(|| self.filled(&format!("{prefix}.tier{tier}")))
            .or_else(|| self.filled(prefix))
    }

    pub fn role_choice(&self, role_id: &str, tier: u8) -> RoleChoice {
        let provider = self
            .scoped("provider", role_id, tier)
            .unwrap_or_else(|| DEFAULT_PROVIDER.to_string());
        let model = if provider == DEFAULT_PROVIDER {
            self.scoped("models", role_id, tier)
        } else {
            self.scoped(&format!("models.{provider}"), role_id, tier)
        };
        RoleChoice {
            model,
            effort: self.scoped("effort", role_id, tier),
            provider,
        }
    }
}

pub const DEFAULT_PROVIDER: &str = "claude";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleChoice {
    pub provider: String,
    pub model: Option<String>,
    pub effort: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_reads_as_empty_rather_than_failing() {
        let dir = std::env::temp_dir().join("studio-settings-missing");
        let _ = std::fs::create_dir_all(&dir);
        let s = Settings::load(&dir.join("nothing-here.json")).unwrap();
        assert_eq!(s, Settings::new());
    }

    #[test]
    fn values_survive_a_save_and_load_round_trip() {
        let dir = std::env::temp_dir().join("studio-settings-round-trip");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(FILE_NAME);

        let mut s = Settings::new();
        s.set("models.tier1", Value::String("fable".into()));
        s.set("lowSpec", Value::Bool(true));
        s.save(&path).unwrap();

        let back = Settings::load(&path).unwrap();
        assert_eq!(back.string("models.tier1"), Some("fable"));
        assert!(back.bool("lowSpec", false));
    }

    #[test]
    fn a_json_array_is_rejected_instead_of_silently_becoming_empty() {
        assert!(matches!(Settings::parse("[1,2]"), Err(SettingsError::Shape(_))));
    }

    fn with(pairs: &[(&str, &str)]) -> Settings {
        let mut s = Settings::new();
        for (k, v) in pairs {
            s.set(k, Value::String((*v).into()));
        }
        s
    }

    #[test]
    fn a_studio_that_has_never_been_configured_leaves_every_choice_to_the_registry() {
        let choice = Settings::new().role_choice("gameplay_engineer", 3);
        assert_eq!(choice.provider, "claude");
        assert_eq!(choice.model, None);
        assert_eq!(choice.effort, None);
    }

    #[test]
    fn a_tier_default_reaches_every_role_in_that_tier() {
        let s = with(&[("models.tier3", "haiku")]);
        assert_eq!(s.role_choice("artist", 3).model.as_deref(), Some("haiku"));
        assert_eq!(s.role_choice("producer", 2).model, None);
    }

    #[test]
    fn a_role_override_beats_the_tier_default_for_that_role_alone() {
        let s = with(&[("models.tier3", "haiku"), ("models.role.qa_engineer", "opus")]);
        assert_eq!(s.role_choice("qa_engineer", 3).model.as_deref(), Some("opus"));
        assert_eq!(s.role_choice("artist", 3).model.as_deref(), Some("haiku"));
    }

    #[test]
    fn clearing_an_override_back_to_blank_falls_through_to_the_tier() {
        let s = with(&[("models.tier2", "opus"), ("models.role.producer", "   ")]);
        assert_eq!(s.role_choice("producer", 2).model.as_deref(), Some("opus"));
    }

    #[test]
    fn a_provider_chosen_globally_applies_until_a_role_names_its_own() {
        let s = with(&[("provider", "gemini"), ("provider.role.studio_director", "claude")]);
        assert_eq!(s.role_choice("artist", 3).provider, "gemini");
        assert_eq!(s.role_choice("studio_director", 1).provider, "claude");
    }

    #[test]
    fn each_provider_carries_its_own_model_names_rather_than_sharing_one_list() {
        let s = with(&[
            ("models.tier3", "opus"),
            ("provider.role.artist", "gemini"),
            ("models.gemini.tier3", "gemini-3-pro"),
        ]);
        assert_eq!(s.role_choice("artist", 3).model.as_deref(), Some("gemini-3-pro"));
        assert_eq!(s.role_choice("qa_engineer", 3).model.as_deref(), Some("opus"));
    }

    #[test]
    fn effort_follows_the_same_role_over_tier_precedence_as_the_model() {
        let s = with(&[("effort.tier3", "low"), ("effort.role.qa_engineer", "max")]);
        assert_eq!(s.role_choice("qa_engineer", 3).effort.as_deref(), Some("max"));
        assert_eq!(s.role_choice("artist", 3).effort.as_deref(), Some("low"));
    }
}
