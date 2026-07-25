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
}
