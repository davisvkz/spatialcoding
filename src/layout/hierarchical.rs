use std::collections::HashMap;

use petgraph::algo::toposort;
use petgraph::graph::Graph;

use crate::edges::edge_store::EdgeStore;
use crate::layout::settings::{LayoutSettings, SortMode, VAlign};
use crate::nodes::lod::LodLevel;
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
        let max_parent_rank = graph
            .neighbors_directed(*idx, petgraph::Direction::Incoming)
            .filter_map(|parent| rank_of.get(&graph[parent]).copied())
            .max()
            .unwrap_or(0);
        let rank = if rank_of.is_empty() { 0 } else { max_parent_rank + 1 };
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
    let lod = LodLevel::Summary;
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
            .map(|n| n.world_rect(node_width, font_size, lod).height())
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
                .map(|n| n.world_rect(node_width, font_size, lod).height())
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
