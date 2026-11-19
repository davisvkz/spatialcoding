/// Level of detail used when *rendering* a file node.
///
/// LOD only changes how a node's contents are drawn, never its world-space
/// geometry: layout, edges, folder bounds and hit-testing stay stable across
/// zoom levels.
///
/// Five levels as in Code Canvas v2 (`codecanvas/renderizacao/zoom-lod.md`).
/// `L1` is the closest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LodLevel {
    /// Zoom ≥ 0.30: real syntax-highlighted text.
    L1,
    /// Zoom ≥ 0.10: each token as a colored rectangle (minimap style).
    L2,
    /// Zoom ≥ 0.04: no text, CST tint at alpha .10.
    L3,
    /// Zoom ≥ 0.01: like L3, header label fading out.
    L4,
    /// Zoom < 0.01: blocks only, filled by selection/diff/search.
    L5,
}

/// How a node's code is drawn at a level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    Text,
    Minimap,
    None,
}

/// Zoom thresholds `ZOOM_LEVEL_THRESHOLD_*` of the reference, L1 to L4.
pub const THRESHOLDS: [f32; 4] = [0.30, 0.10, 0.04, 0.01];
/// Zoom range of the viewport (100 % is the maximum).
pub const MIN_ZOOM: f32 = 0.001;
pub const MAX_ZOOM: f32 = 1.0;
/// Zoom the `F` shortcut jumps to.
pub const FOCUS_ZOOM: f32 = 0.6;

const LEVELS: [LodLevel; 5] = [LodLevel::L1, LodLevel::L2, LodLevel::L3, LodLevel::L4, LodLevel::L5];

impl LodLevel {
    pub fn from_zoom(zoom: f32) -> Self {
        let index = THRESHOLDS.iter().position(|&t| zoom >= t).unwrap_or(4);
        LEVELS[index]
    }

    /// 1 for the closest level, 5 for the farthest.
    pub fn number(self) -> usize {
        self as usize + 1
    }

    /// `renderModeByLevel = text, minimap, none, none, none`.
    pub fn render_mode(self) -> RenderMode {
        match self {
            Self::L1 => RenderMode::Text,
            Self::L2 => RenderMode::Minimap,
            _ => RenderMode::None,
        }
    }

    /// Whether the code area is painted at all.
    pub fn draws_code(self) -> bool {
        self.render_mode() != RenderMode::None
    }

    /// Alpha of the CST base tint: none at L1, .04 at L2, .10 beyond.
    pub fn cst_base_alpha(self) -> f32 {
        match self {
            Self::L1 => 0.0,
            Self::L2 => 0.04,
            _ => 0.10,
        }
    }

    /// Opacity of the file-name label: visible down to L3, fading linearly
    /// from zoom 0.04 to 0.01 (L4), hidden at L5.
    pub fn header_alpha(zoom: f32) -> f32 {
        let (hi, lo) = (THRESHOLDS[2], THRESHOLDS[3]);
        ((zoom - lo) / (hi - lo)).clamp(0.0, 1.0)
    }

    /// L5 fills the whole node with the selection/diff/search color.
    pub fn semantic_fill(self) -> bool {
        self == Self::L5
    }

    /// Minimum nesting depth at which a CST node is dropped when it spans
    /// fewer than [`Self::cst_min_lines`] lines.
    pub fn cst_cull_depth(self) -> usize {
        [2, 2, 2, 3, 3][self as usize]
    }

    pub fn cst_min_lines(self) -> usize {
        [1, 1, 1, 100, 100][self as usize]
    }

    /// `depth >= cull_depth && lines < min_lines` hides a CST node.
    pub fn culls_cst(self, depth: usize, lines: usize) -> bool {
        depth >= self.cst_cull_depth() && lines < self.cst_min_lines()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_zoom_thresholds() {
        assert_eq!(LodLevel::from_zoom(1.0), LodLevel::L1);
        assert_eq!(LodLevel::from_zoom(0.30), LodLevel::L1);
        assert_eq!(LodLevel::from_zoom(0.299), LodLevel::L2);
        assert_eq!(LodLevel::from_zoom(0.10), LodLevel::L2);
        assert_eq!(LodLevel::from_zoom(0.099), LodLevel::L3);
        assert_eq!(LodLevel::from_zoom(0.04), LodLevel::L3);
        assert_eq!(LodLevel::from_zoom(0.039), LodLevel::L4);
        assert_eq!(LodLevel::from_zoom(0.01), LodLevel::L4);
        assert_eq!(LodLevel::from_zoom(0.0099), LodLevel::L5);
        assert_eq!(LodLevel::from_zoom(MIN_ZOOM), LodLevel::L5);
    }

    #[test]
    fn modes_and_tint_follow_the_reference_tables() {
        let modes: Vec<_> = LEVELS.iter().map(|l| l.render_mode()).collect();
        assert_eq!(
            modes,
            [RenderMode::Text, RenderMode::Minimap, RenderMode::None, RenderMode::None, RenderMode::None]
        );
        let alphas: Vec<_> = LEVELS.iter().map(|l| l.cst_base_alpha()).collect();
        assert_eq!(alphas, [0.0, 0.04, 0.10, 0.10, 0.10]);
        assert!(LodLevel::L5.semantic_fill() && !LodLevel::L4.semantic_fill());
        assert_eq!(LodLevel::L3.number(), 3);
    }

    #[test]
    fn header_label_fades_between_004_and_001() {
        assert_eq!(LodLevel::header_alpha(0.5), 1.0);
        assert_eq!(LodLevel::header_alpha(0.04), 1.0);
        assert!((LodLevel::header_alpha(0.025) - 0.5).abs() < 1e-6);
        assert_eq!(LodLevel::header_alpha(0.01), 0.0);
        assert_eq!(LodLevel::header_alpha(0.002), 0.0);
    }

    #[test]
    fn cst_culling_depends_on_level_depth_and_lines() {
        // L1..L3: depth >= 2 and fewer than 1 line (never: lines >= 1).
        assert!(!LodLevel::L1.culls_cst(5, 1));
        assert!(!LodLevel::L3.culls_cst(2, 1));
        // L4/L5: depth >= 3 and fewer than 100 lines.
        assert!(LodLevel::L4.culls_cst(3, 99));
        assert!(!LodLevel::L4.culls_cst(2, 5));
        assert!(!LodLevel::L5.culls_cst(3, 100));
    }
}
