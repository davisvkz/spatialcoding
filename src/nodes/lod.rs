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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_zoom_thresholds() {
        assert_eq!(LodLevel::from_zoom(0.05), LodLevel::TitleOnly);
        assert_eq!(LodLevel::from_zoom(0.119), LodLevel::TitleOnly);
        assert_eq!(LodLevel::from_zoom(0.12), LodLevel::Minified);
        assert_eq!(LodLevel::from_zoom(0.499), LodLevel::Minified);
        assert_eq!(LodLevel::from_zoom(0.5), LodLevel::Full);
        assert_eq!(LodLevel::from_zoom(5.0), LodLevel::Full);
    }
}
