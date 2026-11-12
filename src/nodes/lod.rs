/// Level of detail used when *rendering* a file node.
///
/// LOD only changes how a node's contents are drawn, never its world-space
/// geometry: layout, edges, folder bounds and hit-testing stay stable across
/// zoom levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LodLevel {
    /// Very far: node is a solid block with its file name.
    TitleOnly,
    /// Mid range: each code line drawn as colored bars (minimap style).
    Minified,
    /// Close: real syntax-highlighted text.
    Full,
}

impl LodLevel {
    pub fn from_zoom(zoom: f32) -> Self {
        if zoom < 0.12 {
            Self::TitleOnly
        } else if zoom < 0.5 {
            Self::Minified
        } else {
            Self::Full
        }
    }
}
