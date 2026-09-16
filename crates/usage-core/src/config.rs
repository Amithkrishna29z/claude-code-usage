//! Loads and saves [`AppConfig`] as JSON in the platform config directory:
//! `%APPDATA%\ClaudeCodeUsage` on Windows, `~/Library/Application Support/...` on
//! macOS, `~/.config/ClaudeCodeUsage` on Linux. Never fails on read: a missing or
//! corrupt file yields defaults.

use std::path::PathBuf;

use crate::models::AppConfig;

pub struct ConfigService {
    config_dir: PathBuf,
}

impl ConfigService {
    /// Uses the platform config directory. Falls back to the working directory only
    /// if the platform will not name one.
    pub fn new() -> Self {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("ClaudeCodeUsage");
        Self { config_dir }
    }

    /// Override the directory (used by tests).
    pub fn with_dir(config_dir: impl Into<PathBuf>) -> Self {
        Self {
            config_dir: config_dir.into(),
        }
    }

    pub fn config_dir(&self) -> &PathBuf {
        &self.config_dir
    }

    pub fn config_path(&self) -> PathBuf {
        self.config_dir.join("config.json")
    }

    pub fn load(&self) -> AppConfig {
        std::fs::read_to_string(self.config_path())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, config: &AppConfig) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.config_dir)?;
        let json = serde_json::to_string_pretty(config)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(self.config_path(), json)
    }
}

impl Default for ConfigService {
    fn default() -> Self {
        Self::new()
    }
}
