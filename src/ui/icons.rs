//! Lucide icon font and codepoints.
//!
//! The TTF under `assets/fonts/lucide.ttf` is registered as a dedicated
//! `FontFamily::Name("lucide")` so icon glyphs render independently of the UI
//! text font. Call [`install`] once at startup, from `SpatialCodingApp::new`.
//!
//! Icon constants are PUA codepoints from `lucide-static`'s `info.json`; look
//! an icon up by name there if you need a new one.

use egui::{FontData, FontDefinitions, FontFamily, WidgetInfo, WidgetType};

const LUCIDE_TTF: &[u8] = include_bytes!("../../assets/fonts/lucide.ttf");

/// Family the icon glyphs are registered under. Pass this to `RichText::new`
/// via `.family()` when drawing one.
pub const ICON_FAMILY: &str = "lucide";

/// Register the Lucide icon font with egui.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("lucide".to_owned(), FontData::from_static(LUCIDE_TTF).into());
    fonts
        .families
        .insert(FontFamily::Name(ICON_FAMILY.into()), vec!["lucide".into()]);
    // Also append lucide to the Proportional stack as a fallback so an icon
    // char embedded in a normal label renders instead of showing a tofu.
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .push("lucide".into());
    ctx.set_fonts(fonts);
}

/// Build a [`RichText`] in the icon font at the given size.
pub fn icon(ch: &str, size: f32) -> egui::RichText {
    egui::RichText::new(ch)
        .family(FontFamily::Name(ICON_FAMILY.into()))
        .size(size)
}

/// A square rail icon button whose accessible name is `label` (so
/// `get_by_label(label)` still works in e2e tests) while visually showing only
/// the icon glyph.
pub fn rail_icon_button(ui: &mut egui::Ui, ch: &str, label: &str, selected: bool) -> egui::Response {
    let response = ui.add_sized(
        egui::vec2(32.0, 32.0),
        egui::SelectableLabel::new(selected, icon(ch, 18.0)),
    );
    let ak_label = label.to_owned();
    response.widget_info(move || {
        WidgetInfo::selected(WidgetType::SelectableLabel, true, selected, ak_label.clone())
    });
    response.on_hover_text(label.to_owned())
}

/// Small frameless icon button (header kebab/play/close, action grids).
pub fn tool_icon_button(ui: &mut egui::Ui, ch: &str, label: &str) -> egui::Response {
    let response = ui.add_sized(
        egui::vec2(24.0, 24.0),
        egui::Button::new(icon(ch, 14.0)).frame(false),
    );
    let ak_label = label.to_owned();
    response.widget_info(move || WidgetInfo::labeled(WidgetType::Button, true, ak_label.clone()));
    response.on_hover_text(label.to_owned())
}

// -- Lucide PUA codepoints (from lucide-static@latest/font/info.json) --------

// Left rail
pub const FOLDER_TREE: &str = "\u{e33c}";
pub const GIT_BRANCH: &str = "\u{e0e2}";
pub const SEARCH: &str = "\u{e151}";
pub const SERVER: &str = "\u{e153}";

// Left rail footer
pub const BUG: &str = "\u{e20c}";
pub const HELP: &str = "\u{e082}";
pub const SETTINGS: &str = "\u{e154}";
pub const USER: &str = "\u{e461}";

// Right rail
pub const LAYOUT: &str = "\u{e1c1}";
pub const SYMBOLS: &str = "\u{e408}";
pub const EDGES: &str = "\u{e38b}";
pub const FOLDERS: &str = "\u{e33f}";
pub const FILE_NODES: &str = "\u{e0cf}";
pub const CST_TREE: &str = "\u{e33c}";
pub const AGENT_NOTES: &str = "\u{e117}";
pub const PERF_GAUGE: &str = "\u{e1bf}";

// Header actions
pub const KEBAB: &str = "\u{e0b7}";
pub const PLAY: &str = "\u{e13c}";
pub const CLOSE: &str = "\u{e1b2}";
pub const CHEVRON_LEFT: &str = "\u{e06e}";
pub const CHEVRON_RIGHT: &str = "\u{e06f}";
pub const CHEVRON_UP: &str = "\u{e070}";
pub const CHEVRON_DOWN: &str = "\u{e06d}";
pub const INFO: &str = "\u{e0f9}";
pub const ROTATE_CCW: &str = "\u{e148}";
pub const PAUSE: &str = "\u{e12e}";

// Panel bodies
pub const COPY: &str = "\u{e09e}";
pub const TRASH: &str = "\u{e18e}";
pub const REFRESH: &str = "\u{e144}";
pub const MAXIMIZE: &str = "\u{e112}";
pub const FOLDER: &str = "\u{e0d7}";
pub const FILE: &str = "\u{e0c0}";
pub const TERMINAL: &str = "\u{e181}";
pub const CIRCLE_DOT: &str = "\u{e345}";

// File nodes actions
pub const FILES: &str = "\u{e0cf}";
pub const FILE_DIFF: &str = "\u{e317}";
pub const FILE_MINUS: &str = "\u{e0c6}";
pub const ARROW_LEFT_TO_LINE: &str = "\u{e457}";
pub const ARROW_RIGHT_TO_LINE: &str = "\u{e459}";
pub const SCAN: &str = "\u{e257}";
pub const CHECK_SQUARE: &str = "\u{e16a}";
pub const SQUARE: &str = "\u{e167}";
pub const LAYERS: &str = "\u{e529}";

// Folders actions
pub const CHEVRONS_UP: &str = "\u{e074}";
pub const CHEVRONS_DOWN: &str = "\u{e071}";
pub const FOLDER_CLOSED: &str = "\u{e330}";
pub const FOLDER_OPEN: &str = "\u{e247}";
pub const FOLDER_UP: &str = "\u{e33d}";
pub const FOLDER_DOWN: &str = "\u{e332}";
pub const FOLDER_X: &str = "\u{e33e}";
pub const FOLDER_INPUT: &str = "\u{e334}";
pub const SLASH: &str = "\u{e51d}";
