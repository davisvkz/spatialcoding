//! Docs window (Ctrl+Shift+/): the guide embedded in the binary, shown with
//! headings, bullets and paragraphs.

use crate::state::AppState;

pub const GUIDE: &str = include_str!("../../assets/guide.md");

/// One line of the guide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block<'a> {
    Title(&'a str),
    Heading(&'a str),
    Bullet(&'a str),
    Text(&'a str),
}

/// The blocks of a Markdown guide (`#`, `##`, `- ` and text lines).
pub fn parse(guide: &str) -> Vec<Block<'_>> {
    guide
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            if let Some(rest) = line.strip_prefix("## ") {
                Block::Heading(rest)
            } else if let Some(rest) = line.strip_prefix("# ") {
                Block::Title(rest)
            } else if let Some(rest) = line.strip_prefix("- ") {
                Block::Bullet(rest)
            } else {
                Block::Text(line)
            }
        })
        .collect()
}

/// Titles of the sections, for the navigation list and MCP `read_docs`.
pub fn sections(guide: &str) -> Vec<&str> {
    parse(guide)
        .into_iter()
        .filter_map(|b| if let Block::Heading(h) = b { Some(h) } else { None })
        .collect()
}

pub fn draw(ctx: &egui::Context, state: &mut AppState) {
    if !state.show_docs {
        return;
    }
    let mut open = true;
    egui::Window::new("Docs")
        .open(&mut open)
        .default_size([520.0, 480.0])
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for block in parse(GUIDE) {
                    match block {
                        Block::Title(t) => {
                            ui.heading(t);
                        }
                        Block::Heading(h) => {
                            ui.add_space(8.0);
                            ui.label(egui::RichText::new(h).strong().size(16.0));
                        }
                        Block::Bullet(b) => {
                            ui.horizontal_wrapped(|ui| {
                                ui.label("•");
                                ui.label(b);
                            });
                        }
                        Block::Text(t) => {
                            ui.label(t);
                        }
                    }
                }
            });
        });
    state.show_docs = open;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guide_parses_into_title_sections_and_bullets() {
        let blocks = parse("# T\n\ntext\n\n## A\n- one\n- two\n");
        assert_eq!(
            blocks,
            vec![Block::Title("T"), Block::Text("text"), Block::Heading("A"), Block::Bullet("one"), Block::Bullet("two")]
        );
        let names = sections(GUIDE);
        assert!(names.contains(&"Move around") && names.contains(&"Panels"), "{names:?}");
    }
}
