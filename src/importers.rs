//! "Open files that import the selection": the language server is asked for
//! the references of the symbols each selected file declares at top level, and
//! the files holding those references (plus the ones the syntax links resolve)
//! are opened when every answer is in. Files without a language server fall
//! back to the local import links alone.

use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;

use crate::analysis::lsp::manager::{LspRequest, LspResponse, RequestId};
use crate::analysis::lsp::{server_for, Location};
use crate::analysis::symbol_mapping::{top_level_tokens, utf16_column};
use crate::state::AppState;
use crate::workspace::{self, ImportDirection};

/// Symbols asked about per selected file.
pub const MAX_SYMBOLS_PER_FILE: usize = 40;
/// Files one action opens at most.
pub const MAX_OPENED: usize = 100;

/// The answers an importers query is waiting for.
#[derive(Debug, Default)]
pub struct ImporterQuery {
    pending: HashSet<RequestId>,
    found: BTreeSet<PathBuf>,
    /// The selected files themselves: a use inside them is not an importer.
    exclude: HashSet<PathBuf>,
    /// Asked without an egui context: the shell polls while it waits.
    poll: bool,
}

impl ImporterQuery {
    /// Takes the answer of one of this query's requests; false for others.
    pub fn receive(&mut self, id: RequestId, response: &LspResponse) -> bool {
        if !self.pending.remove(&id) {
            return false;
        }
        if let LspResponse::References(Ok(locations)) = response {
            self.add(locations);
        }
        true
    }

    fn add(&mut self, locations: &[Location]) {
        for location in locations {
            if !self.exclude.contains(&location.path) {
                self.found.insert(location.path.clone());
            }
        }
    }

    pub fn is_waiting(&self) -> bool {
        !self.pending.is_empty()
    }

    pub fn needs_polling(&self) -> bool {
        self.poll && self.is_waiting()
    }
}

/// Whether "Open files that import the selection" can find anything: a local
/// import link resolves to the selection, or a selected file has a language
/// server to ask.
pub fn can_open_importers(state: &AppState) -> bool {
    if state.lsp.importers.is_some() {
        return false;
    }
    !workspace::import_neighbours(state, ImportDirection::Importers).is_empty()
        || state
            .selection
            .selected_ids()
            .filter_map(|id| state.node_store.get_file(id))
            .any(|n| server_for(&n.path).is_some() && !top_level_tokens(&n.path, &n.content, true).is_empty())
}

/// Whether "Open imported files" has a file to open.
pub fn can_open_imports(state: &AppState) -> bool {
    !workspace::import_neighbours(state, ImportDirection::Imported).is_empty()
}

/// Starts the query for the selection. Returns how many language server
/// requests were asked; with none, the local links are opened at once.
pub fn open_importers(ctx: Option<&egui::Context>, state: &mut AppState) -> usize {
    let Some(root) = state.loaded_dir.clone() else { return 0 };
    let mut query = ImporterQuery { poll: ctx.is_none(), ..Default::default() };
    query.found.extend(workspace::import_neighbours(state, ImportDirection::Importers));
    let selected: Vec<(PathBuf, String)> = state
        .selection
        .selected_ids()
        .filter_map(|id| state.node_store.get_file(id))
        .map(|n| (n.path.clone(), n.content.clone()))
        .collect();
    query.exclude = selected.iter().map(|(p, _)| p.clone()).collect();
    let mut asked = 0;
    for (path, text) in &selected {
        let Some((server, language_id)) = server_for(path) else { continue };
        for token in top_level_tokens(path, text, true).into_iter().take(MAX_SYMBOLS_PER_FILE) {
            let Some(line) = text.lines().nth(token.line) else { continue };
            let position = crate::analysis::lsp::Position { line: token.line, character: utf16_column(line, token.start) };
            let id = state.lsp.manager.request(
                ctx,
                server,
                &root,
                LspRequest::References { path: path.clone(), language_id, text: text.clone(), position },
            );
            query.pending.insert(id);
            asked += 1;
        }
    }
    state.lsp.importers = Some(query);
    finish(state);
    asked
}

/// Opens the importers once every answer is in. Called every frame.
pub fn finish(state: &mut AppState) {
    if state.lsp.importers.as_ref().is_none_or(ImporterQuery::is_waiting) {
        return;
    }
    let Some(query) = state.lsp.importers.take() else { return };
    let root = state.loaded_dir.clone().unwrap_or_default();
    let open: HashSet<PathBuf> = state.node_store.file_nodes.values().map(|n| n.path.clone()).collect();
    let paths: Vec<PathBuf> = query
        .found
        .into_iter()
        .filter(|p| p.starts_with(&root) && p.is_file() && !open.contains(p))
        .take(MAX_OPENED)
        .collect();
    if paths.is_empty() {
        crate::ui::toast::push(state, "No other files import the selection.");
    } else {
        workspace::open_files(state, &paths);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::lsp::{Position, Range};

    fn at(path: &str) -> Location {
        Location { path: path.into(), range: Range { start: Position::default(), end: Position::default() } }
    }

    #[test]
    fn answers_are_collected_without_the_selection_itself() {
        let mut query = ImporterQuery::default();
        query.exclude.insert("/p/a.rs".into());
        query.pending.extend([1, 2]);
        assert!(query.receive(1, &LspResponse::References(Ok(vec![at("/p/a.rs"), at("/p/b.rs"), at("/p/b.rs")]))));
        assert!(query.is_waiting());
        assert!(!query.receive(9, &LspResponse::References(Ok(vec![at("/p/z.rs")]))), "not ours");
        assert!(query.receive(2, &LspResponse::References(Err("down".into()))), "a failed answer still counts");
        assert!(!query.is_waiting());
        assert_eq!(query.found.iter().collect::<Vec<_>>(), [&PathBuf::from("/p/b.rs")]);
    }
}
