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
