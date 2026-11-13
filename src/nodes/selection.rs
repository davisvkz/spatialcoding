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

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(sel: &Selection) -> Vec<u64> {
        let mut ids: Vec<u64> = sel.selected_ids().collect();
        ids.sort();
        ids
    }

    #[test]
    fn select_replaces_the_selection() {
        let mut sel = Selection::default();
        sel.select(1);
        sel.select(2);

        assert_eq!(sorted(&sel), [2]);
        assert_eq!(sel.primary, Some(2));
        assert!(!sel.is_selected(1));
    }

    #[test]
    fn toggle_adds_and_removes_and_moves_primary() {
        let mut sel = Selection::default();
        sel.toggle(1);
        sel.toggle(2);
        assert_eq!(sorted(&sel), [1, 2]);
        assert_eq!(sel.primary, Some(2));

        sel.toggle(2);
        assert_eq!(sorted(&sel), [1]);
        assert_eq!(sel.primary, Some(1), "primary falls back to a remaining id");

        sel.toggle(1);
        assert_eq!(sel.count(), 0);
        assert_eq!(sel.primary, None);
    }

    #[test]
    fn toggling_off_a_non_primary_keeps_primary() {
        let mut sel = Selection::default();
        sel.select(1);
        sel.toggle(2);
        sel.toggle(1);
        assert_eq!(sel.primary, Some(2));
    }

    #[test]
    fn select_all_and_clear() {
        let mut sel = Selection::default();
        sel.select(9);
        sel.select_all([3, 4, 5]);
        assert_eq!(sorted(&sel), [3, 4, 5]);
        assert!(sel.primary.is_some_and(|p| sel.is_selected(p)));

        sel.select_all([]);
        assert_eq!(sel.primary, None);

        sel.select(1);
        sel.clear();
        assert_eq!(sel.count(), 0);
        assert_eq!(sel.primary, None);
    }
}
