use std::{collections::HashMap, io, path::PathBuf};

use crate::{constants::CONFIG_FILE, errors::CommandError};

pub static VALID_KEYS: &[(&str, &str, Option<&[&str]>)] = &[
    ("ignore-scripts", "Skip lifecycle scripts on install (true | false)", Some(&["true", "false"])),
    ("registry", "npm registry base URL", None),
    ("global-bin-dir", "Override the directory where global binary symlinks are created", None),
    ("install-progress", "Control install output style (logging | bar | both)", Some(&["logging", "bar", "both"])),
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
        let entry = VALID_KEYS.iter().find(|(k, _, _)| *k == key);
        let (_, _, allowed_values) = entry.ok_or_else(|| CommandError::UnknownConfigKey(key.to_owned()))?;

        match key {
            "registry" => {
                if !value.starts_with("https://") {
                    return Err(CommandError::InsecureUrl(value));
                }
            }
            "global-bin-dir" => {
                if value.trim().is_empty() {
                    return Err(CommandError::InvalidConfigValue {
                        key: key.to_owned(),
                        value,
                        allowed: "non-empty path".to_owned(),
                    });
                }
            }
            _ => {}
        }

        if let Some(allowed) = allowed_values {
            if !allowed.contains(&value.as_str()) {
                return Err(CommandError::InvalidConfigValue {
                    key: key.to_owned(),
                    allowed: allowed.join(", "),
                    value,
                });
            }
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
