use std::path::{Path, PathBuf};

/// World-space height of the file header bar.
pub const HEADER_HEIGHT: f32 = 28.0;
/// World-space vertical padding around the code area.
pub const CODE_PADDING: f32 = 4.0;
/// Maximum number of code lines a node shows.
pub const MAX_VISIBLE_LINES: usize = 50;

/// World-space height of one code line.
pub fn line_height(font_size: f32) -> f32 {
    font_size * 1.4
}

pub struct FileNode {
    pub id: u64,
    pub path: PathBuf,
    pub position: egui::Pos2,
    pub content: String,
    pub line_count: usize,
    pub language: String,
    pub collapsed: bool,
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
        }
    }

    /// World-space rect of the node. Independent of zoom/LOD.
    pub fn world_rect(&self, node_width: f32, font_size: f32) -> egui::Rect {
        let height = HEADER_HEIGHT
            + CODE_PADDING * 2.0
            + self.visible_lines() as f32 * line_height(font_size);
        egui::Rect::from_min_size(self.position, egui::vec2(node_width, height))
    }

    pub fn visible_lines(&self) -> usize {
        self.line_count.min(MAX_VISIBLE_LINES)
    }

    pub fn file_name(&self) -> &str {
        self.path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
    }
}

pub fn detect_language(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
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
        FileNode::new(7, PathBuf::from(path), content.to_string(), egui::pos2(5.0, 6.0))
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
    fn visible_lines_are_capped() {
        let long = "x\n".repeat(MAX_VISIBLE_LINES + 70);
        assert_eq!(node("a.rs", &long).visible_lines(), MAX_VISIBLE_LINES);
        assert_eq!(node("a.rs", "x\ny").visible_lines(), 2);
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
