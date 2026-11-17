use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VAlign {
    Top,
    Center,
    Bottom,
    Fill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortMode {
    Off,
    Desc,
    Asc,
    Alt,
    AltRev,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutSettings {
    /// Lay out again whenever files are opened or removed (kebab "Disable
    /// Auto Layout" turns it off).
    pub auto_layout: bool,
    /// 0 (Off) to 8: split columns taller than this many times their width.
    pub rank_breaking: u32,
    pub h_align: HAlign,
    pub v_align: VAlign,
    pub sort: SortMode,
    pub file_spacing: f32,
    pub folder_spacing: f32,
    pub folder_padding: f32,
}

impl Default for LayoutSettings {
    fn default() -> Self {
        Self {
            auto_layout: true,
            rank_breaking: 6,
            h_align: HAlign::Right,
            v_align: VAlign::Center,
            sort: SortMode::Desc,
            // v2 Code Canvas defaults.
            file_spacing: 500.0,
            folder_spacing: 500.0,
            folder_padding: 80.0,
        }
    }
}
