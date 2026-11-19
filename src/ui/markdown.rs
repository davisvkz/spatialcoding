//! A small Markdown renderer for Agent notes and the Docs window: headings,
//! bullet and numbered lists, block quotes, fenced code and paragraphs, with
//! `**bold**`, `` `code` `` and `[text](url)` inline. Links are returned to
//! the caller (a `cc://` link navigates the canvas).

/// One block of a Markdown text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Heading(u8, String),
    Bullet(String),
    Numbered(usize, String),
    Quote(String),
    Code(String),
    Paragraph(String),
}

/// An inline span of a paragraph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Span {
    Text(String),
    Bold(String),
    Code(String),
    Link { text: String, url: String },
}

/// Splits `markdown` into blocks.
pub fn blocks(markdown: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut lines = markdown.lines().peekable();
    let mut paragraph: Vec<String> = Vec::new();
    let flush = |paragraph: &mut Vec<String>, out: &mut Vec<Block>| {
        if !paragraph.is_empty() {
            out.push(Block::Paragraph(paragraph.join(" ")));
            paragraph.clear();
        }
    };
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            flush(&mut paragraph, &mut out);
            let mut code = Vec::new();
            for next in lines.by_ref() {
                if next.trim_start().starts_with("```") {
                    break;
                }
                code.push(next);
            }
            out.push(Block::Code(code.join("\n")));
        } else if let Some((level, text)) = heading(trimmed) {
            flush(&mut paragraph, &mut out);
            out.push(Block::Heading(level, text.to_string()));
        } else if let Some(text) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            flush(&mut paragraph, &mut out);
            out.push(Block::Bullet(text.to_string()));
        } else if let Some((n, text)) = numbered(trimmed) {
            flush(&mut paragraph, &mut out);
            out.push(Block::Numbered(n, text.to_string()));
        } else if let Some(text) = trimmed.strip_prefix("> ") {
            flush(&mut paragraph, &mut out);
            out.push(Block::Quote(text.to_string()));
        } else if trimmed.is_empty() {
            flush(&mut paragraph, &mut out);
        } else {
            paragraph.push(trimmed.to_string());
        }
    }
    flush(&mut paragraph, &mut out);
    out
}

fn heading(line: &str) -> Option<(u8, &str)> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    (1..=6).contains(&hashes).then_some(())?;
    line[hashes..].strip_prefix(' ').map(|text| (hashes as u8, text.trim()))
}

fn numbered(line: &str) -> Option<(usize, &str)> {
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    (digits > 0).then_some(())?;
    let rest = line[digits..].strip_prefix(". ")?;
    Some((line[..digits].parse().ok()?, rest))
}

/// Splits a paragraph into spans.
pub fn spans(text: &str) -> Vec<Span> {
    let mut out = Vec::new();
    let mut plain = String::new();
    let mut rest = text;
    let flush = |plain: &mut String, out: &mut Vec<Span>| {
        if !plain.is_empty() {
            out.push(Span::Text(std::mem::take(plain)));
        }
    };
    while let Some(c) = rest.chars().next() {
        if let Some(after) = rest.strip_prefix("**")
            && let Some(end) = after.find("**")
        {
            flush(&mut plain, &mut out);
            out.push(Span::Bold(after[..end].to_string()));
            rest = &after[end + 2..];
        } else if let Some(after) = rest.strip_prefix('`')
            && let Some(end) = after.find('`')
        {
            flush(&mut plain, &mut out);
            out.push(Span::Code(after[..end].to_string()));
            rest = &after[end + 1..];
        } else if rest.starts_with('[')
            && let Some(close) = rest.find("](")
            && let Some(end) = rest[close..].find(')')
        {
            flush(&mut plain, &mut out);
            let text = rest[1..close].trim_matches('`').to_string();
            out.push(Span::Link { text, url: rest[close + 2..close + end].to_string() });
            rest = &rest[close + end + 1..];
        } else {
            plain.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    flush(&mut plain, &mut out);
    out
}

/// Draws one paragraph's spans on a wrapping line; returns the clicked link.
fn draw_spans(ui: &mut egui::Ui, text: &str, preview: &dyn Fn(&str) -> String) -> Option<String> {
    let mut clicked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for span in spans(text) {
            match span {
                Span::Text(t) => {
                    ui.label(t);
                }
                Span::Bold(t) => {
                    ui.label(egui::RichText::new(t).strong());
                }
                Span::Code(t) => {
                    ui.label(egui::RichText::new(t).monospace().background_color(ui.visuals().code_bg_color));
                }
                Span::Link { text, url } => {
                    // Hover only previews the target; a click follows it.
                    let response = ui.link(text).on_hover_text(preview(&url));
                    if response.clicked() {
                        clicked = Some(url);
                    }
                }
            }
        }
    });
    clicked
}

/// Draws a Markdown text. `preview` turns a link target into hover text.
/// Returns the link clicked this frame, if any.
pub fn render(ui: &mut egui::Ui, markdown: &str, preview: &dyn Fn(&str) -> String) -> Option<String> {
    let mut clicked = None;
    for block in blocks(markdown) {
        let link = match block {
            Block::Heading(level, text) => {
                let size = match level {
                    1 => 20.0,
                    2 => 16.0,
                    _ => 14.0,
                };
                ui.add_space(4.0);
                ui.label(egui::RichText::new(text).strong().size(size));
                None
            }
            Block::Bullet(text) => ui
                .horizontal_top(|ui| {
                    ui.label("•");
                    draw_spans(ui, &text, preview)
                })
                .inner,
            Block::Numbered(n, text) => ui
                .horizontal_top(|ui| {
                    ui.label(format!("{n}."));
                    draw_spans(ui, &text, preview)
                })
                .inner,
            Block::Quote(text) => ui
                .horizontal(|ui| {
                    ui.label(egui::RichText::new("▎").weak());
                    draw_spans(ui, &text, preview)
                })
                .inner,
            Block::Code(code) => {
                egui::Frame::NONE.fill(ui.visuals().code_bg_color).inner_margin(6.0).corner_radius(4.0).show(ui, |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(code).monospace().small()).wrap());
                });
                None
            }
            Block::Paragraph(text) => draw_spans(ui, &text, preview),
        };
        clicked = clicked.or(link);
        ui.add_space(2.0);
    }
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_split_headings_lists_quotes_code_and_paragraphs() {
        let md = "# Title\nintro line\nsecond line\n\n- a\n* b\n1. one\n2. two\n> quoted\n```rust\nlet x = 1;\n```\n## Sub\n";
        assert_eq!(
            blocks(md),
            vec![
                Block::Heading(1, "Title".into()),
                Block::Paragraph("intro line second line".into()),
                Block::Bullet("a".into()),
                Block::Bullet("b".into()),
                Block::Numbered(1, "one".into()),
                Block::Numbered(2, "two".into()),
                Block::Quote("quoted".into()),
                Block::Code("let x = 1;".into()),
                Block::Heading(2, "Sub".into()),
            ]
        );
    }

    #[test]
    fn spans_find_bold_code_and_links() {
        assert_eq!(
            spans("see **this** and `that`, then [`fn f`](cc://symbol/a.rs:1:4) ok"),
            vec![
                Span::Text("see ".into()),
                Span::Bold("this".into()),
                Span::Text(" and ".into()),
                Span::Code("that".into()),
                Span::Text(", then ".into()),
                Span::Link { text: "fn f".into(), url: "cc://symbol/a.rs:1:4".into() },
                Span::Text(" ok".into()),
            ]
        );
        assert_eq!(spans("a ** b"), vec![Span::Text("a ** b".into())], "an unclosed marker stays text");
        assert_eq!(spans("ação **é**"), vec![Span::Text("ação ".into()), Span::Bold("é".into())]);
    }

    #[test]
    fn headings_need_a_space_and_at_most_six_hashes() {
        assert_eq!(heading("#tag"), None);
        assert_eq!(heading("####### x"), None);
        assert_eq!(heading("### x"), Some((3, "x")));
    }
}
