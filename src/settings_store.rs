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

/// Version written to new files. A file without `version` is version 0.
///
/// 1: "Open with collapsed folders by default" became off by default (it
///    folded most of a freshly opened project into tiny cards), so files
///    written before then lose their stored `true`, which was only ever the
///    old default.
///
/// 2: Settings defaults follow v2 (font 12, width 700, ranges 8–20 and
///    300–2100): the old defaults 11 and 650 move to the new ones and every
///    stored value is clamped into the new ranges.
///
/// 3: Edge colors and weights follow v2. Colors still at the old defaults
///    move to the new ones, Hide Short Edges (not in v2) is turned off and
///    Min Size is clamped to 0.3–1.
pub const CURRENT_VERSION: u32 = 3;

fn legacy_version() -> u32 {
    0
}

/// Everything the panels let the user tune.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PersistedSettings {
    #[serde(default = "legacy_version")]
    pub version: u32,
    pub app: AppSettings,
    pub layout: LayoutSettings,
    pub edges: EdgeStyleConfig,
}

impl Default for PersistedSettings {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            app: AppSettings::default(),
            layout: LayoutSettings::default(),
            edges: EdgeStyleConfig::default(),
        }
    }
}

impl PersistedSettings {
    pub fn capture(state: &AppState) -> Self {
        Self {
            version: CURRENT_VERSION,
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
    match serde_json::from_str::<PersistedSettings>(&text) {
        Ok(settings) => Some(settings.migrated()),
        Err(e) => {
            log::warn!("Ignoring unreadable settings {}: {e}", path.display());
            None
        }
    }
}

impl PersistedSettings {
    /// Brings a file written by an older build up to [`CURRENT_VERSION`].
    fn migrated(mut self) -> Self {
        if self.version < 1 {
            self.app.folders.open_collapsed_by_default = false;
        }
        if self.version < 2 {
            if self.app.font_size == 11.0 && self.app.node_width == 650.0 {
                self.app.font_size = 12.0;
                self.app.node_width = 700.0;
            }
            self.app.font_size = self.app.font_size.clamp(8.0, 20.0);
            self.app.node_width = self.app.node_width.clamp(300.0, 2100.0);
        }
        if self.version < 3 {
            self.edges.migrate_to_v3();
        }
        self.version = CURRENT_VERSION;
        self
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
    fn files_before_version_1_lose_the_old_collapsed_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"app": {"folders": {"open_collapsed_by_default": true, "zoom_fog": true}}}"#).unwrap();
        let loaded = load(&path).unwrap();
        assert!(!loaded.app.folders.open_collapsed_by_default, "the stored true was the old default");
        assert!(loaded.app.folders.zoom_fog, "other choices are kept");
        assert_eq!(loaded.version, CURRENT_VERSION);
    }

    #[test]
    fn files_before_version_2_move_the_old_size_defaults_and_clamp_ranges() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"version": 1, "app": {"font_size": 11.0, "node_width": 650.0}}"#).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!((loaded.app.font_size, loaded.app.node_width), (12.0, 700.0));

        std::fs::write(&path, r#"{"version": 1, "app": {"font_size": 24.0, "node_width": 2500.0}}"#).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!((loaded.app.font_size, loaded.app.node_width), (20.0, 2100.0));

        std::fs::write(&path, r#"{"version": 2, "app": {"font_size": 11.0, "node_width": 650.0}}"#).unwrap();
        assert_eq!(load(&path).unwrap().app.font_size, 11.0, "a choice made on version 2 is kept");
    }

    #[test]
    fn files_before_version_3_move_old_edge_defaults_to_the_reference_colors() {
        use crate::edges::style::{self, EdgeStyleConfig};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let old = r#"{"version": 2, "edges": {"base_color": [0.55, 0.61, 0.75], "selected_color": [0.72, 0.84, 1.0],
            "reexport_color": [0.98, 0.72, 0.30], "barrel_color": [0.37, 0.80, 0.98], "hide_short_pct": 0.5, "min_size_pct": 0.1}}"#;
        std::fs::write(&path, old).unwrap();
        let edges = load(&path).unwrap().edges;
        assert_eq!(edges.base_color, style::BASE_COLOR);
        assert_eq!(edges.selected_color, style::SELECTED_COLOR);
        assert_eq!(edges.reexport_color, style::REEXPORT_COLOR);
        assert_eq!(edges.barrel_color, style::BARREL_COLOR);
        assert_eq!((edges.hide_short_pct, edges.min_size_pct), (0.0, 0.3));

        // A color the user picked is kept.
        std::fs::write(&path, r#"{"version": 2, "edges": {"base_color": [1.0, 0.0, 0.0]}}"#).unwrap();
        assert_eq!(load(&path).unwrap().edges.base_color, [1.0, 0.0, 0.0]);
        // The old squared-px Min Dist field still loads.
        std::fs::write(&path, r#"{"version": 2, "edges": {"gradient_min_dist_sq": 123.0}}"#).unwrap();
        assert_eq!(load(&path).unwrap().edges.gradient_min_dist, 123.0);
        let _ = EdgeStyleConfig::default();
    }

    #[test]
    fn a_choice_made_since_version_1_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut settings = PersistedSettings::default();
        assert!(!settings.app.folders.open_collapsed_by_default, "off by default");
        settings.app.folders.open_collapsed_by_default = true;
        save(&path, &to_json(&settings)).unwrap();
        assert!(load(&path).unwrap().app.folders.open_collapsed_by_default);
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
