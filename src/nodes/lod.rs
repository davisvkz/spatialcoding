#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LodLevel {
    TitleOnly,
    Summary,
    Full,
}

impl LodLevel {
    pub fn from_zoom(zoom: f32) -> Self {
        if zoom < 0.3 {
            Self::TitleOnly
        } else if zoom < 0.7 {
            Self::Summary
        } else {
            Self::Full
        }
    }
}
