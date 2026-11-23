//! Folder-aware hierarchical layout, after v2 Code Canvas.
//!
//! Folders are laid out as nested, non-overlapping clusters. Inside every
//! folder (and at the top level) its items, files and nested folders, are
//! placed in columns by dependency rank: an item that uses another sits to
//! its left. Cycles are broken by dropping DFS back edges, so every member
//! of a cycle still gets its own column. Each column is ordered once by the
//! barycenter of its predecessors to cut edge crossings.
//!
//! Rank Breaking splits a column that is much taller than it is wide into
//! side-by-side sub-columns of the same rank; H Align places narrower items
//! inside their column, and V Align places each column (Fill spreads its
//! items over the tallest column's height).

use std::collections::{BTreeMap, HashMap, HashSet};

use egui::{Pos2, Vec2};

use crate::edges::edge_store::{Edge, EdgeChange, EdgeKind, EdgeStore};
use crate::layout::settings::{HAlign, LayoutSettings, SortMode, VAlign};
use crate::nodes::folder_node::{COLLAPSED_FOLDER_SIZE, FOLDER_HEADER_HEIGHT};
use crate::nodes::node_store::NodeStore;

/// Vertical gap between items of one column, as a share of the horizontal
/// spacing: v2 stacks files much tighter than it spaces columns.
const VERTICAL_GAP_RATIO: f32 = 0.5;

/// A group wider than this many times its height is "too wide": v2 keeps
/// folders roughly square, stacking sibling folders instead of chaining them.
const MAX_GROUP_ASPECT: f32 = 1.6;

/// Gap between a collapsed folder's card and the revealed files under it.
const REVEALED_GAP: f32 = 24.0;

/// A box the layout places: a file node or a folder cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Item {
    File(u64),
    Folder(u64),
}

impl Item {
    fn id(self) -> u64 {
        match self {
            Item::File(id) | Item::Folder(id) => id,
        }
    }
}

/// One level of the folder tree: the top level (`None`) or a folder.
type Group = Option<u64>;

struct Placement {
    /// Item positions relative to their group's content origin.
    offsets: HashMap<Item, Vec2>,
    /// Laid-out content size of each group.
    sizes: HashMap<Group, Vec2>,
}

/// Whether an edge takes part in the layout. Barrel and re-export hops are
/// routing: the layout follows the direct dependency on the provider, or a
/// barrel would close false cycles through its crate or package root. Edges
/// that exist only in a Git baseline are not dependencies of the current
/// tree, so comparing against a revision must not move nodes.
fn feeds_layout(edge: &Edge) -> bool {
    edge.kind == EdgeKind::Direct && edge.change != EdgeChange::Deleted
}

pub fn apply_layout(nodes: &mut NodeStore, edges: &EdgeStore, settings: &LayoutSettings) {
    if nodes.file_nodes.is_empty() {
        return;
    }

    // Folder tree: each item's group, and each file's chain of groups from
    // the top level down to the item that holds it.
    let mut children: BTreeMap<Group, Vec<Item>> = BTreeMap::new();
    let mut group_of: HashMap<Item, Group> = HashMap::new();
    // A collapsed folder is one compact box: what it holds is not laid out
    // (its edges still rank the box, through the chains below).
    let hidden_folders = nodes.layout_hidden_folders();
    let collapsed = |fid: u64| nodes.layout_collapsed(fid);
    // Revealed files of each card, in a stable order.
    let mut revealed: HashMap<u64, Vec<u64>> = HashMap::new();
    let mut revealed_ids: Vec<u64> = nodes.view.revealed_files.iter().copied().collect();
    revealed_ids.sort();
    for file in revealed_ids {
        if let Some(card) = nodes.layout_card(file) {
            revealed.entry(card).or_default().push(file);
        }
    }
    let folder_ids: Vec<u64> = nodes.all_folder_ids();
    for &fid in &folder_ids {
        let parent = nodes.folder_parent(fid);
        group_of.insert(Item::Folder(fid), parent);
        if !hidden_folders.contains(&fid) {
            children.entry(parent).or_default().push(Item::Folder(fid));
        }
    }
    for fid in nodes.all_file_ids() {
        let group = nodes.file_folder(fid);
        group_of.insert(Item::File(fid), group);
        if !group.is_some_and(|g| collapsed(g) || hidden_folders.contains(&g)) {
            children.entry(group).or_default().push(Item::File(fid));
        }
    }
    // Folders without any file below them have nothing to lay out.
    let mut has_files: HashSet<u64> = HashSet::new();
    for fid in nodes.all_file_ids() {
        let mut g = group_of[&Item::File(fid)];
        while let Some(folder) = g {
            if !has_files.insert(folder) {
                break;
            }
            g = group_of.get(&Item::Folder(folder)).copied().flatten();
        }
    }
    for items in children.values_mut() {
        items.retain(|it| match it {
            Item::Folder(f) => has_files.contains(f),
            Item::File(_) => true,
        });
    }
    // The contents of collapsed folders are placed with them, not on their own.
    children.retain(|group, _| !group.is_some_and(|g| collapsed(g) || hidden_folders.contains(&g)));

    let chain = |file: u64| -> Vec<Item> {
        // Top-down: outermost folder first, the file last.
        let mut up = vec![Item::File(file)];
        let mut g = group_of.get(&Item::File(file)).copied().flatten();
        while let Some(folder) = g {
            up.push(Item::Folder(folder));
            g = group_of.get(&Item::Folder(folder)).copied().flatten();
        }
        up.reverse();
        up
    };

    // Each file edge becomes one item edge in the group where the two
    // files' chains part: the deepest group holding both.
    let mut group_edges: HashMap<Group, HashSet<(Item, Item)>> = HashMap::new();
    let mut chains: HashMap<u64, Vec<Item>> = HashMap::new();
    for edge in edges.all_edges() {
        if !feeds_layout(edge)
            || edge.source_node == edge.target_node
            || !nodes.file_nodes.contains_key(&edge.source_node)
            || !nodes.file_nodes.contains_key(&edge.target_node)
        {
            continue;
        }
        let cs = chains.entry(edge.source_node).or_insert_with(|| chain(edge.source_node)).clone();
        let ct = chains.entry(edge.target_node).or_insert_with(|| chain(edge.target_node)).clone();
        let k = cs.iter().zip(&ct).take_while(|(a, b)| a == b).count();
        let (Some(&a), Some(&b)) = (cs.get(k), ct.get(k)) else { continue };
        let group = if k == 0 { None } else { Some(cs[k - 1].id()) };
        group_edges.entry(group).or_default().insert((a, b));
    }

    // Bottom-up: deepest groups first, so a folder's size is known before
    // its parent places it.
    let mut groups: Vec<Group> = children.keys().copied().collect();
    let depth = |g: Group| -> usize {
        let mut d = 0;
        let mut cur = g;
        while let Some(f) = cur {
            d += 1;
            cur = group_of.get(&Item::Folder(f)).copied().flatten();
        }
        d
    };
    groups.sort_by_key(|&g| std::cmp::Reverse(depth(g)));

    let mut placement = Placement { offsets: HashMap::new(), sizes: HashMap::new() };
    let empty = HashSet::new();
    for group in groups {
        let items = &children[&group];
        let item_edges = group_edges.get(&group).unwrap_or(&empty);
        layout_group(group, items, item_edges, nodes, settings, &revealed, &mut placement);
    }

    // Top-down: absolute positions.
    let root_size = placement.sizes.get(&None).copied().unwrap_or(Vec2::ZERO);
    let root_y = match settings.v_align {
        VAlign::Center => -root_size.y * 0.5,
        VAlign::Bottom => -root_size.y,
        VAlign::Top | VAlign::Fill => 0.0,
    };
    let inset = Vec2::new(settings.folder_padding, settings.folder_padding + FOLDER_HEADER_HEIGHT);
    let mut stack: Vec<(Group, Pos2)> = vec![(None, Pos2::new(0.0, root_y))];
    while let Some((group, origin)) = stack.pop() {
        for &item in children.get(&group).map(Vec::as_slice).unwrap_or(&[]) {
            let Some(&offset) = placement.offsets.get(&item) else { continue };
            let pos = origin + offset;
            match item {
                Item::File(id) => {
                    if let Some(node) = nodes.get_file_mut(id) {
                        node.position = pos;
                    }
                }
                Item::Folder(id) if nodes.layout_collapsed(id) => {
                    if let Some(folder) = nodes.get_folder_mut(id) {
                        folder.collapsed_origin = Some(pos);
                    }
                    let mut y = pos.y + COLLAPSED_FOLDER_SIZE.y + REVEALED_GAP;
                    for &file in revealed.get(&id).map(Vec::as_slice).unwrap_or(&[]) {
                        if let Some(node) = nodes.get_file_mut(file) {
                            node.position = Pos2::new(pos.x, y);
                            y += node.world_rect(settings.node_width, settings.font_size).height() + REVEALED_GAP;
                        }
                    }
                }
                Item::Folder(id) => stack.push((Some(id), pos + inset)),
            }
        }
    }
}

/// Places `items` of one group in dependency columns and records their
/// offsets and the group's content size.
fn layout_group(
    group: Group,
    items: &[Item],
    item_edges: &HashSet<(Item, Item)>,
    nodes: &NodeStore,
    settings: &LayoutSettings,
    revealed: &HashMap<u64, Vec<u64>>,
    placement: &mut Placement,
) {
    if items.is_empty() {
        placement.sizes.insert(group, Vec2::ZERO);
        return;
    }
    let pad = settings.folder_padding;
    let size_of = |item: Item, sizes: &HashMap<Group, Vec2>| -> Vec2 {
        match item {
            Item::File(id) => nodes
                .get_file(id)
                .map(|n| n.world_rect(settings.node_width, settings.font_size).size())
                .unwrap_or(Vec2::new(settings.node_width, 100.0)),
            Item::Folder(id) if nodes.layout_collapsed(id) => {
                // The card, then its revealed files stacked under it.
                revealed.get(&id).map_or(COLLAPSED_FOLDER_SIZE, |files| {
                    files.iter().fold(COLLAPSED_FOLDER_SIZE, |acc, &f| {
                        let size = nodes
                            .get_file(f)
                            .map(|n| n.world_rect(settings.node_width, settings.font_size).size())
                            .unwrap_or(Vec2::ZERO);
                        Vec2::new(acc.x.max(size.x), acc.y + REVEALED_GAP + size.y)
                    })
                })
            }
            Item::Folder(id) => {
                let content = sizes.get(&Some(id)).copied().unwrap_or(Vec2::ZERO);
                content + Vec2::new(pad * 2.0, pad * 2.0 + FOLDER_HEADER_HEIGHT)
            }
        }
    };

    let item_size: HashMap<Item, Vec2> = items
        .iter()
        .map(|&it| (it, size_of(it, &placement.sizes)))
        .collect();
    let v_gap = settings.file_spacing * VERTICAL_GAP_RATIO;
    let arrange = |stack_folders: bool, max_columns: usize| -> Vec<Vec<Item>> {
        let edges = item_edges;
        let ranks = rank_items(items, edges, stack_folders);
        let last = max_columns - 1;
        let max_rank = ranks.values().map(|&r| r.min(last)).max().unwrap_or(0);
        let mut columns: Vec<Vec<Item>> = vec![Vec::new(); max_rank + 1];
        for &item in items {
            columns[ranks[&item].min(last)].push(item);
        }
        for column in &mut columns {
            sort_column(column, settings.sort);
        }
        order_by_barycenter(&mut columns, edges);
        break_ranks(columns, &item_size, v_gap, settings.rank_breaking)
    };
    let mut columns = arrange(false, usize::MAX);

    // Compact: a group with folders that came out much wider than tall, in 3 or more
    // columns, is arranged again, least change first, until it is not too
    // wide: (1) sibling folders stack in one column (an edge between two
    // folders keeps the target in the source's column, one from a folder to a
    // file ranks nothing); (2) the same, in at most 3 columns; (3) in at most
    // 2. Every edge is still drawn. The narrowest candidate wins.
    let wide = |columns: &[Vec<Item>]| -> f32 {
        let extent = columns_extent(columns, &item_size, settings, v_gap);
        extent.x / extent.y.max(1.0)
    };
    let count = |columns: &[Vec<Item>]| columns.iter().filter(|c| !c.is_empty()).count();
    let has_folders = items.iter().any(|it| matches!(it, Item::Folder(_)));
    if has_folders && count(&columns) >= 3 && wide(&columns) > MAX_GROUP_ASPECT {
        let mut best = (wide(&columns), None);
        for max_columns in [usize::MAX, 3, 2] {
            let candidate = arrange(true, max_columns);
            let aspect = wide(&candidate);
            if aspect < best.0 {
                best = (aspect, Some(candidate));
            }
            if aspect <= MAX_GROUP_ASPECT {
                break;
            }
        }
        if let Some(candidate) = best.1 {
            columns = candidate;
        }
    }

    let sizes: Vec<Vec<Vec2>> = columns
        .iter()
        .map(|col| col.iter().map(|it| item_size[it]).collect())
        .collect();
    let col_heights: Vec<f32> = sizes
        .iter()
        .map(|col| column_height(col, v_gap))
        .collect();
    let height = col_heights.iter().copied().fold(0.0, f32::max);
    let has_folder = |col: &[Item]| col.iter().any(|it| matches!(it, Item::Folder(_)));
    let h_share = match settings.h_align {
        HAlign::Left => 0.0,
        HAlign::Center => 0.5,
        HAlign::Right => 1.0,
    };

    let mut x = 0.0;
    for (r, column) in columns.iter().enumerate() {
        if column.is_empty() {
            continue;
        }
        let width = sizes[r].iter().map(|s| s.x).fold(0.0, f32::max);
        // Fill spreads a column's items over the full height; a lone item
        // is centered.
        let (mut y, gap_y) = match settings.v_align {
            VAlign::Center => ((height - col_heights[r]) * 0.5, v_gap),
            VAlign::Bottom => (height - col_heights[r], v_gap),
            VAlign::Top => (0.0, v_gap),
            VAlign::Fill if column.len() > 1 => {
                let used: f32 = sizes[r].iter().map(|s| s.y).sum();
                (0.0, (height - used) / (column.len() - 1) as f32)
            }
            VAlign::Fill => ((height - col_heights[r]) * 0.5, v_gap),
        };
        for (i, &item) in column.iter().enumerate() {
            let item_x = x + (width - sizes[r][i].x) * h_share;
            placement.offsets.insert(item, Vec2::new(item_x, y));
            y += sizes[r][i].y + gap_y;
        }
        let next_has_folder = columns.get(r + 1).is_some_and(|c| has_folder(c));
        let gap = if has_folder(column) || next_has_folder {
            settings.folder_spacing
        } else {
            settings.file_spacing
        };
        x += width + gap;
    }
    // The last column's trailing gap is not part of the content.
    let last_gap = columns
        .iter()
        .rev()
        .find(|c| !c.is_empty())
        .map(|c| if has_folder(c) { settings.folder_spacing } else { settings.file_spacing })
        .unwrap_or(0.0);
    placement.sizes.insert(group, Vec2::new((x - last_gap).max(0.0), height));
}

/// Size of the columns as `layout_group` places them: each column as wide as
/// its widest item, a gap between columns, the tallest column's height.
fn columns_extent(
    columns: &[Vec<Item>],
    item_size: &HashMap<Item, Vec2>,
    settings: &LayoutSettings,
    v_gap: f32,
) -> Vec2 {
    let has_folder = |col: &[Item]| col.iter().any(|it| matches!(it, Item::Folder(_)));
    let used: Vec<&Vec<Item>> = columns.iter().filter(|c| !c.is_empty()).collect();
    let mut width = 0.0;
    let mut height: f32 = 0.0;
    for (i, column) in used.iter().enumerate() {
        let sizes: Vec<Vec2> = column.iter().map(|it| item_size[it]).collect();
        width += sizes.iter().map(|s| s.x).fold(0.0, f32::max);
        height = height.max(column_height(&sizes, v_gap));
        if let Some(next) = used.get(i + 1) {
            width += if has_folder(column) || has_folder(next) {
                settings.folder_spacing
            } else {
                settings.file_spacing
            };
        }
    }
    Vec2::new(width, height)
}

fn column_height(sizes: &[Vec2], v_gap: f32) -> f32 {
    sizes.iter().map(|s| s.y).sum::<f32>() + sizes.len().saturating_sub(1) as f32 * v_gap
}

/// Rank Breaking: a column taller than `level` times its width is split into
/// `ceil(height / (level * width))` side-by-side columns of balanced height,
/// keeping its order. Lower levels split sooner; `0` (Off) keeps one column
/// per rank.
fn break_ranks(columns: Vec<Vec<Item>>, size: &HashMap<Item, Vec2>, v_gap: f32, level: u32) -> Vec<Vec<Item>> {
    if level == 0 {
        return columns;
    }
    let mut out = Vec::with_capacity(columns.len());
    for column in columns {
        let sizes: Vec<Vec2> = column.iter().map(|it| size[it]).collect();
        let width = sizes.iter().map(|s| s.x).fold(0.0, f32::max).max(1.0);
        let height = column_height(&sizes, v_gap);
        let limit = level as f32 * width;
        let parts = ((height / limit).ceil() as usize).clamp(1, column.len().max(1));
        if parts <= 1 {
            out.push(column);
            continue;
        }
        let target = height / parts as f32;
        let mut current: Vec<Item> = Vec::new();
        let mut current_h = 0.0;
        let mut left = parts;
        for (i, item) in column.iter().enumerate() {
            let h = sizes[i].y;
            let remaining_items = column.len() - i;
            if !current.is_empty() && left > 1 && (current_h + h > target || remaining_items < left) {
                out.push(std::mem::take(&mut current));
                current_h = 0.0;
                left -= 1;
            }
            current_h += h + v_gap;
            current.push(*item);
        }
        out.push(current);
    }
    out
}

/// Longest-path rank of each item after dropping the back edges a DFS finds
/// (visiting items in a fixed order), so cycles still spread into columns.
/// With `stack_folders`, an edge between two folders puts the target in the
/// source's column instead of the next one, and an edge from a folder to a
/// file does not rank the file at all, so what a folder merely uses stays
/// beside the files that use the folders (v2's compact look).
fn rank_items(items: &[Item], item_edges: &HashSet<(Item, Item)>, stack_folders: bool) -> HashMap<Item, usize> {
    let mut ordered: Vec<Item> = items.to_vec();
    ordered.sort();
    let mut out: HashMap<Item, Vec<Item>> = HashMap::new();
    for &(a, b) in item_edges {
        out.entry(a).or_default().push(b);
    }
    for targets in out.values_mut() {
        targets.sort();
        targets.dedup();
    }

    // Iterative DFS: 0 = unseen, 1 = on stack, 2 = done.
    let mut state: HashMap<Item, u8> = HashMap::new();
    let mut dag: Vec<(Item, Item)> = Vec::new();
    let mut finish: Vec<Item> = Vec::new();
    for &start in &ordered {
        if state.get(&start).copied().unwrap_or(0) != 0 {
            continue;
        }
        state.insert(start, 1);
        let mut stack: Vec<(Item, usize)> = vec![(start, 0)];
        while let Some(&mut (node, ref mut next)) = stack.last_mut() {
            let targets = out.get(&node).map(Vec::as_slice).unwrap_or(&[]);
            if let Some(&t) = targets.get(*next) {
                *next += 1;
                match state.get(&t).copied().unwrap_or(0) {
                    0 => {
                        dag.push((node, t));
                        state.insert(t, 1);
                        stack.push((t, 0));
                    }
                    1 => {} // back edge: dropped to break the cycle
                    _ => dag.push((node, t)),
                }
            } else {
                state.insert(node, 2);
                finish.push(node);
                stack.pop();
            }
        }
    }

    // Reverse finish order is a topological order of the remaining DAG.
    let mut preds: HashMap<Item, Vec<Item>> = HashMap::new();
    for &(a, b) in &dag {
        preds.entry(b).or_default().push(a);
    }
    let mut rank: HashMap<Item, usize> = HashMap::new();
    for &item in finish.iter().rev() {
        let step = |p: &Item| -> Option<usize> {
            match (p, item, stack_folders) {
                (Item::Folder(_), Item::Folder(_), true) => Some(0),
                (Item::Folder(_), Item::File(_), true) => None,
                _ => Some(1),
            }
        };
        let r = preds
            .get(&item)
            .map(|ps| ps.iter().filter_map(|p| Some(rank.get(p)? + step(p)?)).max().unwrap_or(0))
            .unwrap_or(0);
        rank.insert(item, r);
    }
    rank
}

fn sort_column(column: &mut [Item], mode: SortMode) {
    match mode {
        SortMode::Asc => column.sort_by_key(|it| it.id()),
        SortMode::Desc => column.sort_by_key(|it| std::cmp::Reverse(it.id())),
        SortMode::Alt => {
            column.sort_by_key(|it| it.id());
            interleave(column);
        }
        SortMode::AltRev => {
            column.sort_by_key(|it| std::cmp::Reverse(it.id()));
            interleave(column);
        }
        SortMode::Off => {}
    }
}

/// One left-to-right sweep: each column after the first is stably ordered by
/// the mean position of its items' neighbours in earlier columns. Items with
/// no such neighbour keep their own position as the key.
fn order_by_barycenter(columns: &mut [Vec<Item>], item_edges: &HashSet<(Item, Item)>) {
    let mut index: HashMap<Item, f32> = HashMap::new();
    if let Some(first) = columns.first() {
        for (i, &it) in first.iter().enumerate() {
            index.insert(it, i as f32);
        }
    }
    for column in columns.iter_mut().skip(1) {
        let keys: HashMap<Item, f32> = column
            .iter()
            .enumerate()
            .map(|(i, &it)| {
                let ns: Vec<f32> = item_edges
                    .iter()
                    .filter_map(|&(a, b)| {
                        if b == it { index.get(&a) } else if a == it { index.get(&b) } else { None }
                    })
                    .copied()
                    .collect();
                let key = if ns.is_empty() { i as f32 } else { ns.iter().sum::<f32>() / ns.len() as f32 };
                (it, key)
            })
            .collect();
        column.sort_by(|a, b| keys[a].total_cmp(&keys[b]));
        for (i, &it) in column.iter().enumerate() {
            index.insert(it, i as f32);
        }
    }
}

fn interleave<T: Copy>(items: &mut [T]) {
    if items.len() <= 2 {
        return;
    }
    let original = items.to_vec();
    let mid = original.len().div_ceil(2);
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
    use crate::edges::edge_store::{EdgeChange, EdgeKind};
    use std::path::{Path, PathBuf};

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

    /// A barrel hop `a -> index -> b` plus the shortcut `a -> b`: only the
    /// shortcut is a dependency. Counting the hop would push `b` to a third
    /// column, and a barrel that re-exports its importer would close a cycle.
    #[test]
    fn barrel_and_reexport_edges_do_not_move_nodes() {
        let (mut nodes, ids) = store(3);
        let mut direct = EdgeStore::default();
        direct.add(ids[0], ids[2], None);
        apply_layout(&mut nodes, &direct, &settings(SortMode::Asc, VAlign::Top));
        let expected: Vec<_> = ids.iter().map(|&id| pos(&nodes, id)).collect();

        let mut edges = EdgeStore::default();
        edges.add(ids[0], ids[2], None);
        for (source, target, kind) in
            [(ids[0], ids[1], EdgeKind::Barrel), (ids[1], ids[2], EdgeKind::ReExport)]
        {
            edges.add_full(source, target, None, None, None, None, None, None, kind);
        }
        apply_layout(&mut nodes, &edges, &settings(SortMode::Asc, VAlign::Top));

        let actual: Vec<_> = ids.iter().map(|&id| pos(&nodes, id)).collect();
        assert_eq!(actual, expected);
    }

    /// Comparing with a Git revision adds Deleted edges to the store; they
    /// describe the old tree and must not reflow the current one.
    #[test]
    fn deleted_edges_do_not_move_nodes() {
        let (mut nodes, ids) = store(3);
        let mut edges = EdgeStore::default();
        edges.add(ids[0], ids[1], None);
        apply_layout(&mut nodes, &edges, &settings(SortMode::Asc, VAlign::Top));
        let expected: Vec<_> = ids.iter().map(|&id| pos(&nodes, id)).collect();

        let removed = edges.add(ids[1], ids[2], None);
        edges.set_change(removed, EdgeChange::Deleted);
        apply_layout(&mut nodes, &edges, &settings(SortMode::Asc, VAlign::Top));

        let actual: Vec<_> = ids.iter().map(|&id| pos(&nodes, id)).collect();
        assert_eq!(actual, expected);
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
        assert_close(ys[1], height(&nodes, ids[0]) + SPACING * VERTICAL_GAP_RATIO);

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

    /// Files at `paths` (with their folders, as the workspace creates them).
    fn tree(paths: &[&str]) -> (NodeStore, Vec<u64>) {
        let mut nodes = NodeStore::default();
        let mut ids = Vec::new();
        for p in paths {
            let path = PathBuf::from(p);
            let dir = path.parent().unwrap().to_path_buf();
            if !nodes.folder_nodes.values().any(|f| f.path == dir) {
                nodes.add_folder(dir, 0);
            }
            ids.push(nodes.add_file(path, "x\ny\n".into(), UNPLACED));
        }
        (nodes, ids)
    }

    fn folder_bounds(nodes: &mut NodeStore, dir: &str) -> egui::Rect {
        nodes.update_folder_bounds(NODE_WIDTH, 12.0, LayoutSettings::default().folder_padding);
        nodes
            .folder_nodes
            .values()
            .find(|f| f.path == std::path::Path::new(dir))
            .unwrap()
            .bounds
    }

    fn file_rect(nodes: &NodeStore, id: u64) -> egui::Rect {
        nodes.get_file(id).unwrap().world_rect(NODE_WIDTH, 12.0)
    }

    #[test]
    fn sibling_folders_never_overlap() {
        let (mut nodes, ids) = tree(&["r/a/1.rs", "r/a/2.rs", "r/b/1.rs", "r/b/2.rs", "r/b/3.rs"]);
        let mut edges = EdgeStore::default();
        edges.add(ids[0], ids[1], None);
        edges.add(ids[2], ids[3], None);
        apply_layout(&mut nodes, &edges, &LayoutSettings::default());

        let a = folder_bounds(&mut nodes, "r/a");
        let b = folder_bounds(&mut nodes, "r/b");
        assert!(a.is_positive() && b.is_positive());
        assert!(!a.intersects(b), "{a:?} overlaps {b:?}");
    }

    #[test]
    fn nested_folder_sits_inside_its_parent() {
        let (mut nodes, ids) = tree(&["r/src/index.rs", "r/src/utils/kind.rs", "r/src/utils/warn.rs"]);
        let mut edges = EdgeStore::default();
        edges.add(ids[0], ids[1], None);
        apply_layout(&mut nodes, &edges, &LayoutSettings::default());

        let parent = folder_bounds(&mut nodes, "r/src");
        let child = folder_bounds(&mut nodes, "r/src/utils");
        assert!(parent.contains_rect(child), "{child:?} not inside {parent:?}");
        for &id in &ids[1..] {
            assert!(child.contains_rect(file_rect(&nodes, id)));
        }
        assert!(!child.intersects(file_rect(&nodes, ids[0])), "index.rs stays outside utils/");
    }

    #[test]
    fn importer_is_left_of_its_dependency_across_folders() {
        let (mut nodes, ids) = tree(&["r/app/main.rs", "r/lib/dep.rs"]);
        let mut edges = EdgeStore::default();
        edges.add(ids[0], ids[1], None);
        apply_layout(&mut nodes, &edges, &LayoutSettings::default());

        let app = folder_bounds(&mut nodes, "r/app");
        let lib = folder_bounds(&mut nodes, "r/lib");
        assert!(app.max.x < lib.min.x, "app/ {app:?} must be left of lib/ {lib:?}");
        assert!(pos(&nodes, ids[0]).x < pos(&nodes, ids[1]).x);
    }

    #[test]
    fn a_cycle_still_spreads_into_columns() {
        let (mut nodes, ids) = store(2);
        let mut edges = EdgeStore::default();
        edges.add(ids[0], ids[1], None);
        edges.add(ids[1], ids[0], None);
        apply_layout(&mut nodes, &edges, &settings(SortMode::Asc, VAlign::Top));

        assert_ne!(pos(&nodes, ids[0]).x, pos(&nodes, ids[1]).x);
    }

    #[test]
    fn barycenter_keeps_a_dependency_level_with_its_importer() {
        // a, b in column 0 (Asc); b -> d and a -> c. Asc alone would put c
        // above d only if c < d; make it the other way and check the sweep.
        let (mut nodes, ids) = store(4);
        let mut edges = EdgeStore::default();
        edges.add(ids[0], ids[3], None); // a -> d
        edges.add(ids[1], ids[2], None); // b -> c
        apply_layout(&mut nodes, &edges, &settings(SortMode::Asc, VAlign::Top));

        assert!(pos(&nodes, ids[3]).y < pos(&nodes, ids[2]).y, "d follows a, c follows b");
    }

    /// `n` files of `lines` lines each, all starting at `UNPLACED`.
    fn tall_store(n: usize, lines: usize) -> (NodeStore, Vec<u64>) {
        let mut nodes = NodeStore::default();
        let ids = (0..n)
            .map(|i| nodes.add_file(PathBuf::from(format!("t{i}.rs")), "x\n".repeat(lines), UNPLACED))
            .collect();
        (nodes, ids)
    }

    #[test]
    fn rank_breaking_off_keeps_one_column_and_low_levels_split_it() {
        let (mut nodes, ids) = tall_store(4, 200);
        let mut s = settings(SortMode::Asc, VAlign::Top);
        s.rank_breaking = 0;
        apply_layout(&mut nodes, &EdgeStore::default(), &s);
        assert!(ids.iter().all(|&id| pos(&nodes, id).x == 0.0), "Off: one column");

        s.rank_breaking = 1;
        apply_layout(&mut nodes, &EdgeStore::default(), &s);
        let mut xs: Vec<f32> = ids.iter().map(|&id| pos(&nodes, id).x).collect();
        xs.dedup();
        assert_eq!(xs, vec![0.0, COLUMN, 2.0 * COLUMN, 3.0 * COLUMN], "each tall file gets its own sub-column, in order");
        assert!(ids.iter().all(|&id| pos(&nodes, id).y == 0.0));

        // A level high enough for the whole column keeps it whole.
        let total: f32 = ids.iter().map(|&id| height(&nodes, id)).sum();
        s.rank_breaking = 8;
        if total + 3.0 * SPACING * VERTICAL_GAP_RATIO <= 8.0 * NODE_WIDTH {
            apply_layout(&mut nodes, &EdgeStore::default(), &s);
            assert!(ids.iter().all(|&id| pos(&nodes, id).x == 0.0));
        }
    }

    #[test]
    fn split_columns_keep_balanced_order() {
        let level = 2;
        let (mut nodes, ids) = tall_store(6, 120);
        let mut s = settings(SortMode::Asc, VAlign::Top);
        s.rank_breaking = level;
        apply_layout(&mut nodes, &EdgeStore::default(), &s);
        let xs: Vec<f32> = ids.iter().map(|&id| pos(&nodes, id).x).collect();
        assert!(xs.windows(2).all(|w| w[0] <= w[1]), "order kept across sub-columns: {xs:?}");
        assert!(xs.last().unwrap() > &0.0, "the column was split: {xs:?}");
        for &id in &ids {
            let column: Vec<u64> = ids.iter().copied().filter(|&o| pos(&nodes, o).x == pos(&nodes, id).x).collect();
            let h: f32 = column.iter().map(|&o| height(&nodes, o)).sum::<f32>()
                + (column.len() - 1) as f32 * SPACING * VERTICAL_GAP_RATIO;
            assert!(h <= level as f32 * NODE_WIDTH + height(&nodes, id), "sub-column of {id} too tall: {h}");
        }
    }

    #[test]
    fn fill_spreads_a_short_column_over_the_tallest_one() {
        // a -> b, a -> c, a -> d: column 1 holds three files, column 0 one tall file.
        let mut nodes = NodeStore::default();
        let a = nodes.add_file(PathBuf::from("a.rs"), "x\n".repeat(200), UNPLACED);
        let small: Vec<u64> = (0..3)
            .map(|i| nodes.add_file(PathBuf::from(format!("s{i}.rs")), "x\n".into(), UNPLACED))
            .collect();
        let mut edges = EdgeStore::default();
        for &b in &small {
            edges.add(a, b, None);
        }
        let mut s = settings(SortMode::Asc, VAlign::Fill);
        s.rank_breaking = 0;
        apply_layout(&mut nodes, &edges, &s);
        let tall = height(&nodes, a);
        let first = pos(&nodes, small[0]).y;
        let last = small[2];
        assert_close(first, 0.0);
        assert_close(pos(&nodes, last).y + height(&nodes, last), tall);
        let middle = pos(&nodes, small[1]).y + height(&nodes, small[1]) / 2.0;
        assert_close(middle, tall / 2.0);
    }

    #[test]
    fn h_align_places_narrow_items_inside_their_column() {
        // Column 0 holds a folder (wider than a file) and a file.
        let mut nodes = NodeStore::default();
        let root = PathBuf::from("/r");
        let folder = nodes.add_folder(root.join("pkg"), 0);
        let inner = nodes.add_file(root.join("pkg/inner.rs"), "x\n".into(), UNPLACED);
        nodes.get_folder_mut(folder).unwrap().children_ids.push(inner);
        let top = nodes.add_file(root.join("top.rs"), "x\n".into(), UNPLACED);
        let folder_width = NODE_WIDTH + 2.0 * LayoutSettings::default().folder_padding;
        for (align, share) in [(HAlign::Left, 0.0), (HAlign::Center, 0.5), (HAlign::Right, 1.0)] {
            let mut s = settings(SortMode::Asc, VAlign::Top);
            s.h_align = align;
            apply_layout(&mut nodes, &EdgeStore::default(), &s);
            assert_close(pos(&nodes, top).x, (folder_width - NODE_WIDTH) * share);
        }
    }

    #[test]
    fn a_collapsed_folder_is_laid_out_as_one_compact_box() {
        let (mut nodes, ids) = tree(&["/r/pkg/a.rs", "/r/pkg/b.rs", "/r/top.rs"]);
        let pkg = nodes.folder_containing(Path::new("/r/pkg")).unwrap();
        nodes.get_folder_mut(pkg).unwrap().collapsed = true;
        let before: Vec<egui::Pos2> = ids[..2].iter().map(|&id| pos(&nodes, id)).collect();
        let mut edges = EdgeStore::default();
        edges.add(ids[2], ids[0], None);
        apply_layout(&mut nodes, &edges, &settings(SortMode::Asc, VAlign::Top));

        let after: Vec<egui::Pos2> = ids[..2].iter().map(|&id| pos(&nodes, id)).collect();
        assert_eq!(before, after, "hidden files keep their place");
        let origin = nodes.get_folder(pkg).unwrap().collapsed_origin.expect("placed");
        assert!(pos(&nodes, ids[2]).x < origin.x, "top.rs imports into pkg: it sits left of the box");
        assert_close(origin.x - pos(&nodes, ids[2]).x, NODE_WIDTH + LayoutSettings::default().folder_spacing);
    }

    #[test]
    fn a_too_wide_group_stacks_sibling_folders_that_only_use_each_other() {
        let (mut nodes, ids) = tree(&["/r/main.rs", "/r/a/x.rs", "/r/b/y.rs", "/r/c/z.rs"]);
        let mut edges = EdgeStore::default();
        edges.add(ids[0], ids[1], None); // main -> a/
        edges.add(ids[1], ids[2], None); // a/ -> b/
        edges.add(ids[2], ids[3], None); // b/ -> c/
        apply_layout(&mut nodes, &edges, &LayoutSettings::default());

        let a = folder_bounds(&mut nodes, "/r/a");
        let b = folder_bounds(&mut nodes, "/r/b");
        let c = folder_bounds(&mut nodes, "/r/c");
        assert_close(a.min.x, b.min.x);
        assert_close(a.min.x, c.min.x);
        assert!(!a.intersects(b) && !b.intersects(c), "stacked, not overlapping");
        assert!(pos(&nodes, ids[0]).x < a.min.x, "main.rs still sits left of the folders it uses");
    }

    #[test]
    fn a_too_wide_group_keeps_what_folders_use_beside_the_files_that_use_folders() {
        let (mut nodes, ids) = tree(&["/r/main.rs", "/r/net/c.rs", "/r/dom/d.rs", "/r/util.rs"]);
        let mut edges = EdgeStore::default();
        edges.add(ids[0], ids[1], None); // main -> net/
        edges.add(ids[0], ids[2], None); // main -> dom/
        edges.add(ids[1], ids[3], None); // net/ -> util.rs
        edges.add(ids[1], ids[2], None); // net/ -> dom/
        edges.add(ids[3], ids[2], None); // util.rs -> dom/
        apply_layout(&mut nodes, &edges, &LayoutSettings::default());

        let net = folder_bounds(&mut nodes, "/r/net");
        let dom = folder_bounds(&mut nodes, "/r/dom");
        assert_close(net.min.x, dom.min.x);
        assert_close(pos(&nodes, ids[0]).x, pos(&nodes, ids[3]).x);
        assert!(pos(&nodes, ids[3]).x < net.min.x, "util.rs left of the folder column");
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
