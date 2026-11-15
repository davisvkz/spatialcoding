#[derive(Debug, Clone)]
pub struct Theme {
    pub canvas_bg: egui::Color32,
    pub panel_bg: egui::Color32,
    pub panel_border: egui::Color32,
    pub text_primary: egui::Color32,
    pub text_secondary: egui::Color32,
    pub text_muted: egui::Color32,
    pub accent: egui::Color32,
    pub selection: egui::Color32,
    pub node_bg: egui::Color32,
    pub node_border: egui::Color32,
    /// Border of a selected file node (v2: near-white, 1px).
    pub selected_border: egui::Color32,
    pub node_header: egui::Color32,
    pub folder_bg: egui::Color32,
    pub folder_border: egui::Color32,
    pub edge_color: egui::Color32,
    pub grid_dot: egui::Color32,
    pub minimap_bg: egui::Color32,
    pub minimap_viewport: egui::Color32,
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            canvas_bg: egui::Color32::from_rgb(0x0b, 0x0b, 0x0b),
            panel_bg: egui::Color32::from_rgb(0x25, 0x25, 0x25),
            panel_border: egui::Color32::from_rgb(0x33, 0x33, 0x33),
            text_primary: egui::Color32::from_rgb(0xe0, 0xe0, 0xe0),
            text_secondary: egui::Color32::from_rgb(0x99, 0x99, 0x99),
            text_muted: egui::Color32::from_rgb(0x66, 0x66, 0x66),
            accent: egui::Color32::from_rgb(0xf5, 0xc5, 0x42),
            selection: egui::Color32::from_rgb(0x3b, 0x82, 0xf6),
            node_bg: egui::Color32::from_rgb(0x10, 0x10, 0x10),
            node_border: egui::Color32::from_rgb(0x34, 0x34, 0x34),
            selected_border: egui::Color32::from_white_alpha(220),
            node_header: egui::Color32::from_rgb(0x2a, 0x2a, 0x2a),
            folder_bg: egui::Color32::TRANSPARENT,
            folder_border: egui::Color32::from_rgb(0x2e, 0x2e, 0x2e),
            edge_color: egui::Color32::from_rgba_premultiplied(0xaa, 0xaa, 0xaa, 40),
            grid_dot: egui::Color32::from_rgba_premultiplied(0x44, 0x44, 0x44, 60),
            minimap_bg: egui::Color32::from_rgba_premultiplied(0, 0, 0, 180),
            minimap_viewport: egui::Color32::from_rgba_premultiplied(0xf5, 0xc5, 0x42, 80),
        }
    }
}
