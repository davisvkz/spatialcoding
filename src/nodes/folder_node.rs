use std::path::PathBuf;

pub struct FolderNode {
    pub id: u64,
    pub path: PathBuf,
    pub label: String,
    pub file_count: usize,
    pub bounds: egui::Rect,
    pub collapsed: bool,
    pub children_ids: Vec<u64>,
}

impl FolderNode {
    pub fn new(id: u64, path: PathBuf, file_count: usize) -> Self {
        let label = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("folder")
            .to_string();
        Self {
            id,
            path,
            label,
            file_count,
            bounds: egui::Rect::NOTHING,
            collapsed: false,
            children_ids: Vec::new(),
        }
    }

    pub fn update_bounds(&mut self, child_rects: &[egui::Rect], padding: f32) {
        if child_rects.is_empty() {
            return;
        }
        let mut combined = child_rects[0];
        for rect in &child_rects[1..] {
            combined = combined.union(*rect);
        }
        let header_height = 28.0;
        self.bounds = egui::Rect::from_min_max(
            egui::pos2(combined.min.x - padding, combined.min.y - padding - header_height),
            egui::pos2(combined.max.x + padding, combined.max.y + padding),
        );
    }
}
