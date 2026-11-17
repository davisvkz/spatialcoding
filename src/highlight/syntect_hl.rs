use std::collections::HashMap;
use std::path::Path;

use syntect::easy::HighlightLines;
use syntect::highlighting::{Style, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use super::treesitter_hl::TreeSitterHighlighter;

pub struct Highlighter {
    tree_sitter: TreeSitterHighlighter,
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
    cache: HashMap<String, Vec<Vec<(Style, String)>>>,
}

impl Highlighter {
    pub fn new() -> Self {
        Self {
            tree_sitter: TreeSitterHighlighter::default(),
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
            cache: HashMap::new(),
        }
    }

    pub fn highlight_file(&mut self, path: &Path, content: &str) -> &[Vec<(Style, String)>] {
        let key = path.to_string_lossy().into_owned();

        if !self.cache.contains_key(&key)
            && let Some(lines) = self.tree_sitter.highlight(path, content)
        {
            self.cache.insert(key.clone(), lines);
        }

        if !self.cache.contains_key(&key) {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");

            let syntax = self
                .syntax_set
                .find_syntax_by_extension(ext)
                .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());

            let theme = self
                .theme_set
                .themes
                .get("base16-ocean.dark")
                .or_else(|| self.theme_set.themes.get("InspiredGitHub"))
                .expect("no suitable theme found");

            let mut highlighter = HighlightLines::new(syntax, theme);
            let mut lines = Vec::new();

            for line in LinesWithEndings::from(content) {
                let ranges = highlighter
                    .highlight_line(line, &self.syntax_set)
                    .unwrap_or_default();
                let owned: Vec<(Style, String)> =
                    ranges.into_iter().map(|(s, t)| (s, t.to_string())).collect();
                lines.push(owned);
            }

            self.cache.insert(key.clone(), lines);
        }

        &self.cache[&key]
    }

    /// Forgets the colored lines of one file (its text changed).
    pub fn invalidate(&mut self, path: &Path) {
        self.cache.remove(path.to_string_lossy().as_ref());
    }

    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    pub fn style_to_egui_color(style: &Style) -> egui::Color32 {
        egui::Color32::from_rgba_premultiplied(
            style.foreground.r,
            style.foreground.g,
            style.foreground.b,
            style.foreground.a,
        )
    }

    pub fn highlighted_lines_to_layout_job(
        lines: &[Vec<(Style, String)>],
        font_size: f32,
        max_lines: usize,
    ) -> egui::text::LayoutJob {
        let mut job = egui::text::LayoutJob::default();

        let limit = lines.len().min(max_lines);
        for line in &lines[..limit] {
            for (style, text) in line {
                job.append(
                    text,
                    0.0,
                    egui::text::TextFormat {
                        font_id: egui::FontId::monospace(font_size),
                        color: Self::style_to_egui_color(style),
                        ..Default::default()
                    },
                );
            }
        }

        job
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syntect::highlighting::{Color, FontStyle};

    fn joined(lines: &[Vec<(Style, String)>]) -> String {
        lines.iter().flatten().map(|(_, text)| text.as_str()).collect()
    }

    #[test]
    fn highlight_file_returns_one_entry_per_line_preserving_text() {
        let mut hl = Highlighter::new();
        let content = "fn main() {\n    let x = 1;\n}\n";
        let lines = hl.highlight_file(Path::new("main.rs"), content);

        assert_eq!(lines.len(), 3);
        assert_eq!(joined(lines), content);
        assert!(lines[0].len() > 1, "Rust code should get several styled spans");
    }

    #[test]
    fn unknown_extension_falls_back_to_plain_text() {
        let mut hl = Highlighter::new();
        let content = "just some text\nmore\n";
        let lines = hl.highlight_file(Path::new("notes.unknownext"), content);
        assert_eq!(lines.len(), 2);
        assert_eq!(joined(lines), content);
    }

    #[test]
    fn results_are_cached_by_path_until_cleared() {
        let mut hl = Highlighter::new();
        let path = Path::new("a.py");
        hl.highlight_file(path, "x = 1\n");

        assert_eq!(joined(hl.highlight_file(path, "y = 2\n")), "x = 1\n");
        hl.clear_cache();
        assert_eq!(joined(hl.highlight_file(path, "y = 2\n")), "y = 2\n");
    }

    #[test]
    fn style_to_egui_color_copies_foreground() {
        let style = Style {
            foreground: Color { r: 10, g: 20, b: 30, a: 255 },
            background: Color::WHITE,
            font_style: FontStyle::empty(),
        };
        assert_eq!(
            Highlighter::style_to_egui_color(&style),
            egui::Color32::from_rgb(10, 20, 30)
        );
    }

    #[test]
    fn layout_job_respects_max_lines() {
        let mut hl = Highlighter::new();
        let lines = hl.highlight_file(Path::new("a.txt"), "a\nb\nc\n").to_vec();

        let job = Highlighter::highlighted_lines_to_layout_job(&lines, 12.0, 2);
        assert_eq!(job.text, "a\nb\n");

        let all = Highlighter::highlighted_lines_to_layout_job(&lines, 12.0, 100);
        assert_eq!(all.text, "a\nb\nc\n");
    }
}
