use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Edge {
    pub id: u64,
    pub source_node: u64,
    pub target_node: u64,
    pub label: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EdgeStore {
    edges: HashMap<u64, Edge>,
    next_id: u64,
}

impl Default for EdgeStore {
    fn default() -> Self {
        Self {
            edges: HashMap::new(),
            next_id: 1,
        }
    }
}

impl EdgeStore {
    pub fn add(&mut self, source: u64, target: u64, label: Option<String>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.edges.insert(
            id,
            Edge {
                id,
                source_node: source,
                target_node: target,
                label,
            },
        );
        id
    }

    pub fn remove(&mut self, id: u64) {
        self.edges.remove(&id);
    }

    pub fn edges_for_node(&self, node_id: u64) -> Vec<u64> {
        self.edges
            .values()
            .filter(|e| e.source_node == node_id || e.target_node == node_id)
            .map(|e| e.id)
            .collect()
    }

    pub fn all_edges(&self) -> impl Iterator<Item = &Edge> {
        self.edges.values()
    }

    pub fn clear(&mut self) {
        self.edges.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_assigns_sequential_ids_and_stores_endpoints() {
        let mut store = EdgeStore::default();
        let e1 = store.add(1, 2, Some("Import".into()));
        let e2 = store.add(2, 3, None);

        assert_eq!((e1, e2), (1, 2));
        let edge = store.all_edges().find(|e| e.id == e1).unwrap();
        assert_eq!((edge.source_node, edge.target_node), (1, 2));
        assert_eq!(edge.label.as_deref(), Some("Import"));
    }

    #[test]
    fn edges_for_node_matches_source_or_target() {
        let mut store = EdgeStore::default();
        let e1 = store.add(1, 2, None);
        let e2 = store.add(2, 3, None);
        store.add(4, 5, None);

        let mut for_two = store.edges_for_node(2);
        for_two.sort();
        assert_eq!(for_two, [e1, e2]);
        assert!(store.edges_for_node(9).is_empty());
    }

    #[test]
    fn remove_and_clear() {
        let mut store = EdgeStore::default();
        let e1 = store.add(1, 2, None);
        store.add(2, 3, None);

        store.remove(e1);
        store.remove(999);
        assert_eq!(store.all_edges().count(), 1);

        store.clear();
        assert_eq!(store.all_edges().count(), 0);
    }
}
