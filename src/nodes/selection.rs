use std::collections::HashSet;

pub struct Selection {
    pub selected: HashSet<u64>,
    pub primary: Option<u64>,
}

impl Default for Selection {
    fn default() -> Self {
        Self {
            selected: HashSet::new(),
            primary: None,
        }
    }
}

impl Selection {
    pub fn select(&mut self, id: u64) {
        self.selected.clear();
        self.selected.insert(id);
        self.primary = Some(id);
    }

    pub fn toggle(&mut self, id: u64) {
        if self.selected.contains(&id) {
            self.selected.remove(&id);
            if self.primary == Some(id) {
                self.primary = self.selected.iter().next().copied();
            }
        } else {
            self.selected.insert(id);
            self.primary = Some(id);
        }
    }

    pub fn select_all(&mut self, ids: impl IntoIterator<Item = u64>) {
        self.selected.clear();
        self.selected.extend(ids);
        self.primary = self.selected.iter().next().copied();
    }

    pub fn clear(&mut self) {
        self.selected.clear();
        self.primary = None;
    }

    pub fn is_selected(&self, id: u64) -> bool {
        self.selected.contains(&id)
    }

    pub fn count(&self) -> usize {
        self.selected.len()
    }

    pub fn selected_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.selected.iter().copied()
    }
}
