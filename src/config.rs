use std::{collections::HashMap, io, path::PathBuf};

use crate::{constants::CONFIG_FILE, errors::CommandError};

pub const VALID_KEYS: &[(&str, &str)] = &[
    (
        "ignore-scripts",
        "Skip lifecycle scripts on install (true | false)",
    ),
    ("registry", "npm registry base URL"),
    (
        "global-bin-dir",
        "Override the directory where global binary symlinks are created",
    ),
];

#[derive(Debug, Default)]
pub struct OxideConfig(HashMap<String, String>);

impl OxideConfig {
    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("oxide").join(CONFIG_FILE))
    }

    pub fn load() -> Self {
        let path = match Self::config_path() {
            Some(p) => p,
            None => return Self::default(),
        };

        let raw = match std::fs::read_to_string(&path) {
            Ok(r) => r,
            Err(_) => return Self::default(),
        };

        let map: HashMap<String, String> = serde_json::from_str(&raw).unwrap_or_default();

        OxideConfig(map)
    }

    pub fn save(&self) -> Result<(), io::Error> {
        let path = Self::config_path().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "cannot determine config directory")
        })?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let serialized = serde_json::to_string_pretty(&self.0)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        std::fs::write(path, serialized)
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    pub fn is_true(&self, key: &str) -> bool {
        self.get(key)
            .is_some_and(|v| v.eq_ignore_ascii_case("true"))
    }

    pub fn set(&mut self, key: &str, value: String) -> Result<(), CommandError> {
        if !VALID_KEYS.iter().any(|(k, _)| *k == key) {
            return Err(CommandError::UnknownConfigKey(key.to_owned()));
        }
        self.0.insert(key.to_owned(), value);
        Ok(())
    }

    pub fn delete(&mut self, key: &str) -> bool {
        self.0.remove(key).is_some()
    }

    #[allow(dead_code)]
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}
