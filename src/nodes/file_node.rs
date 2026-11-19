use std::path::{Path, PathBuf};

/// World-space height of the strip above the code (v2: `HEADER_HEIGHT`).
pub const HEADER_HEIGHT: f32 = 20.0;
/// World-space vertical padding around the code area (v2: `CONTENT_PADDING`).
pub const CODE_PADDING: f32 = 20.0;
/// World-space distance from the node's left edge to the code, leaving room
/// for the line-number gutter.
pub const CODE_PAD_X: f32 = 56.0;
/// World-space distance from the node's left edge to the right edge of the
/// line numbers.
pub const GUTTER_RIGHT: f32 = 44.0;

/// World-space height of one code line: v2 uses 18 px for its 11 px code.
pub fn line_height(font_size: f32) -> f32 {
    font_size * 18.0 / 11.0
}

pub struct FileNode {
    pub id: u64,
    pub path: PathBuf,
    pub position: egui::Pos2,
    pub content: String,
    pub line_count: usize,
    pub language: String,
    /// A long file shown cut: only its first [`Self::shown_lines`] lines and a
    /// clickable "Long file" row (File nodes › Collapse long files).
    pub collapsed: bool,
    /// Lines shown while `collapsed` (the Max lines setting at open time).
    pub shown_lines: usize,
}

impl FileNode {
    pub fn new(id: u64, path: PathBuf, content: String, position: egui::Pos2) -> Self {
        let line_count = content.lines().count().max(1);
        let language = detect_language(&path);
        Self {
            id,
            path,
            position,
            content,
            line_count,
            language,
            collapsed: false,
            shown_lines: 0,
        }
    }

    /// World-space rect of the node. Independent of zoom/LOD.
    pub fn world_rect(&self, node_width: f32, font_size: f32) -> egui::Rect {
        let height = HEADER_HEIGHT
            + CODE_PADDING * 2.0
            + self.visible_lines() as f32 * line_height(font_size);
        egui::Rect::from_min_size(self.position, egui::vec2(node_width, height))
    }

    /// Source lines the node draws: every line, or the first
    /// [`Self::shown_lines`] while it is a collapsed long file.
    pub fn code_lines(&self) -> usize {
        if self.collapsed { self.shown_lines.min(self.line_count) } else { self.line_count }
    }

    /// Rows the node draws: the code lines plus, for a long file, the
    /// "Long file" row below them.
    pub fn visible_lines(&self) -> usize {
        self.code_lines() + usize::from(self.collapsed)
    }

    /// The row of the "Long file" marker, for a collapsed long file.
    pub fn marker_row(&self) -> Option<usize> {
        self.collapsed.then(|| self.code_lines())
    }

    /// The row source line `line` is drawn on: itself, or the marker row for
    /// a line cut off below the first lines. `None` past the end of the file.
    pub fn anchor_row(&self, line: usize) -> Option<usize> {
        if line < self.code_lines() {
            Some(line)
        } else if line < self.line_count {
            self.marker_row()
        } else {
            None
        }
    }

    /// World-space y of the middle of the row source line `line` (0-based) is
    /// drawn on, or `None` when the node does not draw it. Lines cut off from
    /// a long file attach to its marker row.
    pub fn line_anchor_y(&self, line: usize, font_size: f32) -> Option<f32> {
        self.anchor_row(line).map(|row| {
            self.position.y
                + HEADER_HEIGHT
                + CODE_PADDING
                + (row as f32 + 0.5) * line_height(font_size)
        })
    }

    pub fn file_name(&self) -> &str {
        self.path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
    }
}

pub fn detect_language(path: &Path) -> String {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext {
        "rs" => "Rust",
        "ts" => "TypeScript",
        "tsx" => "TypeScript",
        "js" => "JavaScript",
        "jsx" => "JavaScript",
        "py" => "Python",
        "go" => "Go",
        "rb" => "Ruby",
        "java" => "Java",
        "c" => "C",
        "cpp" | "cc" | "cxx" => "C++",
        "h" | "hpp" => "C/C++ Header",
        "cs" => "C#",
        "swift" => "Swift",
        "kt" | "kts" => "Kotlin",
        "lua" => "Lua",
        "zig" => "Zig",
        "html" | "htm" => "HTML",
        "css" => "CSS",
        "scss" | "sass" => "SCSS",
        "json" => "JSON",
        "yaml" | "yml" => "YAML",
        "toml" => "TOML",
        "md" => "Markdown",
        "sh" | "bash" => "Shell",
        "sql" => "SQL",
        _ => "Plain Text",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(path: &str, content: &str) -> FileNode {
        FileNode::new(
            7,
            PathBuf::from(path),
            content.to_string(),
            egui::pos2(5.0, 6.0),
        )
    }

    #[test]
    fn new_counts_lines_and_detects_language() {
        let n = node("src/main.rs", "a\nb\nc");
        assert_eq!(n.id, 7);
        assert_eq!(n.line_count, 3);
        assert_eq!(n.language, "Rust");
        assert!(!n.collapsed);
    }

    #[test]
    fn empty_file_still_has_one_line() {
        assert_eq!(node("empty.txt", "").line_count, 1);
    }

    #[test]
    fn visible_lines_include_the_complete_file() {
        let long = "x\n".repeat(120);
        assert_eq!(node("a.rs", &long).visible_lines(), 120);
        assert!(node("a.rs", &long).line_anchor_y(119, 12.0).is_some());
        assert_eq!(node("a.rs", "x\ny").visible_lines(), 2);
    }

    #[test]
    fn a_collapsed_long_file_shows_its_first_lines_and_a_marker_row() {
        let mut n = node("a.rs", &"x\n".repeat(50));
        n.collapsed = true;
        n.shown_lines = 10;
        assert_eq!((n.code_lines(), n.visible_lines(), n.marker_row()), (10, 11, Some(10)));
        assert_eq!(n.anchor_row(3), Some(3));
        assert_eq!(n.anchor_row(30), Some(10), "a cut-off line attaches to the marker");
        assert_eq!(n.anchor_row(50), None, "past the end of the file");
        assert_eq!(n.line_anchor_y(30, 10.0), n.line_anchor_y(10, 10.0));
        assert_eq!(n.line_anchor_y(30, 10.0).unwrap(), 6.0 + HEADER_HEIGHT + CODE_PADDING + 10.5 * line_height(10.0));
        n.collapsed = false;
        assert_eq!((n.code_lines(), n.visible_lines(), n.marker_row()), (50, 50, None));
        let mut short = node("b.rs", "a\nb");
        short.collapsed = true;
        short.shown_lines = 1000;
        assert_eq!(short.code_lines(), 2, "never more than the file has");
    }

    #[test]
    fn world_rect_depends_only_on_position_lines_and_font() {
        let rect = node("a.rs", "1\n2\n3").world_rect(700.0, 10.0);
        let expected_height = HEADER_HEIGHT + CODE_PADDING * 2.0 + 3.0 * line_height(10.0);

        assert_eq!(rect.min, egui::pos2(5.0, 6.0));
        assert_eq!(rect.width(), 700.0);
        assert!((rect.height() - expected_height).abs() < 1e-4);
    }

    #[test]
    fn line_anchor_y_is_the_middle_of_a_drawn_line() {
        let n = node("a.rs", "1\n2\n3");
        let y = n.line_anchor_y(1, 10.0).unwrap();
        assert!((y - (6.0 + HEADER_HEIGHT + CODE_PADDING + 1.5 * line_height(10.0))).abs() < 1e-4);
        assert_eq!(n.line_anchor_y(3, 10.0), None, "past the last line");
    }

    #[test]
    fn file_name_falls_back_to_unknown() {
        assert_eq!(node("src/lib.rs", "").file_name(), "lib.rs");
        assert_eq!(node("/", "").file_name(), "unknown");
    }

    #[test]
    fn detect_language_maps_extensions() {
        let cases = [
            ("a.rs", "Rust"),
            ("a.tsx", "TypeScript"),
            ("a.jsx", "JavaScript"),
            ("a.py", "Python"),
            ("a.cc", "C++"),
            ("a.hpp", "C/C++ Header"),
            ("a.kts", "Kotlin"),
            ("a.yml", "YAML"),
            ("a.bash", "Shell"),
            ("Makefile", "Plain Text"),
            ("a.RS", "Plain Text"),
        ];
        for (path, lang) in cases {
            assert_eq!(detect_language(Path::new(path)), lang, "{path}");
        }
    }
}
