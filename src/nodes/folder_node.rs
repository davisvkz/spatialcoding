use std::path::PathBuf;

/// Band reserved above a folder's contents, inside its box. The hierarchical
/// layout reserves the same space so laid-out folders never overlap.
pub const FOLDER_HEADER_HEIGHT: f32 = 28.0;

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
        let header_height = FOLDER_HEADER_HEIGHT;
        self.bounds = egui::Rect::from_min_max(
            egui::pos2(combined.min.x - padding, combined.min.y - padding - header_height),
            egui::pos2(combined.max.x + padding, combined.max.y + padding),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, Rect};

    #[test]
    fn label_comes_from_the_last_path_component() {
        assert_eq!(FolderNode::new(1, PathBuf::from("/a/b/components"), 3).label, "components");
        assert_eq!(FolderNode::new(1, PathBuf::from("/"), 0).label, "folder");
    }

    #[test]
    fn update_bounds_wraps_children_with_padding_and_header() {
        let mut folder = FolderNode::new(1, PathBuf::from("src"), 2);
        let children = [
            Rect::from_min_max(pos2(0.0, 0.0), pos2(10.0, 10.0)),
            Rect::from_min_max(pos2(20.0, 30.0), pos2(40.0, 50.0)),
        ];

        folder.update_bounds(&children, 5.0);

        assert_eq!(
            folder.bounds,
            Rect::from_min_max(pos2(-5.0, -5.0 - 28.0), pos2(45.0, 55.0))
        );
    }

    #[test]
    fn update_bounds_without_children_keeps_previous_bounds() {
        let mut folder = FolderNode::new(1, PathBuf::from("src"), 0);
        folder.update_bounds(&[], 5.0);
        assert_eq!(folder.bounds, Rect::NOTHING);
    }
}
