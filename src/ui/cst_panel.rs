//! Right-rail "CST" panel: the tree-sitter syntax tree of the primary
//! selected file node, filtered by the CST settings (depth, hidden types,
//! semantic nodes). Hovering a row highlights its lines on the canvas, and
//! the syntax nodes at the ends of the selection's edges are outlined
//! (painted by the app shell through [`highlight_rect`]).

use std::collections::{hash_map::DefaultHasher, BTreeMap};
use std::hash::{Hash, Hasher};

use crate::analysis::cst::{parse_cst, CstNode, CstTree};
use crate::nodes::node_store::NodeStore;
use crate::state::{AppState, CstSettings};
use crate::ui::icons;
use crate::ui::panel::{switch, Section};

const ERROR_COLOR: egui::Color32 = egui::Color32::from_rgb(0xef, 0x44, 0x44);

/// The CST row under the pointer in the current frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CstHover {
    pub node_id: u64,
    /// 0-based, inclusive.
    pub start_line: usize,
    pub end_line: usize,
}

/// One grammar type, ordered by kind, with occurrences across open canvas files.
#[derive(Debug, Clone)]
pub struct MappingRow {
    pub kind: String,
    pub selected: usize,
    pub nodes: Vec<CstHover>,
}

#[derive(Default)]
pub struct CstPanelState {
    cached: Option<CachedCst>,
    /// Trees of the files at the ends of selected edges, by node id.
    edge_trees: std::collections::HashMap<u64, CachedCst>,
    /// Set while the pointer is over a row; reset every frame the panel draws.
    pub hovered: Option<CstHover>,
    /// With Single Hover Symbol off: the hovered row's ancestors' lines.
    pub hover_ancestors: Vec<(usize, usize)>,
    /// "Selected Edge CST Nodes": per file node, the lines of the innermost
    /// named CST node at each end of the selection's edges.
    pub edge_nodes: std::collections::HashMap<u64, Vec<(usize, usize)>>,
    /// The settings group under the gear is open.
    pub show_settings: bool,
    pub mapping: Vec<MappingRow>,
    /// The kind whose style editor is open under its Symbol Mapping row.
    pub editing_style: Option<String>,
    pub mapping_hover: Vec<(CstHover, egui::Color32)>,
    mapping_hash: Option<u64>,
    /// The tint of every open file at the zoomed-out levels, by node id.
    tints: std::collections::HashMap<u64, TintCache>,
    /// How many times a file was parsed (cache misses).
    parses: usize,
}

/// One CST node as the canvas tints it: where it sits in the file.
#[derive(Debug, Clone)]
pub struct TintItem {
    pub kind: String,
    pub depth: usize,
    pub start_line: usize,
    pub end_line: usize,
    /// Left and right edge of the node's bounding box, in columns.
    pub x0: usize,
    pub x1: usize,
}

struct TintCache {
    source_hash: u64,
    settings_hash: u64,
    items: Vec<TintItem>,
}

/// Most nodes of one file the canvas tints.
const MAX_TINT_ITEMS: usize = 6000;
/// Lines scanned to find a node's bounding box; longer nodes use the
/// first line's start and the file's widest column.
const BOX_SCAN_LINES: usize = 200;


struct CachedCst {
    node_id: u64,
    source_hash: u64,
    tree: Option<CstTree>,
}

fn source_hash(node: &crate::nodes::file_node::FileNode) -> u64 {
    node.content_hash
}

impl CstPanelState {
    /// Makes [`Self::tree`] match file node `node_id`, parsing only when the
    /// node or its path/content changed since the last call.
    pub fn sync(&mut self, node_store: &NodeStore, node_id: u64) {
        let Some(node) = node_store.get_file(node_id) else {
            self.cached = None;
            return;
        };
        let source_hash = source_hash(node);
        let fresh = self
            .cached
            .as_ref()
            .is_some_and(|c| c.node_id == node_id && c.source_hash == source_hash);
        if !fresh {
            self.parses += 1;
            self.cached = Some(CachedCst {
                node_id,
                source_hash,
                tree: parse_cst(&node.path, &node.content),
            });
        }
    }

    /// The tree of the last synced node, if its language has a grammar.
    pub fn tree(&self) -> Option<&CstTree> {
        self.cached.as_ref()?.tree.as_ref()
    }

    /// The (cached) tree of any file node, for edge-end highlights.
    fn tree_of(&mut self, node_store: &NodeStore, node_id: u64) -> Option<&CstTree> {
        let node = node_store.get_file(node_id)?;
        let hash = source_hash(node);
        let fresh = self
            .edge_trees
            .get(&node_id)
            .is_some_and(|c| c.source_hash == hash);
        if !fresh {
            self.parses += 1;
            let tree = parse_cst(&node.path, &node.content);
            self.edge_trees.insert(
                node_id,
                CachedCst {
                    node_id,
                    source_hash: hash,
                    tree,
                },
            );
        }
        self.edge_trees.get(&node_id)?.tree.as_ref()
    }

    /// The tinted nodes of file `node_id`: named nodes below the root,
    /// within the depth limit, with their bounding boxes. Cached by content
    /// and settings.
    pub fn tint_items(&mut self, store: &NodeStore, node_id: u64, settings: &CstSettings) -> &[TintItem] {
        let Some(node) = store.get_file(node_id) else {
            self.tints.remove(&node_id);
            return &[];
        };
        let source_hash = source_hash(node);
        let mut hasher = DefaultHasher::new();
        (settings.limit_depth, settings.max_depth).hash(&mut hasher);
        let settings_hash = hasher.finish();
        let fresh = self
            .tints
            .get(&node_id)
            .is_some_and(|c| c.source_hash == source_hash && c.settings_hash == settings_hash);
        if !fresh {
            let items = match self.tree_of(store, node_id) {
                Some(tree) => collect_tint(&tree.root, &node.content, settings),
                None => Vec::new(),
            };
            self.tints.insert(node_id, TintCache { source_hash, settings_hash, items });
        }
        &self.tints[&node_id].items
    }

    /// Clears what the canvas paints for this panel.
    pub fn clear_highlights(&mut self) {
        self.hovered = None;
        self.hover_ancestors.clear();
        self.edge_nodes.clear();
        self.mapping_hover.clear();
    }

    fn sync_mapping(
        &mut self,
        store: &NodeStore,
        selected: &std::collections::HashSet<u64>,
        settings: &CstSettings,
    ) {
        self.edge_trees
            .retain(|id, _| store.get_file(*id).is_some());
        self.tints.retain(|id, _| store.get_file(*id).is_some());
        let mut ids: Vec<_> = store.file_nodes.keys().copied().collect();
        ids.sort_unstable();
        let mut hasher = DefaultHasher::new();
        (
            settings.limit_depth,
            settings.max_depth,
            settings.show_hidden_types,
            settings.semantic_only,
        )
            .hash(&mut hasher);
        for &id in &ids {
            id.hash(&mut hasher);
            source_hash(store.get_file(id).unwrap()).hash(&mut hasher);
            selected.contains(&id).hash(&mut hasher);
        }
        let hash = hasher.finish();
        if self.mapping_hash == Some(hash) {
            return;
        }
        let mut rows = BTreeMap::new();
        for id in ids {
            if let Some(tree) = self.tree_of(store, id) {
                collect_mapping(
                    &tree.root,
                    id,
                    selected.contains(&id),
                    settings,
                    0,
                    &mut rows,
                );
            }
        }
        self.mapping = rows.into_values().collect();
        self.mapping_hash = Some(hash);
    }
}

/// Colors are the reference's role families (`DEFAULT_CST`): see
/// [`crate::analysis::cst_style::family_of`]. A color picked in Symbol
/// Mapping wins.
pub fn mapping_color(kind: &str, settings: &CstSettings) -> egui::Color32 {
    if let Some(style) = settings.type_styles.get(kind) {
        let [r, g, b] = style.color;
        return egui::Color32::from_rgb(r, g, b);
    }
    if let Some(&[r, g, b]) = settings.type_colors.get(kind) {
        return egui::Color32::from_rgb(r, g, b);
    }
    let [r, g, b] = crate::analysis::cst_style::family_of(kind).color();
    egui::Color32::from_rgb(r, g, b)
}

/// The nodes to tint: pre-order, named, below the root.
fn collect_tint(root: &CstNode, content: &str, settings: &CstSettings) -> Vec<TintItem> {
    let lines: Vec<&str> = content.lines().collect();
    let indent: Vec<usize> = lines.iter().map(|l| l.len() - l.trim_start().len()).collect();
    let widest = lines.iter().map(|l| l.len()).max().unwrap_or(0);
    let mut out = Vec::new();
    let mut stack: Vec<(&CstNode, usize)> = root.children.iter().rev().map(|c| (c, 1)).collect();
    while let Some((node, depth)) = stack.pop() {
        if out.len() >= MAX_TINT_ITEMS {
            break;
        }
        if settings.limit_depth && depth > settings.max_depth {
            continue;
        }
        if node.named && node.end_line >= node.start_line {
            let (mut x0, mut x1) = (node.start_col, node.end_col);
            if node.end_line > node.start_line {
                let scan = node.end_line - node.start_line <= BOX_SCAN_LINES;
                x1 = if scan {
                    (node.start_line..=node.end_line).filter_map(|l| lines.get(l)).map(|l| l.len()).max().unwrap_or(x1)
                } else {
                    widest
                };
                if scan {
                    let inner = (node.start_line + 1..=node.end_line)
                        .filter(|&l| lines.get(l).is_some_and(|t| !t.trim().is_empty()))
                        .filter_map(|l| indent.get(l).copied())
                        .min();
                    x0 = inner.map_or(x0, |i| i.min(x0));
                }
            }
            out.push(TintItem {
                kind: node.kind.clone(),
                depth,
                start_line: node.start_line,
                end_line: node.end_line,
                x0,
                x1: x1.max(x0 + 1),
            });
        }
        stack.extend(node.children.iter().rev().map(|c| (c, depth + 1)));
    }
    out
}

fn is_mapped_semantic(kind: &str) -> bool {
    kind.ends_with("_declaration")
        || kind.ends_with("_definition")
        || kind.ends_with("_signature")
        || kind.ends_with("_item")
        || kind.ends_with("_statement")
        || kind.starts_with("jsx_")
        || matches!(
            kind,
            "assignment"
                | "arrow_function"
                | "function_expression"
                | "interface_type"
                | "struct_type"
                | "type_spec"
                | "package_clause"
                | "if_expression"
                | "match_expression"
        )
}

fn collect_mapping(
    node: &CstNode,
    node_id: u64,
    selected: bool,
    settings: &CstSettings,
    depth: usize,
    rows: &mut BTreeMap<String, MappingRow>,
) {
    if !settings.semantic_only || !node.named || is_mapped_semantic(&node.kind) || node.is_error {
        let row = rows.entry(node.kind.clone()).or_insert_with(|| MappingRow {
            kind: node.kind.clone(),
            selected: 0,
            nodes: Vec::new(),
        });
        row.selected += usize::from(selected);
        row.nodes.push(CstHover {
            node_id,
            start_line: node.start_line,
            end_line: node.end_line,
        });
    }
    if settings.limit_depth && depth >= settings.max_depth {
        return;
    }
    for child in listed_children(node, settings) {
        collect_mapping(child, node_id, selected, settings, depth + 1, rows);
    }
}

fn draw_mapping(ui: &mut egui::Ui, state: &mut AppState) {
    let mut hovered = None;
    egui::Frame::NONE
        .stroke(egui::Stroke::new(
            1.0_f32,
            ui.visuals().widgets.noninteractive.bg_stroke.color,
        ))
        .corner_radius(3.0)
        .inner_margin(4.0)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            egui::ScrollArea::vertical()
                .id_salt("cst_mapping")
                .max_height(ui.ctx().screen_rect().height() * 0.75)
                .show(ui, |ui| {
                    for row in &state.cst.mapping {
                        ui.push_id(&row.kind, |ui| {
                            let name = row.kind.replace('_', " ");
                            let mut rgb = mapping_color(&row.kind, &state.settings.cst).to_array()
                                [..3]
                                .try_into()
                                .unwrap();
                            let response = ui
                                .horizontal(|ui| {
                                    let edit = ui.color_edit_button_srgb(&mut rgb);
                                    let label = format!("Edit {name}");
                                    edit.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::WidgetType::ColorButton,
                                            true,
                                            &label,
                                        )
                                    });
                                    edit.on_hover_text(label);
                                    let counts = format!("{}/{}", row.selected, row.nodes.len());
                                    let count_width = ui
                                        .painter()
                                        .layout_no_wrap(
                                            counts.clone(),
                                            egui::FontId::proportional(10.0),
                                            ui.visuals().weak_text_color(),
                                        )
                                        .size()
                                        .x
                                        + 8.0;
                                    ui.add_sized(
                                        egui::vec2(
                                            (ui.available_width() - count_width - 8.0).max(0.0),
                                            20.0,
                                        ),
                                        egui::Label::new(egui::RichText::new(&name).size(11.0))
                                            .truncate(),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(egui::RichText::new(counts).size(10.0).weak());
                                            let open = state.cst.editing_style.as_deref() == Some(row.kind.as_str());
                                            let gear = ui
                                                .small_button(if open { "▾" } else { "⚙" })
                                                .on_hover_text(format!("Style of {name}"));
                                            gear.widget_info(|| {
                                                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Style {name}"))
                                            });
                                            if gear.clicked() {
                                                state.cst.editing_style = if open { None } else { Some(row.kind.clone()) };
                                            }
                                        },
                                    );
                                })
                                .response;
                            if state.cst.editing_style.as_deref() == Some(row.kind.as_str()) {
                                draw_style_editor(ui, &row.kind, &mut state.settings.cst);
                            }
                            if rgb != mapping_color(&row.kind, &state.settings.cst).to_array()[..3]
                            {
                                state.settings.cst.type_colors.insert(row.kind.clone(), rgb);
                            }
                            if response.contains_pointer() {
                                hovered = Some(row.kind.clone());
                                ui.painter().rect_filled(
                                    response.rect,
                                    2.0,
                                    egui::Color32::from_white_alpha(8),
                                );
                            }
                        });
                    }
                    if state.cst.mapping.is_empty() {
                        ui.weak("No CST types match the current filters.");
                    }
                });
        });
    if let Some(row) = hovered.and_then(|kind| state.cst.mapping.iter().find(|r| r.kind == kind)) {
        let color = mapping_color(&row.kind, &state.settings.cst);
        if state.settings.cst.single_hover {
            let node = row
                .nodes
                .iter()
                .find(|n| state.selection.selected.contains(&n.node_id))
                .unwrap_or(&row.nodes[0]);
            state.cst.mapping_hover.push((*node, color));
        } else {
            state
                .cst
                .mapping_hover
                .extend(row.nodes.iter().map(|n| (*n, color)));
        }
    }
}

/// The style editor of one kind (`codecanvas/img/16`): visibility, solid or
/// gradient fill with colors and opacity, border color, width, opacity and
/// sides, a preview of the tint at zoom 0.30 and a reset to the default.
fn draw_style_editor(ui: &mut egui::Ui, kind: &str, settings: &mut CstSettings) {
    use crate::analysis::cst_style::FillMode;
    let original = settings.style_for(kind);
    let mut style = original.clone();
    let mut reset = false;
    egui::Frame::NONE
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(3.0)
        .inner_margin(6.0)
        .show(ui, |ui| {
            ui.checkbox(&mut style.visible, "Visible");
            ui.horizontal(|ui| {
                ui.label("Fill");
                crate::ui::panel::segmented(ui, &mut style.fill, &[(FillMode::Solid, "Solid"), (FillMode::Gradient, "Gradient")]);
            });
            ui.horizontal(|ui| {
                ui.color_edit_button_srgb(&mut style.color);
                if style.fill == FillMode::Gradient {
                    ui.color_edit_button_srgb(&mut style.color_end);
                }
                ui.label("Fill opacity");
                ui.add(egui::Slider::new(&mut style.fill_alpha, 0.0..=1.0).fixed_decimals(2));
            });
            ui.horizontal(|ui| {
                ui.label("Border");
                ui.color_edit_button_srgb(&mut style.border_color);
                ui.add(egui::Slider::new(&mut style.border_width, 0.0..=4.0).text("Width").fixed_decimals(1));
            });
            ui.add(egui::Slider::new(&mut style.border_alpha, 0.0..=1.0).text("Border opacity").fixed_decimals(2));
            ui.horizontal(|ui| {
                ui.label("Sides");
                ui.checkbox(&mut style.sides.top, "Top");
                ui.checkbox(&mut style.sides.right, "Right");
                ui.checkbox(&mut style.sides.bottom, "Bottom");
                ui.checkbox(&mut style.sides.left, "Left");
            });
            ui.weak(preview_text(&style));
            if ui.small_button("Reset to Default").clicked() {
                settings.type_styles.remove(kind);
                settings.type_colors.remove(kind);
                reset = true;
            }
        });
    if !reset && style != original {
        settings.type_styles.insert(kind.to_string(), style);
    }
}

/// "4% at zoom 0.30": the tint a type gets at the first zoomed-out level.
pub fn preview_text(style: &crate::analysis::cst_style::CstTypeStyle) -> String {
    let alpha = crate::nodes::lod::LodLevel::L2.cst_base_alpha() * style.fill_alpha;
    format!("{:.0}% at zoom 0.30", alpha * 100.0)
}

/// Grammar kinds that only group other nodes. "Semantic Nodes Only" lists
/// their children in their place.
pub fn is_wrapper(kind: &str) -> bool {
    kind.ends_with("_list")
        || matches!(
            kind,
            "block"
                | "statement_block"
                | "compound_statement"
                | "parameters"
                | "formal_parameters"
                | "arguments"
                | "argument_list"
                | "type_arguments"
                | "type_parameters"
                | "parenthesized_expression"
                | "expression_statement"
                | "class_body"
                | "interface_body"
        )
}

/// The rows the tree lists under `node`: its children after the Hidden
/// Types and Semantic Nodes Only filters (wrappers replaced by their own
/// listed children).
fn listed_children<'a>(node: &'a CstNode, settings: &CstSettings) -> Vec<&'a CstNode> {
    let mut out = Vec::new();
    for child in &node.children {
        if !(settings.show_hidden_types || child.named || child.is_error) {
            continue;
        }
        if settings.semantic_only && child.named && !child.is_error && is_wrapper(&child.kind) {
            out.extend(listed_children(child, settings));
        } else {
            out.push(child);
        }
    }
    out
}

/// How many rows the tree lists, within the depth limit.
fn count_rows(node: &CstNode, settings: &CstSettings, depth: usize) -> usize {
    1 + if settings.limit_depth && depth >= settings.max_depth {
        0
    } else {
        listed_children(node, settings)
            .into_iter()
            .map(|c| count_rows(c, settings, depth + 1))
            .sum()
    }
}

/// The innermost named node covering `line`, as (start, end) lines.
fn innermost_at(node: &CstNode, line: usize) -> Option<(usize, usize)> {
    if line < node.start_line || line > node.end_line {
        return None;
    }
    node.children
        .iter()
        .filter(|c| c.named)
        .find_map(|c| innermost_at(c, line))
        .or(Some((node.start_line, node.end_line)))
}

/// Limit CST depth and Max depth, shared by the CST settings and
/// Performance › CST Depth.
pub fn draw_depth_controls(ui: &mut egui::Ui, settings: &mut CstSettings) {
    switch(ui, &mut settings.limit_depth, "Limit CST depth")
        .on_hover_text("List syntax nodes only down to Max depth.");
    ui.add_enabled_ui(settings.limit_depth, |ui| {
        ui.horizontal(|ui| {
            ui.label("Max depth");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let response =
                    ui.add(egui::DragValue::new(&mut settings.max_depth).range(1..=64usize));
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Max CST depth")
                });
            });
        });
    });
}

pub fn draw_cst_panel(ui: &mut egui::Ui, state: &mut AppState) {
    state.cst.clear_highlights();
    let settings = state.settings.cst.clone();

    if settings.selected_edge_nodes && settings.show_canvas {
        collect_edge_nodes(state);
    }

    let primary = state
        .selection
        .primary
        .filter(|id| state.node_store.get_file(*id).is_some());
    state
        .cst
        .sync_mapping(&state.node_store, &state.selection.selected, &settings);
    if let Some(id) = primary {
        state.cst.sync(&state.node_store, id);
    }
    ui.horizontal(|ui| {
        ui.weak(format!("({})", state.cst.mapping.len()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let gear = icons::tool_icon_button(ui, icons::SETTINGS, "Toggle settings");
            if gear.clicked() {
                state.cst.show_settings = !state.cst.show_settings;
            }
        });
    });
    switch(
        ui,
        &mut state.settings.cst.selected_edge_nodes,
        "Selected Edge CST Nodes",
    )
    .on_hover_text("Outline the syntax node at each end of the selection's edges.");
    ui.label(
        egui::RichText::new(format!("{}  Symbol Mapping", icons::INFO))
            .size(9.0)
            .weak(),
    )
    .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "Symbol Mapping"));
    if state.cst.show_settings {
        Section::new("Settings").show(ui, |ui| {
            let cst = &mut state.settings.cst;
            Section::new("CST Depth").show(ui, |ui| draw_depth_controls(ui, cst));
            switch(ui, &mut cst.show_hidden_types, "Show Hidden Types")
                .on_hover_text("Also list anonymous tokens such as fn, ( and ;.");
            switch(ui, &mut cst.semantic_only, "Semantic Nodes Only")
                .on_hover_text("Skip blocks, parameter and argument lists and similar wrappers.");
            switch(ui, &mut cst.single_hover, "Single Hover Symbol")
                .on_hover_text("Hovering a row highlights only its node, not the nodes around it.");
        });
    }
    draw_mapping(ui, state);
    ui.separator();
    ui.weak("Syntax tree");

    let Some(node_id) = primary else {
        if state.node_store.file_nodes.is_empty() {
            ui.label("Open some files on the canvas to start populating the CST nodes list.");
        } else {
            ui.label("Select a file node to see its syntax tree.");
        }
        return;
    };
    let node = state.node_store.get_file(node_id).unwrap();
    ui.label(egui::RichText::new(node.file_name()).strong());
    let Some(tree) = state.cst.tree() else {
        ui.label(format!(
            "No syntax tree: {} is not supported.",
            node.language
        ));
        return;
    };
    ui.label(format!(
        "{} \u{b7} {} nodes",
        tree.language.name(),
        tree.node_count
    ));
    if tree.truncated {
        ui.colored_label(ERROR_COLOR, "Large file: only the first nodes are shown.");
    }
    ui.weak(format!(
        "{} tree rows",
        count_rows(&tree.root, &settings, 0)
    ));

    let settings = state.settings.cst.clone();
    let mut hovered: Option<Vec<(usize, usize)>> = None;
    if let Some(tree) = state.cst.tree() {
        let id = ui.make_persistent_id(("cst", node_id));
        let mut path = Vec::new();
        draw_node(ui, &tree.root, &settings, id, 0, &mut path, &mut hovered);
    }
    if let Some(mut chain) = hovered {
        let (start_line, end_line) = chain.pop().expect("the hovered row itself");
        state.cst.hovered = Some(CstHover {
            node_id,
            start_line,
            end_line,
        });
        if !settings.single_hover {
            state.cst.hover_ancestors = chain;
        }
    }
}

/// Selected Edge CST Nodes only marks the ends of the edges of at most this
/// many selected files.
pub const MAX_EDGE_NODE_SELECTION: usize = 3;

/// Fills [`CstPanelState::edge_nodes`] from the selection's visible edges.
fn collect_edge_nodes(state: &mut AppState) {
    if state.selection.count() > MAX_EDGE_NODE_SELECTION {
        return;
    }
    let mut ends: Vec<(u64, usize)> = Vec::new();
    for edge in state.edge_store.all_edges() {
        if !edge.is_selected(&state.selection)
            || !edge.is_visible_for(&state.edge_style, &state.selection)
        {
            continue;
        }
        if let Some(span) = edge.source_span {
            ends.push((edge.source_node, span.start));
        }
        if let Some(span) = edge.target_span {
            ends.push((edge.target_node, span.start));
        }
    }
    ends.sort();
    ends.dedup();
    // Bounded: a hub file can carry hundreds of edges.
    for (node_id, line) in ends.into_iter().take(200) {
        let Some(tree) = state.cst.tree_of(&state.node_store, node_id) else {
            continue;
        };
        if let Some(range) = innermost_at(&tree.root, line) {
            let ranges = state.cst.edge_nodes.entry(node_id).or_default();
            if !ranges.contains(&range) {
                ranges.push(range);
            }
        }
    }
}

fn draw_node(
    ui: &mut egui::Ui,
    node: &CstNode,
    settings: &CstSettings,
    id: egui::Id,
    depth: usize,
    path: &mut Vec<(usize, usize)>,
    hovered: &mut Option<Vec<(usize, usize)>>,
) {
    let mut text = egui::RichText::new(row_label(node)).monospace();
    if node.is_error {
        text = text.color(ERROR_COLOR);
    } else if !node.named {
        text = text.weak();
    }

    let at_limit = settings.limit_depth && depth >= settings.max_depth;
    let children = if at_limit {
        Vec::new()
    } else {
        listed_children(node, settings)
    };
    path.push((node.start_line, node.end_line));

    let response = if children.is_empty() {
        let hidden_below = at_limit && !listed_children(node, settings).is_empty();
        if hidden_below {
            ui.label(text.clone())
                .on_hover_text("Deeper nodes are past Max depth.")
        } else {
            ui.label(text)
        }
    } else {
        egui::CollapsingHeader::new(text)
            .id_salt(id)
            .default_open(depth == 0)
            .show(ui, |ui| {
                for (i, child) in children.into_iter().enumerate() {
                    draw_node(ui, child, settings, id.with(i), depth + 1, path, hovered);
                }
            })
            .header_response
    };
    if response.hovered() {
        *hovered = Some(path.clone());
    }
    path.pop();
}

/// `field: kind "text"  L3–L7`, with 1-based lines.
pub fn row_label(node: &CstNode) -> String {
    let mut label = String::new();
    if let Some(field) = &node.field {
        label.push_str(field);
        label.push_str(": ");
    }
    label.push_str(&node.kind);
    if let Some(text) = &node.text {
        label.push_str(&format!(" {text:?}"));
    }
    if node.start_line == node.end_line {
        label.push_str(&format!("  L{}", node.start_line + 1));
    } else {
        label.push_str(&format!(
            "  L{}\u{2013}{}",
            node.start_line + 1,
            node.end_line + 1
        ));
    }
    label
}

/// Screen rect covering code lines `start..=end` (0-based) of a file node
/// whose first code line starts at `top_left`. Lines past the node's
/// `visible_lines` are cut off; `None` if none of the range is visible.
pub fn highlight_rect(
    top_left: egui::Pos2,
    width: f32,
    line_h: f32,
    start: usize,
    end: usize,
    visible_lines: usize,
) -> Option<egui::Rect> {
    if start >= visible_lines || end < start {
        return None;
    }
    let end = end.min(visible_lines - 1);
    Some(egui::Rect::from_min_max(
        egui::pos2(top_left.x, top_left.y + start as f32 * line_h),
        egui::pos2(top_left.x + width, top_left.y + (end + 1) as f32 * line_h),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn store_with(path: &str, content: &str) -> (NodeStore, u64) {
        let mut store = NodeStore::default();
        let id = store.add_file(PathBuf::from(path), content.to_string(), egui::Pos2::ZERO);
        (store, id)
    }

    fn rust_tree(src: &str) -> CstTree {
        parse_cst(std::path::Path::new("/p/a.rs"), src).unwrap()
    }

    #[test]
    fn mapping_cache_tracks_content_filters_selection_and_closed_files() {
        let (mut store, a) = store_with("a.rs", "fn a() { let x = 1; }\n");
        let b = store.add_file(
            PathBuf::from("b.rs"),
            "fn b() {}\n".into(),
            egui::Pos2::ZERO,
        );
        let mut cst = CstPanelState::default();
        let mut settings = CstSettings::default();
        let mut selected = std::collections::HashSet::from([a]);
        cst.sync_mapping(&store, &selected, &settings);
        let row = cst
            .mapping
            .iter()
            .find(|r| r.kind == "function_item")
            .unwrap();
        assert_eq!((row.selected, row.nodes.len()), (1, 2));
        assert!(cst.mapping.windows(2).all(|w| w[0].kind < w[1].kind));
        assert!(!cst.mapping.iter().any(|r| r.kind == "identifier"));
        cst.sync_mapping(&store, &selected, &settings);
        assert_eq!(cst.parses, 2);
        selected.insert(b);
        cst.sync_mapping(&store, &selected, &settings);
        assert_eq!(cst.parses, 2, "selection changes don't reparse");
        assert_eq!(
            cst.mapping
                .iter()
                .find(|r| r.kind == "function_item")
                .unwrap()
                .selected,
            2
        );
        store.replace_content(b, "struct B;\n".into());
        cst.sync_mapping(&store, &selected, &settings);
        assert_eq!(cst.parses, 3);
        assert_eq!(
            cst.mapping
                .iter()
                .find(|r| r.kind == "function_item")
                .unwrap()
                .nodes
                .len(),
            1
        );
        settings.max_depth = 0;
        cst.sync_mapping(&store, &selected, &settings);
        assert!(cst.mapping.is_empty(), "the root is not a semantic symbol");
        settings.limit_depth = false;
        settings.semantic_only = false;
        cst.sync_mapping(&store, &selected, &settings);
        assert!(cst.mapping.iter().any(|r| r.kind == "identifier"));
        assert!(!cst.mapping.iter().any(|r| r.kind == "fn"));
        settings.show_hidden_types = true;
        cst.sync_mapping(&store, &selected, &settings);
        assert!(cst.mapping.iter().any(|r| r.kind == "fn"));
        store.file_nodes.remove(&a);
        cst.sync_mapping(&store, &selected, &settings);
        assert!(!cst.mapping.iter().any(|r| r.kind == "function_item"));
        assert!(!cst.edge_trees.contains_key(&a));
    }

    #[test]
    fn mapping_colors_persist_and_old_settings_keep_defaults() {
        let mut settings = CstSettings::default();
        assert_eq!(
            mapping_color("function_item", &settings),
            egui::Color32::from_rgb(59, 130, 246)
        );
        assert_eq!(
            mapping_color("abstract_class_declaration", &settings),
            egui::Color32::from_rgb(168, 85, 247)
        );
        settings
            .type_colors
            .insert("function_item".into(), [1, 2, 3]);
        let persisted = serde_json::to_string(&settings).unwrap();
        let restored = serde_json::from_str(&persisted).unwrap();
        assert_eq!(
            mapping_color("function_item", &restored),
            egui::Color32::from_rgb(1, 2, 3)
        );
        let legacy: CstSettings = serde_json::from_str(r#"{"max_depth":3}"#).unwrap();
        assert_eq!(legacy.max_depth, 3);
        assert!(legacy.type_colors.is_empty());
    }

    fn kinds(nodes: &[&CstNode]) -> Vec<String> {
        nodes.iter().map(|n| n.kind.clone()).collect()
    }

    #[test]
    fn semantic_nodes_only_lists_a_wrappers_children_in_its_place() {
        let tree = rust_tree("fn f(a: u8) {\n    let x = a;\n}\n");
        let function = &tree.root.children[0];
        let mut settings = CstSettings {
            semantic_only: false,
            ..Default::default()
        };
        let plain = kinds(&listed_children(function, &settings));
        assert!(
            plain.contains(&"parameters".to_owned()) && plain.contains(&"block".to_owned()),
            "{plain:?}"
        );
        settings.semantic_only = true;
        let semantic = kinds(&listed_children(function, &settings));
        assert!(
            !semantic.contains(&"block".to_owned()) && !semantic.contains(&"parameters".to_owned())
        );
        assert!(
            semantic.contains(&"parameter".to_owned())
                && semantic.contains(&"let_declaration".to_owned()),
            "{semantic:?}"
        );
        settings.show_hidden_types = true;
        assert!(kinds(&listed_children(function, &settings)).contains(&"fn".to_owned()));
    }

    #[test]
    fn max_depth_bounds_the_listed_rows() {
        let tree = rust_tree("mod m {\n    fn f() {\n        let x = 1;\n    }\n}\n");
        let deep = CstSettings {
            limit_depth: false,
            ..Default::default()
        };
        let one = CstSettings {
            limit_depth: true,
            max_depth: 1,
            ..Default::default()
        };
        assert_eq!(
            count_rows(&tree.root, &one, 0),
            2,
            "source_file and mod_item"
        );
        assert!(count_rows(&tree.root, &deep, 0) > count_rows(&tree.root, &one, 0));
    }

    #[test]
    fn innermost_named_node_covers_the_line() {
        let tree = rust_tree("fn f() {\n    let x = 1;\n}\n");
        assert_eq!(
            innermost_at(&tree.root, 1),
            Some((1, 1)),
            "the let statement"
        );
        assert_eq!(innermost_at(&tree.root, 0), Some((0, 0)));
        assert_eq!(innermost_at(&tree.root, 99), None);
    }

    #[test]
    fn sync_parses_once_until_the_content_changes() {
        let (mut store, id) = store_with("a.rs", "fn a() {}\n");
        let mut cst = CstPanelState::default();

        cst.sync(&store, id);
        cst.sync(&store, id);
        assert_eq!(cst.parses, 1);
        assert_eq!(cst.tree().unwrap().root.kind, "source_file");

        store.replace_content(id, "struct S;\n".into());
        cst.sync(&store, id);
        assert_eq!(cst.parses, 2);
        let root = &cst.tree().unwrap().root;
        assert_eq!(root.children[0].kind, "struct_item");
    }

    #[test]
    fn sync_switches_nodes_and_handles_unsupported_and_missing_ones() {
        let (mut store, rs) = store_with("a.rs", "fn a() {}\n");
        let md = store.add_file(
            PathBuf::from("README.md"),
            "# hi\n".into(),
            egui::Pos2::ZERO,
        );
        let mut cst = CstPanelState::default();

        cst.sync(&store, rs);
        assert!(cst.tree().is_some());
        cst.sync(&store, md);
        assert!(cst.tree().is_none(), "Markdown has no grammar");
        cst.sync(&store, 999);
        assert!(cst.tree().is_none());
        assert_eq!(cst.parses, 2);
    }

    #[test]
    fn row_label_shows_field_kind_text_and_one_based_lines() {
        let node = CstNode {
            kind: "identifier".into(),
            field: Some("name".into()),
            named: true,
            is_error: false,
            start_line: 2,
            end_line: 2,
            start_col: 0,
            end_col: 5,
            text: Some("hello".into()),
            children: Vec::new(),
        };
        assert_eq!(row_label(&node), "name: identifier \"hello\"  L3");

        let block = CstNode {
            kind: "block".into(),
            field: None,
            text: None,
            end_line: 6,
            ..node
        };
        assert_eq!(row_label(&block), "block  L3\u{2013}7");
    }

    #[test]
    fn highlight_rect_covers_the_lines_and_clips_to_visible_ones() {
        let origin = egui::pos2(10.0, 100.0);
        let r = highlight_rect(origin, 50.0, 2.0, 1, 3, 10).unwrap();
        assert_eq!(r.min, egui::pos2(10.0, 102.0));
        assert_eq!(r.max, egui::pos2(60.0, 108.0));

        let clipped = highlight_rect(origin, 50.0, 2.0, 8, 20, 10).unwrap();
        assert_eq!(clipped.max.y, 120.0);

        assert!(highlight_rect(origin, 50.0, 2.0, 10, 12, 10).is_none());
        assert!(highlight_rect(origin, 50.0, 2.0, 0, 0, 0).is_none());
    }
}
