use std::collections::HashMap;
use std::path::Path;

use syntect::easy::HighlightLines;
use syntect::highlighting::{Style, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

pub struct Highlighter {
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
    cache: HashMap<String, Vec<Vec<(Style, String)>>>,
}

impl Highlighter {
    pub fn new() -> Self {
        Self {
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
            cache: HashMap::new(),
        }
    }

    pub fn highlight_file(&mut self, path: &Path, content: &str) -> &[Vec<(Style, String)>] {
        let key = path.to_string_lossy().into_owned();

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
