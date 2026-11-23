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
    /// 0 (Off) to 8: a folder's columns are split (the tallest first, where no
    /// edge crosses the cut) until the tallest is at most this many times as
    /// tall as the folder's columns are wide, all together.
    pub rank_breaking: u32,
    /// Orders each column after the previous one by where its neighbours
    /// are, to cut crossings, instead of by the score alone. Off by default.
    pub barycenter: bool,
    pub h_align: HAlign,
    pub v_align: VAlign,
    pub sort: SortMode,
    pub file_spacing: f32,
    pub folder_spacing: f32,
    pub folder_padding: f32,
    /// Geometry of a file node, set from the canvas settings
    /// ([`AppState::layout_input`]); never persisted with the layout.
    #[serde(skip)]
    pub node_width: f32,
    #[serde(skip)]
    pub font_size: f32,
}

impl Default for LayoutSettings {
    fn default() -> Self {
        Self {
            auto_layout: true,
            rank_breaking: 6,
            barycenter: false,
            h_align: HAlign::Right,
            v_align: VAlign::Center,
            sort: SortMode::Desc,
            // v2 Code Canvas defaults.
            file_spacing: 500.0,
            folder_spacing: 500.0,
            folder_padding: 80.0,
            node_width: 700.0,
            font_size: 12.0,
        }
    }
}
