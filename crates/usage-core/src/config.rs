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

    /// Reads the config, falling back to defaults for a missing or unparseable file.
    ///
    /// A UTF-8 BOM is stripped first. Windows editors add one routinely — Notepad and
    /// PowerShell's `Set-Content -Encoding utf8` both do — and `serde_json` rejects
    /// it, which would silently discard every hand-edited setting.
    pub fn load(&self) -> AppConfig {
        match self.read() {
            Ok(Some(config)) => config,
            Ok(None) => AppConfig::default(),
            Err(err) => {
                eprintln!(
                    "usage: {} is not valid JSON ({err}); using defaults and leaving the \
                     file alone so your settings are not overwritten.",
                    self.config_path().display()
                );
                AppConfig::default()
            }
        }
    }

    /// `Ok(None)` means "no file yet", which is normal on a first run. `Err` means the
    /// file exists but could not be parsed, and the caller must not overwrite it.
    fn read(&self) -> Result<Option<AppConfig>, serde_json::Error> {
        let Ok(text) = std::fs::read_to_string(self.config_path()) else {
            return Ok(None);
        };
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        serde_json::from_str(text).map(Some)
    }

    /// Whether the on-disk config can be read back. The app uses this to avoid
    /// clobbering a file that is mid-edit or malformed.
    pub fn is_readable(&self) -> bool {
        self.read().is_ok()
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
