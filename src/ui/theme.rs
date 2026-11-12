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
            canvas_bg: egui::Color32::from_rgb(0x1a, 0x1a, 0x1a),
            panel_bg: egui::Color32::from_rgb(0x25, 0x25, 0x25),
            panel_border: egui::Color32::from_rgb(0x33, 0x33, 0x33),
            text_primary: egui::Color32::from_rgb(0xe0, 0xe0, 0xe0),
            text_secondary: egui::Color32::from_rgb(0x99, 0x99, 0x99),
            text_muted: egui::Color32::from_rgb(0x66, 0x66, 0x66),
            accent: egui::Color32::from_rgb(0xf5, 0xc5, 0x42),
            selection: egui::Color32::from_rgb(0x3b, 0x82, 0xf6),
            node_bg: egui::Color32::from_rgb(0x1e, 0x1e, 0x1e),
            node_border: egui::Color32::from_rgb(0x3a, 0x3a, 0x3a),
            node_header: egui::Color32::from_rgb(0x2a, 0x2a, 0x2a),
            folder_bg: egui::Color32::from_rgba_premultiplied(0x1c, 0x1c, 0x1c, 200),
            folder_border: egui::Color32::from_rgba_premultiplied(0x33, 0x33, 0x33, 180),
            edge_color: egui::Color32::from_rgba_premultiplied(0xaa, 0xaa, 0xaa, 40),
            grid_dot: egui::Color32::from_rgba_premultiplied(0x44, 0x44, 0x44, 60),
            minimap_bg: egui::Color32::from_rgba_premultiplied(0, 0, 0, 180),
            minimap_viewport: egui::Color32::from_rgba_premultiplied(0xf5, 0xc5, 0x42, 80),
        }
    }
}
