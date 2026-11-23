//! Folder-aware hierarchical layout, after v2 Code Canvas.
//!
//! Folders are laid out as nested, non-overlapping clusters. Inside every
//! folder (and at the top level) its items, files and nested folders, are
//! placed in columns by dependency rank: an item that uses another sits to
//! its left. Cycles are broken by dropping DFS back edges, so every member
//! of a cycle still gets its own column.
//!
//! Items that only use each other, or nothing at all, are islands: they are
//! ranked on their own and sit in columns after the ones that connect to the
//! rest of the project. Within a column items are ordered by how connected
//! they are (`in*4 + out*2 + in_siblings*3 + out_siblings`), and, when the
//! Barycenter setting is on, by where their neighbours sit.
//!
//! Rank Breaking splits the tallest column of a folder in two, where no edge
//! crosses the cut, until the folder is no taller than the level times the
//! columns' width; H Align places narrower items inside their column, and V
//! Align places each column (Fill spreads its items over the tallest
//! column's height).

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

/// How many times Rank Breaking cuts a folder's columns at most.
const MAX_RANK_BREAKS: usize = 10;

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

/// How connected an item is, in file-to-file edges (each pair of files once,
/// edges inside the item not counted). The `*_siblings` ones only count the
/// edges whose other end is a sibling in the same group.
#[derive(Debug, Default, Clone, Copy)]
struct Degrees {
    incoming: u32,
    outgoing: u32,
    incoming_siblings: u32,
    outgoing_siblings: u32,
}

impl Degrees {
    /// The key a column is ordered by.
    fn score(self) -> u32 {
        self.incoming * 4 + self.outgoing * 2 + self.incoming_siblings * 3 + self.outgoing_siblings
    }
}

/// What laying out one group reads besides its own items and edges.
#[derive(Clone, Copy)]
struct Context<'a> {
    nodes: &'a NodeStore,
    settings: &'a LayoutSettings,
    /// Revealed files of each collapsed card.
    revealed: &'a HashMap<u64, Vec<u64>>,
    connectivity: &'a Connectivity,
}

/// What the edges say about the items of one folder tree.
#[derive(Default)]
struct Connectivity {
    degrees: HashMap<Item, Degrees>,
    /// Items with an edge to something outside their group.
    open: HashSet<Item>,
}

struct Placement {
    /// Item positions relative to their group's content origin.
    offsets: HashMap<Item, Vec2>,
    /// Laid-out content size of each group.
    sizes: HashMap<Group, Vec2>,
}

/// Whether an edge takes part in the layout. An import through a barrel is
/// two links: the hop to the barrel (`Barrel`) and the shortcut to the file
/// that defines the name (`ReExport`). The shortcut is the real dependency;
/// counting the hop would push the provider one column further and, for a
/// barrel that re-exports its importer, close a cycle. Edges that exist only in
/// a Git baseline are not dependencies of the current tree, so comparing
/// against a revision must not move nodes (decisions 108 and 119).
fn feeds_layout(edge: &Edge) -> bool {
    edge.kind != EdgeKind::Barrel && edge.change != EdgeChange::Deleted
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
    let mut connectivity = Connectivity::default();
    let mut counted: HashSet<(u64, u64)> = HashSet::new();
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
        // Degrees: every item from the one where the chains part down to the
        // file has this edge once. Below the meeting group the other end is
        // outside the item's own group, which leaves it open.
        if counted.insert((edge.source_node, edge.target_node)) {
            for (j, &item) in cs.iter().enumerate().skip(k) {
                let degrees = connectivity.degrees.entry(item).or_default();
                degrees.outgoing += 1;
                if j == k {
                    degrees.outgoing_siblings += 1;
                } else {
                    connectivity.open.insert(item);
                }
            }
            for (j, &item) in ct.iter().enumerate().skip(k) {
                let degrees = connectivity.degrees.entry(item).or_default();
                degrees.incoming += 1;
                if j == k {
                    degrees.incoming_siblings += 1;
                } else {
                    connectivity.open.insert(item);
                }
            }
        }
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
        layout_group(group, items, item_edges, Context { nodes, settings, revealed: &revealed, connectivity: &connectivity }, &mut placement);
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
    cx: Context<'_>,
    placement: &mut Placement,
) {
    let Context { nodes, settings, revealed, connectivity } = cx;
    if items.is_empty() {
        placement.sizes.insert(group, Vec2::ZERO);
        return;
    }
    // The order the items were collected in depends on hash maps: sort them,
    // so the same project always gets the same layout.
    let mut items = items.to_vec();
    items.sort();
    let items = items.as_slice();
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
    let islands = islands(items, item_edges, &connectivity.open);
    let arrange = |stack_folders: bool, max_columns: usize| -> Vec<Vec<Item>> {
        let edges = item_edges;
        let ranks = rank_items(items, edges, stack_folders);
        let last = max_columns - 1;
        // Islands come after the columns of what connects to the project.
        let first_island = items
            .iter()
            .filter(|it| !islands.contains(it))
            .map(|it| ranks[it].min(last) + 1)
            .max()
            .unwrap_or(0);
        let column_of = |item: Item| -> usize {
            let rank = ranks[&item];
            if islands.contains(&item) { (first_island + rank).min(last) } else { rank.min(last) }
        };
        let max_rank = items.iter().map(|&it| column_of(it)).max().unwrap_or(0);
        let mut columns: Vec<Vec<Item>> = vec![Vec::new(); max_rank + 1];
        for &item in items {
            columns[column_of(item)].push(item);
        }
        for column in &mut columns {
            sort_column(column, settings.sort, |it| connectivity.degrees.get(&it).copied().unwrap_or_default().score());
        }
        if settings.barycenter {
            order_by_barycenter(&mut columns, edges);
        }
        break_ranks(columns, &item_size, v_gap, settings.rank_breaking, edges)
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

/// Rank Breaking. While the tallest column of a group is more than `level`
/// times as tall as all the columns are wide together (at most
/// [`MAX_RANK_BREAKS`] times), the tallest column that has a legal cut is cut
/// in two, near the middle of its height, and the second half becomes the
/// column next to it. A cut is legal when no edge between two items of the
/// column crosses it. `0` (Off) keeps one column per rank.
fn break_ranks(
    mut columns: Vec<Vec<Item>>,
    size: &HashMap<Item, Vec2>,
    v_gap: f32,
    level: u32,
    item_edges: &HashSet<(Item, Item)>,
) -> Vec<Vec<Item>> {
    if level == 0 {
        return columns;
    }
    let heights = |columns: &[Vec<Item>]| -> Vec<f32> {
        columns.iter().map(|c| column_height(&c.iter().map(|it| size[it]).collect::<Vec<_>>(), v_gap)).collect()
    };
    for _ in 0..MAX_RANK_BREAKS {
        let used = columns.iter().filter(|c| !c.is_empty()).count();
        let width = columns
            .iter()
            .flatten()
            .map(|it| size[it].x)
            .fold(0.0, f32::max)
            .max(1.0);
        let column_heights = heights(&columns);
        let tallest = column_heights.iter().copied().fold(0.0, f32::max);
        if tallest <= level as f32 * used as f32 * width {
            break;
        }
        let mut by_height: Vec<usize> = (0..columns.len()).filter(|&c| columns[c].len() >= 2).collect();
        by_height.sort_by(|&a, &b| column_heights[b].total_cmp(&column_heights[a]).then(a.cmp(&b)));
        let cut = by_height
            .into_iter()
            .find_map(|c| middle_cut(&columns[c], size, v_gap, item_edges).map(|at| (c, at)));
        let Some((c, at)) = cut else { break };
        let tail = columns[c].split_off(at);
        columns.insert(c + 1, tail);
    }
    columns
}

/// Where to cut `column` in two: the number of items that stay in the first
/// half, the one nearest to half of the column's height such that no edge of
/// `item_edges` between two items of the column crosses it. `None` when every
/// cut would separate the two ends of an edge.
fn middle_cut(column: &[Item], size: &HashMap<Item, Vec2>, v_gap: f32, item_edges: &HashSet<(Item, Item)>) -> Option<usize> {
    let position: HashMap<Item, usize> = column.iter().enumerate().map(|(i, &it)| (it, i)).collect();
    // `crossing[i]` counts the edges that a cut after `i` items would split.
    let mut crossing = vec![0i32; column.len() + 1];
    for &(a, b) in item_edges {
        if let (Some(&p), Some(&q)) = (position.get(&a), position.get(&b)) {
            let (low, high) = (p.min(q), p.max(q));
            crossing[low + 1] += 1;
            crossing[high + 1] -= 1;
        }
    }
    let total = column_height(&column.iter().map(|it| size[it]).collect::<Vec<_>>(), v_gap);
    let mut open = 0;
    let mut above = 0.0;
    let mut best: Option<(f32, usize)> = None;
    for (i, item) in column.iter().enumerate().take(column.len() - 1) {
        above += size[item].y + if i > 0 { v_gap } else { 0.0 };
        open += crossing[i + 1];
        if open == 0 {
            let distance = (above - total / 2.0).abs();
            if best.is_none_or(|(d, _)| distance < d) {
                best = Some((distance, i + 1));
            }
        }
    }
    best.map(|(_, at)| at)
}

/// The items that only use each other: every connected component of the
/// group's items (by its edges, in any direction) in which no item has an
/// edge to something outside the group. An item with no edge at all is one.
fn islands(items: &[Item], item_edges: &HashSet<(Item, Item)>, open: &HashSet<Item>) -> HashSet<Item> {
    let index: HashMap<Item, usize> = items.iter().enumerate().map(|(i, &it)| (it, i)).collect();
    let mut parent: Vec<usize> = (0..items.len()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for &(a, b) in item_edges {
        if let (Some(&a), Some(&b)) = (index.get(&a), index.get(&b)) {
            let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
            parent[ra] = rb;
        }
    }
    let mut connected: HashSet<usize> = HashSet::new();
    for (i, item) in items.iter().enumerate() {
        if open.contains(item) {
            connected.insert(root(&mut parent, i));
        }
    }
    items
        .iter()
        .enumerate()
        .filter(|&(i, _)| !connected.contains(&root(&mut parent, i)))
        .map(|(_, &item)| item)
        .collect()
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

/// Orders a column by `score` (how connected an item is), and by id among
/// equal ones. Desc puts the most connected on top; Alt and Alt Rev deal the
/// sorted items out alternately; Off keeps the order the items came in.
fn sort_column(column: &mut [Item], mode: SortMode, score: impl Fn(Item) -> u32) {
    let key = |it: &Item| (score(*it), it.id());
    match mode {
        SortMode::Asc => column.sort_by_key(key),
        SortMode::Desc => column.sort_by_key(|it| std::cmp::Reverse(key(it))),
        SortMode::Alt => {
            column.sort_by_key(key);
            interleave(column);
        }
        SortMode::AltRev => {
            column.sort_by_key(|it| std::cmp::Reverse(key(it)));
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

    /// An import through a barrel: the hop `a -> index` (Barrel), the
    /// barrel's own export `index -> b` and the shortcut `a -> b` (ReExport).
    /// The hop must not rank `index` after `a`, nor push `b` a column further.
    #[test]
    fn the_barrel_hop_does_not_move_nodes_but_the_shortcut_counts() {
        let (mut nodes, ids) = store(3);
        let (importer, barrel, provider) = (ids[0], ids[1], ids[2]);
        let mut real = EdgeStore::default();
        real.add(barrel, provider, None);
        real.add_full(importer, provider, None, None, None, None, None, None, EdgeKind::ReExport);
        apply_layout(&mut nodes, &real, &settings(SortMode::Asc, VAlign::Top));
        let expected: Vec<_> = ids.iter().map(|&id| pos(&nodes, id)).collect();
        assert_eq!(pos(&nodes, importer).x, pos(&nodes, barrel).x, "both import the provider");
        assert!(pos(&nodes, provider).x > pos(&nodes, importer).x, "the shortcut ranks the provider after its importer");

        let mut with_hop = EdgeStore::default();
        with_hop.add(barrel, provider, None);
        with_hop.add_full(importer, provider, None, None, None, None, None, None, EdgeKind::ReExport);
        with_hop.add_full(importer, barrel, None, None, None, None, None, None, EdgeKind::Barrel);
        apply_layout(&mut nodes, &with_hop, &settings(SortMode::Asc, VAlign::Top));

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
        let mut s = settings(SortMode::Asc, VAlign::Top);
        // By score alone c and d tie and c comes first; the sweep follows the importers.
        apply_layout(&mut nodes, &edges, &s);
        assert!(pos(&nodes, ids[2]).y < pos(&nodes, ids[3]).y, "off: the order of equal scores is by id");

        s.barycenter = true;
        apply_layout(&mut nodes, &edges, &s);
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

    /// The columns of `ids` as their distinct x positions, left to right.
    fn columns_of(nodes: &NodeStore, ids: &[u64]) -> Vec<f32> {
        let mut xs: Vec<f32> = ids.iter().map(|&id| pos(nodes, id).x).collect();
        xs.sort_by(f32::total_cmp);
        xs.dedup();
        xs
    }

    /// Tallest column over the width of all the columns together, as Rank Breaking measures it.
    fn aspect_of(nodes: &NodeStore, ids: &[u64]) -> f32 {
        let xs = columns_of(nodes, ids);
        let tallest = xs
            .iter()
            .map(|&x| {
                let column: Vec<u64> = ids.iter().copied().filter(|&id| pos(nodes, id).x == x).collect();
                column.iter().map(|&id| height(nodes, id)).sum::<f32>() + (column.len() - 1) as f32 * SPACING * VERTICAL_GAP_RATIO
            })
            .fold(0.0, f32::max);
        tallest / (xs.len() as f32 * NODE_WIDTH)
    }

    #[test]
    fn rank_breaking_cuts_a_group_only_until_it_is_square_enough() {
        let (mut nodes, ids) = tall_store(6, 120);
        let mut s = settings(SortMode::Asc, VAlign::Top);
        for level in [1u32, 2, 4] {
            s.rank_breaking = level;
            apply_layout(&mut nodes, &EdgeStore::default(), &s);
            let xs: Vec<f32> = ids.iter().map(|&id| pos(&nodes, id).x).collect();
            assert!(xs.windows(2).all(|w| w[0] <= w[1]), "level {level}: order kept across the columns: {xs:?}");
            let aspect = aspect_of(&nodes, &ids);
            let columns = columns_of(&nodes, &ids).len();
            assert!(columns > 1, "level {level}: the column was cut");
            assert!(aspect <= level as f32 || columns == 6, "level {level}: {columns} columns, aspect {aspect}");
        }
    }

    #[test]
    fn rank_breaking_cuts_near_the_middle_and_at_most_ten_times() {
        let (mut nodes, ids) = tall_store(4, 120);
        let mut s = settings(SortMode::Asc, VAlign::Top);
        s.rank_breaking = 4;
        apply_layout(&mut nodes, &EdgeStore::default(), &s);
        let xs = columns_of(&nodes, &ids);
        assert_eq!(xs.len(), 2, "one cut was enough: {xs:?}");
        let counts: Vec<usize> = xs.iter().map(|&x| ids.iter().filter(|&&id| pos(&nodes, id).x == x).count()).collect();
        assert_eq!(counts, vec![2, 2], "the cut is in the middle of four equal files");

        let (mut nodes, ids) = tall_store(40, 200);
        s.rank_breaking = 1;
        apply_layout(&mut nodes, &EdgeStore::default(), &s);
        assert_eq!(columns_of(&nodes, &ids).len(), 11, "ten cuts of one column make eleven");
    }

    #[test]
    fn a_cut_never_separates_the_two_ends_of_an_edge() {
        let item = |i: u64| Item::File(i);
        let column: Vec<Item> = (0..4).map(item).collect();
        let size: HashMap<Item, Vec2> = column.iter().map(|&it| (it, Vec2::new(10.0, 100.0))).collect();
        let none = HashSet::new();
        assert_eq!(middle_cut(&column, &size, 0.0, &none), Some(2), "the middle");
        // 1 and 2 are linked: a cut after two items would split them.
        let linked: HashSet<(Item, Item)> = [(item(1), item(2))].into();
        assert_eq!(middle_cut(&column, &size, 0.0, &linked), Some(1), "the nearest legal cut; 1 and 3 tie and the first wins");
        // A chain: every cut splits an edge.
        let chain: HashSet<(Item, Item)> = [(item(0), item(1)), (item(1), item(2)), (item(2), item(3))].into();
        assert_eq!(middle_cut(&column, &size, 0.0, &chain), None);
        // An edge to a file in another column does not matter.
        let outside: HashSet<(Item, Item)> = [(item(1), item(9))].into();
        assert_eq!(middle_cut(&column, &size, 0.0, &outside), Some(2));
    }

    #[test]
    fn files_that_connect_to_nothing_leave_the_first_column_to_the_ones_that_do() {
        // lib/a -> lib/b is used from outside (app/main -> lib/a); lib/lone has no edge.
        let (mut nodes, ids) = tree(&["r/app/main.rs", "r/lib/a.rs", "r/lib/b.rs", "r/lib/lone.rs"]);
        let mut edges = EdgeStore::default();
        edges.add(ids[0], ids[1], None);
        edges.add(ids[1], ids[2], None);
        apply_layout(&mut nodes, &edges, &LayoutSettings::default());

        let (a, b, lone) = (pos(&nodes, ids[1]).x, pos(&nodes, ids[2]).x, pos(&nodes, ids[3]).x);
        assert!(a < b, "a uses b");
        assert!(lone > b, "the loose file comes after every column that is linked to the rest: {a} {b} {lone}");
    }

    #[test]
    fn a_group_that_only_uses_itself_is_an_island_and_one_that_reaches_out_is_not() {
        // lib: a -> b (island: nothing outside touches them), c -> d, and d is used from outside.
        let (mut nodes, ids) = tree(&["r/app/main.rs", "r/lib/a.rs", "r/lib/b.rs", "r/lib/c.rs", "r/lib/d.rs"]);
        let mut edges = EdgeStore::default();
        edges.add(ids[1], ids[2], None);
        edges.add(ids[3], ids[4], None);
        edges.add(ids[0], ids[4], None);
        apply_layout(&mut nodes, &edges, &LayoutSettings::default());

        let x = |i: usize| pos(&nodes, ids[i]).x;
        assert!(x(3) < x(4), "c uses d");
        assert!(x(1) > x(4), "the island a -> b follows the linked columns: {} vs {}", x(1), x(4));
        assert!(x(1) < x(2), "inside the island a still comes before b");
    }

    #[test]
    fn at_the_top_level_everything_is_laid_out_as_before() {
        // Two separate pairs and a loose file, none of them "open": islands side by side by depth.
        let (mut nodes, ids) = store(5);
        let mut edges = EdgeStore::default();
        edges.add(ids[0], ids[1], None);
        edges.add(ids[2], ids[3], None);
        apply_layout(&mut nodes, &edges, &settings(SortMode::Asc, VAlign::Top));
        let xs: Vec<f32> = ids.iter().map(|&id| pos(&nodes, id).x).collect();
        assert_eq!(xs, vec![0.0, COLUMN, 0.0, COLUMN, 0.0], "depth decides the column, not the component");
    }

    #[test]
    fn a_column_is_ordered_by_how_connected_its_files_are() {
        // p, q -> x and r -> y: x and y share a column; x is the more connected one.
        let (mut nodes, ids) = store(5);
        let (p, q, r, x, y) = (ids[0], ids[1], ids[2], ids[3], ids[4]);
        let mut edges = EdgeStore::default();
        edges.add(p, x, None);
        edges.add(q, x, None);
        edges.add(r, y, None);
        apply_layout(&mut nodes, &edges, &settings(SortMode::Desc, VAlign::Top));
        assert_eq!(pos(&nodes, x).x, pos(&nodes, y).x);
        assert!(pos(&nodes, x).y < pos(&nodes, y).y, "desc: the more connected file is on top");
        apply_layout(&mut nodes, &edges, &settings(SortMode::Asc, VAlign::Top));
        assert!(pos(&nodes, y).y < pos(&nodes, x).y, "asc: the less connected file is on top");
    }

    #[test]
    fn the_score_weighs_edges_to_siblings_and_to_the_outside_differently() {
        let degrees = |incoming, outgoing, incoming_siblings, outgoing_siblings| {
            Degrees { incoming, outgoing, incoming_siblings, outgoing_siblings }.score()
        };
        assert_eq!(degrees(1, 0, 0, 0), 4);
        assert_eq!(degrees(0, 1, 0, 0), 2);
        assert_eq!(degrees(0, 0, 1, 0), 3);
        assert_eq!(degrees(0, 0, 0, 1), 1);
        // An edge between siblings counts in both of its ends' totals and in its sibling counters.
        assert_eq!(degrees(1, 0, 1, 0), 7);
    }

    #[test]
    fn the_same_project_always_gets_the_same_layout() {
        let paths: Vec<String> = (0..24).map(|i| format!("r/d{}/f{i}.rs", i % 4)).collect();
        let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
        let mut seen: Option<Vec<egui::Pos2>> = None;
        for sort in [SortMode::Off, SortMode::Desc, SortMode::Alt] {
            seen = None;
            for _ in 0..6 {
                let (mut nodes, ids) = tree(&refs);
                let mut edges = EdgeStore::default();
                let mut again = Xorshift(11);
                for _ in 0..40 {
                    let (a, b) = (again.next(ids.len()), again.next(ids.len()));
                    if a != b {
                        edges.add(ids[a], ids[b], None);
                    }
                }
                apply_layout(&mut nodes, &edges, &settings(sort, VAlign::Center));
                let now: Vec<egui::Pos2> = ids.iter().map(|&id| pos(&nodes, id)).collect();
                if let Some(before) = &seen {
                    assert_eq!(*before, now, "{sort:?}: two runs on the same project differ");
                }
                seen = Some(now);
            }
        }
        assert!(seen.is_some());
    }

    /// A tiny deterministic generator for the invariants below.
    struct Xorshift(u64);

    impl Xorshift {
        fn next(&mut self, below: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % below as u64) as usize
        }
    }

    #[test]
    fn random_projects_never_overlap_and_dependencies_point_right() {
        for seed in 1..=20u64 {
            let mut random = Xorshift(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let count = 6 + random.next(24);
            let paths: Vec<String> = (0..count).map(|i| format!("r/{}/{}/f{i}.rs", random.next(3), random.next(2))).collect();
            let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
            let (mut nodes, ids) = tree(&refs);
            let mut edges = EdgeStore::default();
            let mut pairs = Vec::new();
            for _ in 0..random.next(40) {
                // Edges only run from a lower to a higher index: no cycles.
                let (a, b) = (random.next(count), random.next(count));
                if a < b {
                    edges.add(ids[a], ids[b], None);
                    pairs.push((ids[a], ids[b]));
                }
            }
            let s = LayoutSettings { barycenter: random.next(2) == 1, rank_breaking: random.next(9) as u32, ..Default::default() };
            apply_layout(&mut nodes, &edges, &s);

            for (i, &a) in ids.iter().enumerate() {
                for &b in &ids[i + 1..] {
                    assert!(!file_rect(&nodes, a).intersects(file_rect(&nodes, b)), "seed {seed}: {a} and {b} overlap");
                }
            }
            // Files in the same folder: the importer is left of what it imports.
            let folder = |id: u64| nodes.file_folder(id);
            for (a, b) in pairs {
                if folder(a) == folder(b) {
                    assert!(pos(&nodes, a).x < pos(&nodes, b).x, "seed {seed}: {a} -> {b} points left");
                }
            }
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
