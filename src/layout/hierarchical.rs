use std::collections::HashMap;

use petgraph::algo::toposort;
use petgraph::graph::Graph;

use crate::edges::edge_store::EdgeStore;
use crate::layout::settings::{LayoutSettings, SortMode, VAlign};
use crate::nodes::node_store::NodeStore;

pub fn apply_layout(
    nodes: &mut NodeStore,
    edges: &EdgeStore,
    settings: &LayoutSettings,
) {
    let node_ids = nodes.all_file_ids();
    if node_ids.is_empty() {
        return;
    }

    let mut graph = Graph::<u64, ()>::new();
    let mut id_to_idx: HashMap<u64, petgraph::graph::NodeIndex> = HashMap::new();

    for &nid in &node_ids {
        let idx = graph.add_node(nid);
        id_to_idx.insert(nid, idx);
    }

    for edge in edges.all_edges() {
        if let (Some(&src_idx), Some(&tgt_idx)) =
            (id_to_idx.get(&edge.source_node), id_to_idx.get(&edge.target_node))
        {
            graph.add_edge(src_idx, tgt_idx, ());
        }
    }

    let sorted = match toposort(&graph, None) {
        Ok(order) => order,
        Err(_) => graph.node_indices().collect(),
    };

    let mut rank_of: HashMap<u64, usize> = HashMap::new();
    for idx in &sorted {
        let nid = graph[*idx];
        // Roots (no ranked parent) go in rank 0; others one past their deepest parent.
        let rank = graph
            .neighbors_directed(*idx, petgraph::Direction::Incoming)
            .filter_map(|parent| rank_of.get(&graph[parent]).copied())
            .max()
            .map_or(0, |r| r + 1);
        rank_of.insert(nid, rank);
    }

    let mut ranks: HashMap<usize, Vec<u64>> = HashMap::new();
    for (&nid, &rank) in &rank_of {
        ranks.entry(rank).or_default().push(nid);
    }

    for members in ranks.values_mut() {
        match settings.sort {
            SortMode::Asc => members.sort(),
            SortMode::Desc => members.sort_by(|a, b| b.cmp(a)),
            SortMode::Alt => { members.sort(); interleave(members); }
            SortMode::AltRev => { members.sort_by(|a, b| b.cmp(a)); interleave(members); }
            SortMode::Off => {}
        }
    }

    let node_width = 700.0_f32;
    let font_size = 12.0_f32;

    let max_rank = ranks.keys().copied().max().unwrap_or(0);
    for rank in 0..=max_rank {
        let members = match ranks.get(&rank) {
            Some(m) => m,
            None => continue,
        };

        let x = rank as f32 * (node_width + settings.file_spacing);
        let total_height: f32 = members
            .iter()
            .filter_map(|&nid| nodes.get_file(nid))
            .map(|n| n.world_rect(node_width, font_size).height())
            .sum::<f32>()
            + (members.len().saturating_sub(1) as f32) * settings.file_spacing;

        let mut y = match settings.v_align {
            VAlign::Top => 0.0,
            VAlign::Center => -total_height * 0.5,
            VAlign::Bottom => -total_height,
            VAlign::Fill => 0.0,
        };

        for &nid in members {
            let node_height = nodes
                .get_file(nid)
                .map(|n| n.world_rect(node_width, font_size).height())
                .unwrap_or(100.0);

            if let Some(node) = nodes.get_file_mut(nid) {
                node.position = egui::pos2(x, y);
            }
            y += node_height + settings.file_spacing;
        }
    }
}

fn interleave(items: &mut Vec<u64>) {
    if items.len() <= 2 {
        return;
    }
    let original = items.clone();
    let mid = (original.len() + 1) / 2;
    let mut idx = 0;
    for i in 0..mid {
        items[idx] = original[i];
        idx += 1;
        let j = mid + i;
        if j < original.len() {
            items[idx] = original[j];
            idx += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const NODE_WIDTH: f32 = 700.0;
    const SPACING: f32 = 100.0;
    const COLUMN: f32 = NODE_WIDTH + SPACING;

    /// Starting position far from anything the layout produces, so untouched nodes are detectable.
    /// Kept small: heights are measured at the current position, and f32 loses precision far out.
    const UNPLACED: egui::Pos2 = egui::pos2(-7.0, -7.0);

    fn assert_close(a: f32, b: f32) {
        assert!((a - b).abs() < 1e-3, "{a} != {b}");
    }

    /// `n` one-line file nodes, all starting at `UNPLACED`.
    fn store(n: usize) -> (NodeStore, Vec<u64>) {
        let mut nodes = NodeStore::default();
        let ids = (0..n)
            .map(|i| nodes.add_file(PathBuf::from(format!("f{i}.rs")), "x\n".into(), UNPLACED))
            .collect();
        (nodes, ids)
    }

    fn settings(sort: SortMode, v_align: VAlign) -> LayoutSettings {
        LayoutSettings {
            sort,
            v_align,
            file_spacing: SPACING,
            ..Default::default()
        }
    }

    fn pos(nodes: &NodeStore, id: u64) -> egui::Pos2 {
        nodes.get_file(id).unwrap().position
    }

    fn height(nodes: &NodeStore, id: u64) -> f32 {
        nodes.get_file(id).unwrap().world_rect(NODE_WIDTH, 12.0).height()
    }

    #[test]
    fn empty_store_is_a_no_op() {
        let mut nodes = NodeStore::default();
        apply_layout(&mut nodes, &EdgeStore::default(), &LayoutSettings::default());
        assert!(nodes.all_file_ids().is_empty());
    }

    #[test]
    fn chain_places_each_node_one_column_further() {
        let (mut nodes, ids) = store(3);
        let mut edges = EdgeStore::default();
        edges.add(ids[0], ids[1], None);
        edges.add(ids[1], ids[2], None);

        apply_layout(&mut nodes, &edges, &settings(SortMode::Asc, VAlign::Top));

        for (rank, &id) in ids.iter().enumerate() {
            assert_eq!(pos(&nodes, id), egui::pos2(rank as f32 * COLUMN, 0.0));
        }
    }

    #[test]
    fn rank_is_longest_path_from_a_root() {
        // a -> b -> c and a -> c: c must sit after b, not next to it.
        let (mut nodes, ids) = store(3);
        let mut edges = EdgeStore::default();
        edges.add(ids[0], ids[1], None);
        edges.add(ids[0], ids[2], None);
        edges.add(ids[1], ids[2], None);

        apply_layout(&mut nodes, &edges, &settings(SortMode::Asc, VAlign::Top));

        assert_eq!(pos(&nodes, ids[2]).x, 2.0 * COLUMN);
    }

    #[test]
    fn independent_roots_share_rank_zero() {
        // Two separate roots (a -> b, c) must both land in the first column.
        let (mut nodes, ids) = store(3);
        let mut edges = EdgeStore::default();
        edges.add(ids[0], ids[1], None);

        apply_layout(&mut nodes, &edges, &settings(SortMode::Asc, VAlign::Top));

        assert_eq!(pos(&nodes, ids[0]).x, 0.0);
        assert_eq!(pos(&nodes, ids[2]).x, 0.0);
        assert_eq!(pos(&nodes, ids[1]).x, COLUMN);
    }

    #[test]
    fn cycles_and_dangling_edges_still_place_every_node() {
        let (mut nodes, ids) = store(3);
        let mut edges = EdgeStore::default();
        edges.add(ids[0], ids[1], None);
        edges.add(ids[1], ids[0], None);
        edges.add(ids[2], 999, None);

        apply_layout(&mut nodes, &edges, &settings(SortMode::Asc, VAlign::Top));

        for &id in &ids {
            assert_ne!(pos(&nodes, id), UNPLACED, "node {id} not placed");
        }
    }

    #[test]
    fn sort_mode_orders_nodes_within_a_rank() {
        let (mut nodes, ids) = store(3);
        let edges = EdgeStore::default();

        apply_layout(&mut nodes, &edges, &settings(SortMode::Asc, VAlign::Top));
        let ys: Vec<f32> = ids.iter().map(|&id| pos(&nodes, id).y).collect();
        assert!(ys[0] < ys[1] && ys[1] < ys[2], "asc: {ys:?}");
        assert_eq!(ys[0], 0.0);
        assert_close(ys[1], height(&nodes, ids[0]) + SPACING);

        apply_layout(&mut nodes, &edges, &settings(SortMode::Desc, VAlign::Top));
        let ys: Vec<f32> = ids.iter().map(|&id| pos(&nodes, id).y).collect();
        assert!(ys[0] > ys[1] && ys[1] > ys[2], "desc: {ys:?}");
    }

    #[test]
    fn center_alignment_centers_a_rank_on_y_zero() {
        let (mut nodes, ids) = store(2);
        apply_layout(
            &mut nodes,
            &EdgeStore::default(),
            &settings(SortMode::Asc, VAlign::Center),
        );

        let top = pos(&nodes, ids[0]).y;
        let bottom = pos(&nodes, ids[1]).y + height(&nodes, ids[1]);
        assert_close(top, -bottom);
        assert!(top < 0.0);
    }

    #[test]
    fn bottom_alignment_ends_a_rank_at_y_zero() {
        let (mut nodes, ids) = store(2);
        apply_layout(
            &mut nodes,
            &EdgeStore::default(),
            &settings(SortMode::Asc, VAlign::Bottom),
        );

        let bottom = pos(&nodes, ids[1]).y + height(&nodes, ids[1]);
        assert_close(bottom, 0.0);
    }

    #[test]
    fn interleave_alternates_halves() {
        let mut odd = vec![1, 2, 3, 4, 5];
        interleave(&mut odd);
        assert_eq!(odd, [1, 4, 2, 5, 3]);

        let mut even = vec![1, 2, 3, 4];
        interleave(&mut even);
        assert_eq!(even, [1, 3, 2, 4]);

        let mut short = vec![2, 1];
        interleave(&mut short);
        assert_eq!(short, [2, 1]);

        let mut empty: Vec<u64> = vec![];
        interleave(&mut empty);
        assert!(empty.is_empty());
    }
}
