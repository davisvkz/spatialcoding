//! Background dependency analysis through the language servers.
//!
//! [`DependencyIndexer::rebuild`] asks the server of every open file for its
//! symbols; each answer queues one references request per picked symbol.
//! Answers are cached per file content, so opening another file only asks
//! about the new one: the cached references already cover the whole
//! workspace. At most [`MAX_IN_FLIGHT`] requests per server are queued at
//! once, so the LSP tab's own requests are never stuck behind hundreds of
//! these. Answers given while the server was still starting or indexing may be
//! incomplete; such files are asked again once the server is ready.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use super::{DependencyLink, LineSpan, OpenFiles, StatementFinder, byte_offset, pick_symbols, usage_occurrences};
use crate::analysis::lsp::manager::{LspManager, LspRequest, LspResponse, RequestId, ServerStatus};
use crate::analysis::lsp::{Location, LspSymbol, Position, ServerId, server_for};

/// Requests of the indexer waiting on one server at a time.
pub const MAX_IN_FLIGHT: usize = 4;

struct SymbolEntry {
    symbol: LspSymbol,
    label: String,
    definition: LineSpan,
    position: Position,
    /// End of the symbol name (selection_range.end), for inline highlight.
    name_end: Position,
    references: Option<Vec<Location>>,
}

struct FileEntry {
    hash: u64,
    server: ServerId,
    language_id: &'static str,
    text: String,
    /// `None` while the symbols are on their way.
    symbols: Option<Result<Vec<SymbolEntry>, String>>,
    /// Some answer came while the server was not ready.
    stale: bool,
    /// Already asked again after a stale answer.
    retried: bool,
}

impl FileEntry {
    fn answered(&self) -> bool {
        match &self.symbols {
            None => false,
            Some(Err(_)) => true,
            Some(Ok(symbols)) => symbols.iter().all(|s| s.references.is_some()),
        }
    }
}

#[derive(Debug, Clone)]
enum Job {
    Symbols { path: PathBuf, hash: u64 },
    References { path: PathBuf, hash: u64, index: usize },
}

impl Job {
    fn file(&self) -> (&Path, u64) {
        match self {
            Job::Symbols { path, hash } | Job::References { path, hash, .. } => (path, *hash),
        }
    }
}

/// Progress of one language server, for the Edges tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerProgress {
    pub server: ServerId,
    pub files: usize,
    pub done: usize,
    pub status: Option<ServerStatus>,
    /// Why the symbols of a file could not be read (a missing server).
    pub error: Option<String>,
}

/// An exact definition or reference name from LSP, with UTF-16 converted to
/// byte columns for the canvas's measured glyph layout. No text-name guessing.
pub struct SymbolOccurrence<'a> {
    pub definition_path: &'a Path,
    pub symbol: &'a LspSymbol,
    pub name_span: (usize, usize, usize),
    pub definition_name_span: (usize, usize, usize),
}

#[derive(Default)]
pub struct DependencyIndexer {
    local_links: Vec<DependencyLink>,
    root: Option<PathBuf>,
    files: HashMap<PathBuf, FileEntry>,
    queue: VecDeque<Job>,
    in_flight: HashMap<RequestId, Job>,
    changed: bool,
}

fn hash_text(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

impl DependencyIndexer {
    /// Makes the analysis cover exactly `files` (`(node path, content)` of
    /// the open file nodes) of the project at `root`.
    pub fn rebuild(&mut self, manager: &mut LspManager, root: &Path, files: &[(&Path, &str)]) {
        if self.root.as_deref() != Some(root) {
            *self = Self {
                root: Some(root.to_path_buf()),
                ..Self::default()
            };
        }
        self.local_links = super::local::links(root, &OpenFiles::new(files.iter().copied()));
        self.files
            .retain(|path, entry| files.iter().any(|(p, text)| *p == path && hash_text(text) == entry.hash));
        for (path, text) in files {
            if self.files.contains_key(*path) {
                continue;
            }
            let Some((server, language_id)) = server_for(path) else {
                continue;
            };
            let hash = hash_text(text);
            self.files.insert(
                path.to_path_buf(),
                FileEntry {
                    hash,
                    server,
                    language_id,
                    text: text.to_string(),
                    symbols: None,
                    stale: false,
                    retried: false,
                },
            );
            self.queue.push_back(Job::Symbols {
                path: path.to_path_buf(),
                hash,
            });
        }
        self.changed = true;
        self.pump(manager);
    }

    /// Takes an answer of the language servers if it belongs to the
    /// indexer; other answers are handed back.
    pub fn receive(
        &mut self,
        manager: &mut LspManager,
        id: RequestId,
        response: LspResponse,
    ) -> Option<LspResponse> {
        let Some(job) = self.in_flight.remove(&id) else {
            return Some(response);
        };
        let (path, hash) = job.file();
        let root = self.root.clone().unwrap_or_default();
        if let Some(entry) = self.files.get_mut(path).filter(|e| e.hash == hash) {
            if matches!(
                manager.status(entry.server, &root),
                Some(ServerStatus::Starting | ServerStatus::Indexing)
            ) {
                entry.stale = true;
            }
            match (&job, response) {
                (Job::Symbols { path, hash }, LspResponse::Symbols(Ok(symbols))) => {
                    let picked: Vec<SymbolEntry> = pick_symbols(&symbols)
                        .into_iter()
                        .map(|s| SymbolEntry {
                            symbol: s.clone(),
                            label: s.kind.label().to_string(),
                            definition: LineSpan::of_range(s.range),
                            position: s.selection_range.start,
                            name_end: s.selection_range.end,
                            references: None,
                        })
                        .collect();
                    for index in 0..picked.len() {
                        self.queue.push_back(Job::References {
                            path: path.clone(),
                            hash: *hash,
                            index,
                        });
                    }
                    entry.symbols = Some(Ok(picked));
                }
                (Job::Symbols { .. }, LspResponse::Symbols(Err(err))) => {
                    entry.symbols = Some(Err(err));
                }
                (Job::References { index, .. }, LspResponse::References(result)) => {
                    if let Some(Ok(symbols)) = &mut entry.symbols
                        && let Some(symbol) = symbols.get_mut(*index)
                    {
                        symbol.references = Some(result.unwrap_or_else(|err| {
                            log::debug!("references failed: {err}");
                            Vec::new()
                        }));
                    }
                }
                _ => {}
            }
            self.changed = true;
        }
        self.pump(manager);
        None
    }

    /// Asks again, once, the files answered while their server was busy
    /// starting or indexing, now that it is ready.
    pub fn tick(&mut self, manager: &mut LspManager) {
        let Some(root) = self.root.clone() else {
            return;
        };
        for (path, entry) in &mut self.files {
            if !entry.stale || entry.retried || !entry.answered() {
                continue;
            }
            let status = manager.status(entry.server, &root);
            if matches!(status, Some(ServerStatus::Failed(_)) | None) {
                entry.stale = false;
            } else if status == Some(ServerStatus::Ready) {
                entry.stale = false;
                entry.retried = true;
                entry.symbols = None;
                self.queue.push_back(Job::Symbols {
                    path: path.clone(),
                    hash: entry.hash,
                });
            }
        }
        self.pump(manager);
    }

    /// Sends queued requests while their server has room.
    fn pump(&mut self, manager: &mut LspManager) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let mut busy: HashMap<ServerId, usize> = HashMap::new();
        for job in self.in_flight.values() {
            if let Some(entry) = self.files.get(job.file().0) {
                *busy.entry(entry.server).or_default() += 1;
            }
        }
        let mut waiting = VecDeque::new();
        while let Some(job) = self.queue.pop_front() {
            let (path, hash) = job.file();
            let Some(entry) = self.files.get(path).filter(|e| e.hash == hash) else {
                continue;
            };
            let count = busy.entry(entry.server).or_default();
            if *count >= MAX_IN_FLIGHT {
                waiting.push_back(job);
                continue;
            }
            let request = match &job {
                Job::Symbols { path, .. } => LspRequest::Symbols {
                    path: path.clone(),
                    language_id: entry.language_id,
                    text: entry.text.clone(),
                },
                Job::References { path, index, .. } => {
                    let Some(Ok(symbols)) = &entry.symbols else { continue };
                    let Some(symbol) = symbols.get(*index) else { continue };
                    LspRequest::References {
                        path: path.clone(),
                        language_id: entry.language_id,
                        text: entry.text.clone(),
                        position: symbol.position,
                    }
                }
            };
            *count += 1;
            let id = manager.request(None, entry.server, &root, request);
            self.in_flight.insert(id, job);
        }
        self.queue = waiting;
    }

    /// True while requests are queued or on their way, or files answered by a
    /// busy server wait to be asked again.
    pub fn is_waiting(&self) -> bool {
        !self.queue.is_empty()
            || !self.in_flight.is_empty()
            || self.files.values().any(|e| e.stale && !e.retried)
    }

    /// True once after the analysis changed (new answers, new files).
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// Names on one displayed line. Definitions are available even when there
    /// are no cross-file edges; references retain the server's exact ranges
    /// (including import aliases). Called on a canvas click, not every frame.
    pub fn symbol_occurrences(&self, path: &Path, line: usize) -> Vec<SymbolOccurrence<'_>> {
        let Some(source) = self.files.get(path) else { return Vec::new() };
        let canonical = std::fs::canonicalize(path).ok();
        let same_path = |other: &Path| other == path || canonical.as_deref() == Some(other);
        let mut out = Vec::new();
        for (definition_path, entry) in &self.files {
            let Some(Ok(symbols)) = &entry.symbols else { continue };
            for symbol in symbols {
                let Some(definition_name_span) = Self::name_byte_span(
                    &entry.text, symbol.position, symbol.name_end,
                ) else { continue };
                let mut push = |name_span| out.push(SymbolOccurrence {
                    definition_path,
                    symbol: &symbol.symbol,
                    name_span,
                    definition_name_span,
                });
                if same_path(definition_path) && symbol.position.line == line {
                    push(definition_name_span);
                }
                for reference in symbol.references.iter().flatten() {
                    if reference.range.start.line == line && same_path(&reference.path)
                        && let Some(span) = Self::name_byte_span(
                            &source.text, reference.range.start, reference.range.end,
                        )
                    {
                        push(span);
                    }
                }
            }
        }
        // A declaration may also be returned as a reference by the server.
        out.sort_by_key(|o| (o.name_span, o.definition_path, o.definition_name_span));
        out.dedup_by(|a, b| a.name_span == b.name_span
            && a.definition_path == b.definition_path
            && a.definition_name_span == b.definition_name_span);
        out
    }

    /// Every link between the open files known so far.
    pub fn links(&self, open: &OpenFiles) -> Vec<DependencyLink> {
        let mut finder = StatementFinder::default();
        let mut targets: Vec<(&PathBuf, &FileEntry)> = self.files.iter().collect();
        targets.sort_by_key(|(path, _)| *path);
        let mut out: Vec<_> = self.local_links.iter().filter(|l| open.get(&l.source).is_some() && open.get(&l.target).is_some()).cloned().collect();
        for (target, entry) in targets {
            let Some(Ok(symbols)) = &entry.symbols else { continue };
            if open.get(target).is_none() {
                continue;
            }
            for symbol in symbols {
                let Some(references) = &symbol.references else { continue };
                let name_span = Self::name_byte_span(
                    &entry.text,
                    symbol.position,
                    symbol.name_end,
                );
                for usage in usage_occurrences(target, references, open, &mut finder) {
                    let relationship = open.get(&usage.source).map(|(_, text)| finder.relationship_at(&usage.source, text, usage.range.start.line, usage.range.start.character)).unwrap_or_default();
                    if relationship == crate::edges::style::RelationshipType::Imports
                        && out.iter().any(|l| l.relationship == relationship && l.source == usage.source && l.target == *target && l.usage == usage.span) {
                        continue;
                    }
                    let source_label = self.enclosing_symbol_label(&usage.source, usage.range.start.line);
                    let source_name_span = open.get(&usage.source).and_then(|(_, text)|
                        Self::name_byte_span(text, usage.range.start, usage.range.end));
                    out.push(DependencyLink {
                        evidence: super::LinkEvidence::LanguageServer,
                        relationship,
                        source: usage.source,
                        target: target.clone(),
                        label: symbol.label.clone(),
                        source_label,
                        usage: usage.span,
                        definition: symbol.definition,
                        name_span,
                        source_name_span,
                    });
                }
            }
        }
        out
    }

    /// Kind label of the picked symbol in `source` whose definition encloses
    /// `line`, for the bi-color gradient. Innermost wins (a method inside a
    /// struct), falling back to the enclosing top-level symbol.
    fn enclosing_symbol_label(&self, source: &Path, line: usize) -> Option<String> {
        let entry = self.files.get(source)?;
        let Some(Ok(symbols)) = &entry.symbols else {
            return None;
        };
        let mut best: Option<&SymbolEntry> = None;
        for sym in symbols {
            if sym.definition.start <= line && line <= sym.definition.end {
                let span = sym.definition.end.saturating_sub(sym.definition.start);
                let best_span = best
                    .map(|b| b.definition.end.saturating_sub(b.definition.start))
                    .unwrap_or(usize::MAX);
                if span <= best_span {
                    best = Some(sym);
                }
            }
        }
        best.map(|s| s.label.clone())
    }

    /// Converts the symbol's selection_range (UTF-16 positions) to
    /// `(line, start_byte_in_line, end_byte_in_line)`.
    fn name_byte_span(
        content: &str,
        start: Position,
        end: Position,
    ) -> Option<(usize, usize, usize)> {
        if start.line != end.line {
            return None;
        }
        let line = start.line;
        let abs_start = byte_offset(content, line, start.character)?;
        let abs_end = byte_offset(content, line, end.character)?;
        let line_start = byte_offset(content, line, 0)?;
        Some((line, abs_start - line_start, abs_end - line_start))
    }

    /// Progress per language server, in [`ServerId`] order.
    pub fn progress(&self, manager: &LspManager) -> Vec<ServerProgress> {
        let mut out: Vec<ServerProgress> = Vec::new();
        for entry in self.files.values() {
            let i = match out.iter().position(|p| p.server == entry.server) {
                Some(i) => i,
                None => {
                    let status = self.root.as_deref().and_then(|r| manager.status(entry.server, r));
                    out.push(ServerProgress {
                        server: entry.server,
                        files: 0,
                        done: 0,
                        status,
                        error: None,
                    });
                    out.len() - 1
                }
            };
            let p = &mut out[i];
            p.files += 1;
            if entry.answered() {
                p.done += 1;
            }
            if let Some(Err(err)) = &entry.symbols {
                p.error.get_or_insert_with(|| err.clone());
            }
        }
        out.sort_by_key(|p| p.server);
        out
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    /// A manager whose servers never start: every request fails at once.
    fn failing_manager() -> LspManager {
        LspManager::with_launcher(Arc::new(|_, _| Err("no server".to_string())))
    }

    fn drain(indexer: &mut DependencyIndexer, manager: &mut LspManager) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while indexer.is_waiting() {
            assert!(std::time::Instant::now() < deadline, "indexer never settled");
            for (id, response) in manager.poll() {
                assert!(indexer.receive(manager, id, response).is_none());
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn failures_are_reported_and_unknown_answers_handed_back() {
        let mut manager = failing_manager();
        let mut indexer = DependencyIndexer::default();
        let root = Path::new("/p");
        let files = [
            (Path::new("/p/a.rs"), "fn a() {}\n"),
            (Path::new("/p/b.rs"), "fn b() {}\n"),
            (Path::new("/p/README.md"), "# no server\n"),
        ];
        indexer.rebuild(&mut manager, root, &files);
        assert!(indexer.is_waiting());
        drain(&mut indexer, &mut manager);

        let progress = indexer.progress(&manager);
        assert_eq!(progress.len(), 1, "markdown has no server");
        assert_eq!((progress[0].server, progress[0].files, progress[0].done), (ServerId::Rust, 2, 2));
        assert_eq!(progress[0].error.as_deref(), Some("no server"));
        assert!(indexer.links(&OpenFiles::new(files)).is_empty());

        let foreign = LspResponse::Symbols(Ok(Vec::new()));
        assert_eq!(indexer.receive(&mut manager, 9999, foreign.clone()), Some(foreign));
    }

    #[test]
    fn rebuild_asks_only_about_new_or_changed_files_and_forgets_closed_ones() {
        let mut manager = failing_manager();
        let mut indexer = DependencyIndexer::default();
        let root = Path::new("/p");
        indexer.rebuild(&mut manager, root, &[(Path::new("/p/a.rs"), "1")]);
        drain(&mut indexer, &mut manager);
        assert!(indexer.take_changed());
        assert!(!indexer.take_changed());

        indexer.rebuild(&mut manager, root, &[(Path::new("/p/a.rs"), "1"), (Path::new("/p/b.rs"), "2")]);
        assert_eq!(indexer.in_flight.len() + indexer.queue.len(), 1, "a.rs is cached");
        drain(&mut indexer, &mut manager);

        indexer.rebuild(&mut manager, root, &[(Path::new("/p/b.rs"), "changed")]);
        assert_eq!(indexer.files.len(), 1, "a.rs closed");
        assert_eq!(indexer.in_flight.len() + indexer.queue.len(), 1, "b.rs changed");
        drain(&mut indexer, &mut manager);

        indexer.rebuild(&mut manager, Path::new("/other"), &[]);
        assert!(indexer.files.is_empty(), "another project starts over");
    }

    #[test]
    fn at_most_max_in_flight_requests_per_server() {
        let mut manager = failing_manager();
        let mut indexer = DependencyIndexer::default();
        let names: Vec<String> = (0..10).map(|i| format!("/p/f{i}.rs")).collect();
        let files: Vec<(&Path, &str)> = names.iter().map(|n| (Path::new(n.as_str()), "x")).collect();
        indexer.rebuild(&mut manager, Path::new("/p"), &files);
        assert_eq!(indexer.in_flight.len(), MAX_IN_FLIGHT);
        assert_eq!(indexer.queue.len(), 10 - MAX_IN_FLIGHT);
        drain(&mut indexer, &mut manager);
        assert_eq!(indexer.progress(&manager)[0].done, 10);
    }

    #[test]
    fn occurrence_picking_uses_exact_utf16_reference_ranges_even_for_an_alias() {
        use crate::analysis::lsp::{Range, SymbolKind};
        let range = |start, end| Range {
            start: Position { line: 0, character: start },
            end: Position { line: 0, character: end },
        };
        let symbol = LspSymbol {
            name: "greet".into(), detail: None, kind: SymbolKind::Function,
            range: range(0, 26), selection_range: range(16, 21), children: vec![],
        };
        let definition = PathBuf::from("/p/definition.ts");
        let source = PathBuf::from("/p/use.ts");
        let source_text = "\"😀\"; saluer(); // saluer is an alias";
        let file = |text: &str, symbols| FileEntry {
            hash: hash_text(text), server: ServerId::TypeScript, language_id: "typescript",
            text: text.into(), symbols: Some(Ok(symbols)), stale: false, retried: false,
        };
        let mut indexer = DependencyIndexer::default();
        indexer.files.insert(definition.clone(), file("export function greet() {}", vec![SymbolEntry {
            symbol: symbol.clone(), label: "fn".into(), definition: LineSpan { start: 0, end: 0 },
            position: symbol.selection_range.start, name_end: symbol.selection_range.end,
            references: Some(vec![
                Location { path: source.clone(), range: range(6, 12) },
                Location { path: definition.clone(), range: range(16, 21) },
            ]),
        }]));
        indexer.files.insert(source.clone(), file(source_text, vec![]));
        let occurrences = indexer.symbol_occurrences(&source, 0);
        assert_eq!(occurrences.len(), 1, "the spelling in the comment is not an LSP occurrence");
        let occurrence = &occurrences[0];
        assert_eq!(occurrence.symbol.name, "greet");
        let (_, start, end) = occurrence.name_span;
        assert_eq!(&source_text[start..end], "saluer", "UTF-16 is converted after the emoji");
        assert_eq!(occurrence.definition_path, definition);
        assert_eq!(indexer.symbol_occurrences(&definition, 0).len(), 1, "declaration reference is deduplicated");
        assert!(indexer.symbol_occurrences(&source, 1).is_empty());
        let open = OpenFiles::new(indexer.files.iter().map(|(path, entry)| (path.as_path(), entry.text.as_str())));
        let links = indexer.links(&open);
        let (_, start, end) = links[0].source_name_span.unwrap();
        assert_eq!(&source_text[start..end], "saluer", "edge carries the exact UTF-16 alias reference range");
    }
}
