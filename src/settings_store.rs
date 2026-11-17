//! Keeps the panel settings across restarts: the app settings, the layout
//! settings and the dependency-edge style, as one JSON file in the user's
//! config directory.
//!
//! Only the binary turns persistence on (see `main.rs`); tests and library
//! users never read or write the file. Every struct is `#[serde(default)]`,
//! so a file written by an older build still loads, with new fields at their
//! defaults.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::edges::style::EdgeStyleConfig;
use crate::layout::settings::LayoutSettings;
use crate::state::{AppSettings, AppState};

/// Everything the panels let the user tune.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PersistedSettings {
    pub app: AppSettings,
    pub layout: LayoutSettings,
    pub edges: EdgeStyleConfig,
}

impl PersistedSettings {
    pub fn capture(state: &AppState) -> Self {
        Self {
            app: state.settings.clone(),
            layout: state.layout_settings.clone(),
            edges: state.edge_style.clone(),
        }
    }

    pub fn apply(self, state: &mut AppState) {
        state.settings = self.app;
        state.layout_settings = self.layout;
        state.edge_style = self.edges;
    }
}

/// `$XDG_CONFIG_HOME/spatialcoding/settings.json`, else
/// `~/.config/spatialcoding/settings.json`. `SPATIALCODING_SETTINGS`
/// overrides the whole path.
pub fn default_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("SPATIALCODING_SETTINGS") {
        return Some(PathBuf::from(path));
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("spatialcoding").join("settings.json"))
}

/// Reads the file. A missing or unreadable file yields `None`, so the app
/// starts with defaults instead of failing.
pub fn load(path: &Path) -> Option<PersistedSettings> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&text) {
        Ok(settings) => Some(settings),
        Err(e) => {
            log::warn!("Ignoring unreadable settings {}: {e}", path.display());
            None
        }
    }
}

pub fn to_json(settings: &PersistedSettings) -> String {
    serde_json::to_string_pretty(settings).unwrap_or_default()
}

/// Writes `json` atomically: a temp file next to the target, then a rename.
pub fn save(path: &Path, json: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_through_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("settings.json");
        let mut settings = PersistedSettings::default();
        settings.layout.file_spacing = 777.0;
        settings.app.files.long_max_lines = 42;
        settings.edges.show_barrel_edges = false;
        save(&path, &to_json(&settings)).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.layout.file_spacing, 777.0);
        assert_eq!(loaded.app.files.long_max_lines, 42);
        assert!(!loaded.edges.show_barrel_edges);
    }

    #[test]
    fn older_files_with_missing_fields_still_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"layout": {"file_spacing": 123.0}, "app": {"font_size": 14.0}}"#).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.layout.file_spacing, 123.0);
        assert_eq!(loaded.layout.folder_padding, LayoutSettings::default().folder_padding);
        assert_eq!(loaded.app.font_size, 14.0);
        assert_eq!(loaded.app.node_width, AppSettings::default().node_width);
    }

    #[test]
    fn corrupt_files_fall_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "not json").unwrap();
        assert!(load(&path).is_none());
        assert!(load(&dir.path().join("missing.json")).is_none());
    }
}
