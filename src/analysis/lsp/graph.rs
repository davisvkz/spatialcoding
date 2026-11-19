//! The graph of one symbol as `codecanvas/renderizacao/lsp.md` describes it:
//! references, definitions, type definitions, implementations and the
//! incoming and outgoing calls, each with the role its entries play.

use std::path::PathBuf;

use serde_json::Value;

use super::protocol::{parse_range, uri_to_path};
use super::{Location, Range};

/// What one entry of a symbol graph is. The labels are the ones the
/// reference's Symbol Graph panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Role {
    ImplSel,
    ImplCtx,
    InCallsite,
    InCaller,
    InCallerSel,
    OutCallsite,
    OutCallee,
    OutCalleeSel,
    TypeDef,
    DefSel,
    DefCtx,
    Ref,
}

/// The sections of the Symbol Graph panel, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Group {
    Implementations,
    IncomingCalls,
    OutgoingCalls,
    TypeDefinitions,
    Definitions,
    References,
}

impl Group {
    pub const ALL: [Group; 6] = [
        Group::Implementations,
        Group::IncomingCalls,
        Group::OutgoingCalls,
        Group::TypeDefinitions,
        Group::Definitions,
        Group::References,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Group::Implementations => "Implementations",
            Group::IncomingCalls => "Incoming Calls",
            Group::OutgoingCalls => "Outgoing Calls",
            Group::TypeDefinitions => "Type Definitions",
            Group::Definitions => "Definitions",
            Group::References => "References",
        }
    }
}

impl Role {
    /// `in callsite`, `caller sel`, `def ctx`…
    pub fn label(self) -> &'static str {
        match self {
            Role::ImplSel => "impl sel",
            Role::ImplCtx => "impl ctx",
            Role::InCallsite => "in callsite",
            Role::InCaller => "caller",
            Role::InCallerSel => "caller sel",
            Role::OutCallsite => "out callsite",
            Role::OutCallee => "callee",
            Role::OutCalleeSel => "callee sel",
            Role::TypeDef => "type def",
            Role::DefSel => "def sel",
            Role::DefCtx => "def ctx",
            Role::Ref => "ref",
        }
    }

    pub fn group(self) -> Group {
        match self {
            Role::ImplSel | Role::ImplCtx => Group::Implementations,
            Role::InCallsite | Role::InCaller | Role::InCallerSel => Group::IncomingCalls,
            Role::OutCallsite | Role::OutCallee | Role::OutCalleeSel => Group::OutgoingCalls,
            Role::TypeDef => Group::TypeDefinitions,
            Role::DefSel | Role::DefCtx => Group::Definitions,
            Role::Ref => Group::References,
        }
    }

    /// The color of the role (`codecanvas/renderizacao/lsp.md` §4).
    pub fn color(self) -> [u8; 3] {
        match self.group() {
            Group::References => [0xFF, 0xC4, 0x65],
            Group::Definitions | Group::TypeDefinitions => [0xFF, 0x5C, 0xBF],
            Group::Implementations => [0x00, 0xFF, 0x65],
            Group::IncomingCalls => [0xC4, 0xA3, 0xFF],
            Group::OutgoingCalls => [0xFF, 0x6B, 0x24],
        }
    }

    /// Fill alpha of the role's rectangle: callsites .26, callers and
    /// callees .16, a selection .30, an implementation .05, a reference .28.
    pub fn fill_alpha(self) -> f32 {
        match self {
            Role::InCallsite | Role::OutCallsite => 0.26,
            Role::InCaller | Role::OutCallee => 0.16,
            Role::InCallerSel | Role::OutCalleeSel | Role::DefSel | Role::ImplSel => 0.30,
            Role::ImplCtx => 0.05,
            Role::DefCtx | Role::TypeDef => 0.16,
            Role::Ref => 0.28,
        }
    }

    /// Whether the entry is a name (a small mark and an edge end) rather
    /// than the context block around one.
    pub fn is_token(self) -> bool {
        !matches!(
            self,
            Role::ImplCtx | Role::InCaller | Role::OutCallee | Role::DefCtx | Role::TypeDef
        )
    }
}

/// One place of a symbol graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphEntry {
    pub role: Role,
    pub path: PathBuf,
    pub range: Range,
}

/// The graph of the symbol under `source`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolGraph {
    /// The token the symbol was asked at (white at alpha .35).
    pub source: Location,
    pub entries: Vec<GraphEntry>,
}

impl SymbolGraph {
    /// Entries of one group, in file and line order.
    pub fn group(&self, group: Group) -> Vec<&GraphEntry> {
        let mut items: Vec<&GraphEntry> = self.entries.iter().filter(|e| e.role.group() == group).collect();
        items.sort_by(|a, b| (&a.path, a.range).cmp(&(&b.path, b.range)));
        items
    }

    /// Distinct files of the entries with how many entries each holds.
    pub fn files(&self) -> Vec<(PathBuf, usize)> {
        let mut counts: std::collections::BTreeMap<&PathBuf, usize> = std::collections::BTreeMap::new();
        for e in &self.entries {
            *counts.entry(&e.path).or_default() += 1;
        }
        counts.into_iter().map(|(p, n)| (p.clone(), n)).collect()
    }

    /// Entries drawn as an edge from the source token: every token entry.
    pub fn edge_count(&self) -> usize {
        self.entries.iter().filter(|e| e.role.is_token()).count()
    }
}

/// A definition-like answer: the name, and the whole declaration when the
/// server said so (`LocationLink`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub path: PathBuf,
    pub selection: Range,
    pub context: Option<Range>,
}

/// Parses a `definition`, `typeDefinition` or `implementation` result:
/// `Location`, `Location[]` or `LocationLink[]`.
pub fn parse_links(result: &Value) -> Vec<Link> {
    let items: Vec<&Value> = match result {
        Value::Array(items) => items.iter().collect(),
        Value::Object(_) => vec![result],
        _ => Vec::new(),
    };
    let mut links: Vec<Link> = items
        .into_iter()
        .filter_map(|v| {
            if let Some(uri) = v.get("targetUri") {
                let context = v.get("targetRange").and_then(parse_range);
                let selection = v.get("targetSelectionRange").and_then(parse_range).or(context)?;
                Some(Link { path: uri_to_path(uri.as_str()?)?, selection, context })
            } else {
                let range = parse_range(v.get("range")?)?;
                Some(Link { path: uri_to_path(v.get("uri")?.as_str()?)?, selection: range, context: None })
            }
        })
        .collect();
    links.sort_by(|a, b| (&a.path, a.selection).cmp(&(&b.path, b.selection)));
    links.dedup();
    links
}

/// A `CallHierarchyItem`: where a function lives.
#[derive(Debug, Clone, PartialEq)]
pub struct CallItem {
    pub path: PathBuf,
    /// The whole function.
    pub range: Range,
    /// Its name.
    pub selection: Range,
    /// The raw item, sent back to the server to ask for its calls.
    pub raw: Value,
}

/// Parses `textDocument/prepareCallHierarchy`.
pub fn parse_call_items(result: &Value) -> Vec<CallItem> {
    result
        .as_array()
        .map(|items| items.iter().filter_map(parse_call_item).collect())
        .unwrap_or_default()
}

fn parse_call_item(v: &Value) -> Option<CallItem> {
    let range = parse_range(v.get("range")?)?;
    Some(CallItem {
        path: uri_to_path(v.get("uri")?.as_str()?)?,
        range,
        selection: v.get("selectionRange").and_then(parse_range).unwrap_or(range),
        raw: v.clone(),
    })
}

/// One answer of `callHierarchy/incomingCalls` or `outgoingCalls`: the
/// function on the other side and the call sites (`fromRanges`, in the
/// caller's file).
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub item: CallItem,
    pub sites: Vec<Range>,
}

/// `field` is `from` for incoming calls and `to` for outgoing ones.
pub fn parse_calls(result: &Value, field: &str) -> Vec<Call> {
    result
        .as_array()
        .map(|calls| {
            calls
                .iter()
                .filter_map(|c| {
                    let item = parse_call_item(c.get(field)?)?;
                    let sites = c
                        .get("fromRanges")
                        .and_then(Value::as_array)
                        .map(|r| r.iter().filter_map(parse_range).collect())
                        .unwrap_or_default();
                    Some(Call { item, sites })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Everything the queries returned, for [`build`].
#[derive(Debug, Default)]
pub struct Answers {
    pub references: Vec<Location>,
    pub definitions: Vec<Link>,
    pub type_definitions: Vec<Link>,
    pub implementations: Vec<Link>,
    pub incoming: Vec<Call>,
    pub outgoing: Vec<Call>,
}

/// Builds the entries of the graph of the symbol at `source` (in `source_path`)
/// from the answers.
pub fn build(source: Location, answers: Answers) -> SymbolGraph {
    let mut entries = Vec::new();
    let mut push = |role, path: &PathBuf, range: Range| entries.push(GraphEntry { role, path: path.clone(), range });

    for r in &answers.references {
        // The token itself is drawn as the source, not as a reference.
        if *r != source {
            push(Role::Ref, &r.path, r.range);
        }
    }
    for d in &answers.definitions {
        push(Role::DefSel, &d.path, d.selection);
        if let Some(ctx) = d.context.filter(|c| *c != d.selection) {
            push(Role::DefCtx, &d.path, ctx);
        }
    }
    for t in &answers.type_definitions {
        push(Role::TypeDef, &t.path, t.context.unwrap_or(t.selection));
    }
    for i in &answers.implementations {
        push(Role::ImplSel, &i.path, i.selection);
        if let Some(ctx) = i.context.filter(|c| *c != i.selection) {
            push(Role::ImplCtx, &i.path, ctx);
        }
    }
    for call in &answers.incoming {
        push(Role::InCaller, &call.item.path, call.item.range);
        push(Role::InCallerSel, &call.item.path, call.item.selection);
        for site in &call.sites {
            push(Role::InCallsite, &call.item.path, *site);
        }
    }
    for call in &answers.outgoing {
        push(Role::OutCallee, &call.item.path, call.item.range);
        push(Role::OutCalleeSel, &call.item.path, call.item.selection);
        for site in &call.sites {
            push(Role::OutCallsite, &source.path, *site);
        }
    }
    SymbolGraph { source, entries }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::lsp::Position;
    use serde_json::json;

    fn range(l1: usize, c1: usize, l2: usize, c2: usize) -> Range {
        Range { start: Position { line: l1, character: c1 }, end: Position { line: l2, character: c2 } }
    }

    fn loc(path: &str, r: Range) -> Location {
        Location { path: path.into(), range: r }
    }

    #[test]
    fn links_keep_the_name_and_the_declaration_apart() {
        let result = json!([
            {"targetUri": "file:///p/a.ts", "targetRange": {"start": {"line": 3, "character": 0}, "end": {"line": 9, "character": 1}},
             "targetSelectionRange": {"start": {"line": 3, "character": 16}, "end": {"line": 3, "character": 22}}},
            {"uri": "file:///p/b.ts", "range": {"start": {"line": 1, "character": 2}, "end": {"line": 1, "character": 5}}}
        ]);
        let links = parse_links(&result);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].selection, range(3, 16, 3, 22));
        assert_eq!(links[0].context, Some(range(3, 0, 9, 1)));
        assert_eq!(links[1].context, None, "a plain Location has no declaration range");
        assert!(parse_links(&Value::Null).is_empty());
    }

    #[test]
    fn call_hierarchy_items_and_calls_parse() {
        let item = json!({"name": "f", "kind": 12, "uri": "file:///p/a.rs",
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 4, "character": 1}},
            "selectionRange": {"start": {"line": 0, "character": 3}, "end": {"line": 0, "character": 4}}});
        let items = parse_call_items(&json!([item.clone()]));
        assert_eq!(items[0].selection, range(0, 3, 0, 4));
        assert_eq!(items[0].raw["name"], "f");

        let incoming = json!([{"from": item.clone(), "fromRanges": [{"start": {"line": 2, "character": 4}, "end": {"line": 2, "character": 5}}]}]);
        let calls = parse_calls(&incoming, "from");
        assert_eq!(calls[0].sites, vec![range(2, 4, 2, 5)]);
        assert!(parse_calls(&incoming, "to").is_empty(), "the field name decides");
    }

    #[test]
    fn building_a_graph_assigns_a_role_to_every_entry() {
        let source = loc("/p/a.rs", range(5, 4, 5, 9));
        let caller = CallItem { path: "/p/b.rs".into(), range: range(10, 0, 14, 1), selection: range(10, 3, 10, 8), raw: Value::Null };
        let callee = CallItem { path: "/p/c.rs".into(), range: range(0, 0, 3, 1), selection: range(0, 3, 0, 6), raw: Value::Null };
        let graph = build(
            source.clone(),
            Answers {
                references: vec![source.clone(), loc("/p/b.rs", range(12, 4, 12, 9))],
                definitions: vec![Link { path: "/p/a.rs".into(), selection: range(1, 3, 1, 8), context: Some(range(1, 0, 3, 1)) }],
                type_definitions: vec![Link { path: "/p/t.rs".into(), selection: range(0, 4, 0, 5), context: Some(range(0, 0, 2, 1)) }],
                implementations: vec![Link { path: "/p/i.rs".into(), selection: range(2, 4, 2, 9), context: Some(range(2, 0, 8, 1)) }],
                incoming: vec![Call { item: caller, sites: vec![range(12, 4, 12, 9)] }],
                outgoing: vec![Call { item: callee, sites: vec![range(6, 8, 6, 11)] }],
            },
        );
        let roles: Vec<Role> = graph.entries.iter().map(|e| e.role).collect();
        for role in [
            Role::Ref, Role::DefSel, Role::DefCtx, Role::TypeDef, Role::ImplSel, Role::ImplCtx,
            Role::InCaller, Role::InCallerSel, Role::InCallsite, Role::OutCallee, Role::OutCalleeSel, Role::OutCallsite,
        ] {
            assert!(roles.contains(&role), "{role:?}");
        }
        assert_eq!(roles.iter().filter(|r| **r == Role::Ref).count(), 1, "the source token is not a reference");
        // The outgoing callsite is in the source file.
        let callsite = graph.entries.iter().find(|e| e.role == Role::OutCallsite).unwrap();
        assert_eq!(callsite.path, source.path);
    }

    #[test]
    fn groups_files_and_counts_follow_the_panel() {
        let source = loc("/p/a.rs", range(0, 0, 0, 1));
        let graph = build(
            source,
            Answers {
                references: vec![loc("/p/b.rs", range(1, 0, 1, 1)), loc("/p/b.rs", range(2, 0, 2, 1)), loc("/p/c.rs", range(3, 0, 3, 1))],
                definitions: vec![Link { path: "/p/a.rs".into(), selection: range(0, 0, 0, 1), context: None }],
                ..Default::default()
            },
        );
        assert_eq!(graph.group(Group::References).len(), 3);
        assert_eq!(graph.group(Group::Definitions).len(), 1);
        assert_eq!(graph.files(), vec![("/p/a.rs".into(), 1), ("/p/b.rs".into(), 2), ("/p/c.rs".into(), 1)]);
        assert_eq!(graph.edge_count(), 4);
        assert_eq!(Group::ALL.map(Group::title)[1], "Incoming Calls");
    }

    #[test]
    fn roles_carry_the_reference_colors_and_alphas() {
        assert_eq!(Role::Ref.color(), [0xFF, 0xC4, 0x65]);
        assert_eq!(Role::DefSel.color(), [0xFF, 0x5C, 0xBF]);
        assert_eq!(Role::ImplCtx.color(), [0x00, 0xFF, 0x65]);
        assert_eq!(Role::InCallsite.color(), [0xC4, 0xA3, 0xFF]);
        assert_eq!(Role::OutCallee.color(), [0xFF, 0x6B, 0x24]);
        assert_eq!((Role::InCallsite.fill_alpha(), Role::InCaller.fill_alpha(), Role::DefSel.fill_alpha()), (0.26, 0.16, 0.30));
        assert_eq!(Role::ImplCtx.fill_alpha(), 0.05);
        assert_eq!(Role::InCallsite.label(), "in callsite");
        assert!(Role::InCallerSel.is_token() && !Role::InCaller.is_token());
    }
}
