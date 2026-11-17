//! Colors and the global egui style, measured from v2.codecanvas.app (a zinc
//! palette on a near-black chrome). Every color the UI paints comes from here.

use egui::Color32;

#[derive(Debug, Clone)]
pub struct Theme {
    /// Top bar and rails behind everything.
    pub chrome_bg: Color32,
    pub canvas_bg: Color32,
    /// 1px outline of the rounded canvas area.
    pub canvas_frame_border: Color32,
    pub panel_bg: Color32,
    pub panel_border: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    /// Idle color of rail and toolbar icons.
    pub rail_icon: Color32,
    /// Fill of controls (pill, segmented container, hovered rail icon).
    pub control_bg: Color32,
    pub control_border: Color32,
    /// Fill of the active tool, segment or rail entry.
    pub control_active: Color32,
    /// White call-to-action button (Open All, Apply Layout).
    pub cta_bg: Color32,
    pub cta_fg: Color32,
    pub accent: Color32,
    pub selection: Color32,
    pub node_bg: Color32,
    pub node_border: Color32,
    /// Border of a selected file node (v2: near-white, 1px).
    pub selected_border: Color32,
    pub node_header: Color32,
    pub folder_bg: Color32,
    pub folder_border: Color32,
    pub edge_color: Color32,
    pub grid_dot: Color32,
    pub minimap_bg: Color32,
    pub minimap_viewport: Color32,
    /// Line numbers in the gutter of a file node.
    pub line_number: Color32,
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            chrome_bg: Color32::from_rgb(0x09, 0x09, 0x0b),
            canvas_bg: Color32::from_rgb(0x09, 0x09, 0x09),
            canvas_frame_border: Color32::from_rgba_unmultiplied(0x27, 0x27, 0x2a, 178),
            panel_bg: Color32::from_rgb(0x09, 0x09, 0x0b),
            panel_border: Color32::from_rgba_unmultiplied(0x27, 0x27, 0x2a, 140),
            text_primary: Color32::from_rgb(0xd4, 0xd4, 0xd8),
            text_secondary: Color32::from_rgb(0xa1, 0xa1, 0xaa),
            text_muted: Color32::from_rgb(0x71, 0x71, 0x7a),
            rail_icon: Color32::from_rgb(0x96, 0x96, 0x9b),
            control_bg: Color32::from_rgb(0x1d, 0x1d, 0x1f),
            control_border: Color32::from_rgba_unmultiplied(0x52, 0x52, 0x5b, 153),
            control_active: Color32::from_rgb(0x3f, 0x3f, 0x46),
            cta_bg: Color32::WHITE,
            cta_fg: Color32::from_rgb(0x18, 0x18, 0x1b),
            accent: Color32::from_rgb(0xf5, 0xc5, 0x42),
            selection: Color32::from_rgb(0x3b, 0x82, 0xf6),
            node_bg: Color32::from_rgb(0x0c, 0x0c, 0x0e),
            node_border: Color32::from_rgba_unmultiplied(0x71, 0x71, 0x7a, 128),
            selected_border: Color32::from_white_alpha(220),
            node_header: Color32::from_rgb(0x2a, 0x2a, 0x2a),
            folder_bg: Color32::TRANSPARENT,
            folder_border: Color32::from_rgba_unmultiplied(0x52, 0x52, 0x5b, 140),
            edge_color: Color32::from_rgba_premultiplied(0xaa, 0xaa, 0xaa, 40),
            grid_dot: Color32::from_rgba_premultiplied(0x44, 0x44, 0x44, 60),
            minimap_bg: Color32::from_rgba_unmultiplied(0x16, 0x16, 0x18, 235),
            minimap_viewport: Color32::from_rgba_premultiplied(0xf5, 0xc5, 0x42, 80),
            line_number: Color32::from_rgb(0x3f, 0x3f, 0x46),
        }
    }
}

/// Installs the v2 look into egui: dark zinc widgets, small Inter text and
/// tight spacing. Call once at startup, after `icons::install`.
pub fn apply_style(ctx: &egui::Context, theme: &Theme) {
    use egui::{CornerRadius, FontFamily, FontId, Stroke, TextStyle};

    ctx.style_mut(|style| {
        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.panel_fill = theme.chrome_bg;
        v.window_fill = theme.panel_bg;
        v.window_stroke = Stroke::new(1.0_f32, theme.panel_border);
        v.window_corner_radius = CornerRadius::same(10);
        v.menu_corner_radius = CornerRadius::same(8);
        v.extreme_bg_color = theme.control_bg;
        v.faint_bg_color = theme.control_bg;
        v.code_bg_color = theme.control_bg;
        v.override_text_color = None;
        v.hyperlink_color = theme.selection;
        v.selection.bg_fill = theme.control_active;
        v.selection.stroke = Stroke::new(1.0_f32, theme.text_primary);
        v.popup_shadow = egui::Shadow::NONE;
        v.window_shadow = egui::Shadow::NONE;

        let radius = CornerRadius::same(6);
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = theme.chrome_bg;
        w.noninteractive.weak_bg_fill = theme.chrome_bg;
        w.noninteractive.bg_stroke = Stroke::new(1.0_f32, theme.panel_border);
        w.noninteractive.fg_stroke = Stroke::new(1.0_f32, theme.text_primary);
        w.noninteractive.corner_radius = radius;

        w.inactive.bg_fill = theme.control_bg;
        w.inactive.weak_bg_fill = theme.control_bg;
        w.inactive.bg_stroke = Stroke::new(1.0_f32, theme.control_border);
        w.inactive.fg_stroke = Stroke::new(1.0_f32, theme.text_secondary);
        w.inactive.corner_radius = radius;

        w.hovered.bg_fill = Color32::from_rgb(0x27, 0x27, 0x2a);
        w.hovered.weak_bg_fill = Color32::from_rgb(0x27, 0x27, 0x2a);
        w.hovered.bg_stroke = Stroke::new(1.0_f32, theme.control_border);
        w.hovered.fg_stroke = Stroke::new(1.0_f32, theme.text_primary);
        w.hovered.corner_radius = radius;

        w.active.bg_fill = theme.control_active;
        w.active.weak_bg_fill = theme.control_active;
        w.active.bg_stroke = Stroke::new(1.0_f32, theme.control_border);
        w.active.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
        w.active.corner_radius = radius;

        w.open.bg_fill = theme.control_active;
        w.open.weak_bg_fill = theme.control_active;
        w.open.corner_radius = radius;

        let sp = &mut style.spacing;
        sp.item_spacing = egui::vec2(6.0, 4.0);
        sp.button_padding = egui::vec2(8.0, 3.0);
        sp.interact_size = egui::vec2(24.0, 22.0);
        sp.menu_margin = egui::Margin::same(4);

        let proportional = FontFamily::Proportional;
        style.text_styles = [
            (TextStyle::Heading, FontId::new(13.0, proportional.clone())),
            (TextStyle::Body, FontId::new(11.0, proportional.clone())),
            (TextStyle::Button, FontId::new(10.5, proportional.clone())),
            (TextStyle::Small, FontId::new(9.5, proportional)),
            (TextStyle::Monospace, FontId::new(11.0, FontFamily::Monospace)),
        ]
        .into();
    });
}
