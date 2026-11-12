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
