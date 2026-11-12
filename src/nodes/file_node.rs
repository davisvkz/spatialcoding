use std::path::{Path, PathBuf};

use super::lod::LodLevel;

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

    pub fn world_rect(&self, node_width: f32, font_size: f32, lod: LodLevel) -> egui::Rect {
        let header_height = 28.0_f32;
        let line_height = font_size * 1.4;
        let visible_lines = match lod {
            LodLevel::TitleOnly => 0,
            LodLevel::Summary => 1,
            LodLevel::Full => self.line_count.min(50),
        };
        let height = header_height + visible_lines as f32 * line_height;
        egui::Rect::from_min_size(self.position, egui::vec2(node_width, height))
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
