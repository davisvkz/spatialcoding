//! Agent notes: markdown cards an MCP agent pushes to explain the code. A
//! note links to the canvas with `cc://` URLs (`codecanvas/funcionalidades.md`):
//!
//! - `cc://file/<path>`
//! - `cc://folder/<dir>`
//! - `cc://symbol/<path>:LINE:CHAR`
//! - `cc://range/<path>:SL:SC-EL:EC`
//!
//! Lines and columns are 1-based, as an editor shows them. A relative path is
//! taken from the loaded folder.

use std::path::{Path, PathBuf};

/// One card of the Agent notes panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentNote {
    pub id: u64,
    pub title: String,
    pub markdown: String,
}

/// Where a `cc://` link points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CcLink {
    File(PathBuf),
    Folder(PathBuf),
    /// 0-based line and column of the symbol.
    Symbol { path: PathBuf, line: usize, column: usize },
    /// 0-based `(line, column)` start and end.
    Range { path: PathBuf, start: (usize, usize), end: (usize, usize) },
}

impl CcLink {
    pub fn path(&self) -> &Path {
        match self {
            CcLink::File(p) | CcLink::Folder(p) => p,
            CcLink::Symbol { path, .. } | CcLink::Range { path, .. } => path,
        }
    }

    /// What a hover shows: `src/a.rs:12:4`.
    pub fn preview(&self) -> String {
        match self {
            CcLink::File(p) | CcLink::Folder(p) => p.display().to_string(),
            CcLink::Symbol { path, line, column } => format!("{}:{}:{}", path.display(), line + 1, column + 1),
            CcLink::Range { path, start, end } => {
                format!("{}:{}:{}-{}:{}", path.display(), start.0 + 1, start.1 + 1, end.0 + 1, end.1 + 1)
            }
        }
    }
}

/// Parses a `cc://` URL. A relative path is joined to `root`. `None` for
/// other schemes and malformed positions.
pub fn parse_link(url: &str, root: Option<&Path>) -> Option<CcLink> {
    let rest = url.strip_prefix("cc://")?;
    let (kind, target) = rest.split_once('/')?;
    let target = decode(target);
    let resolve = |p: &str| -> PathBuf {
        let path = PathBuf::from(p);
        match root {
            Some(root) if path.is_relative() => root.join(path),
            _ => path,
        }
    };
    let number = |s: &str| s.parse::<usize>().ok().filter(|n| *n >= 1).map(|n| n - 1);
    match kind {
        "file" => Some(CcLink::File(resolve(&target))),
        "folder" => Some(CcLink::Folder(resolve(&target))),
        "symbol" => {
            let mut parts = target.rsplitn(3, ':');
            let column = number(parts.next()?)?;
            let line = number(parts.next()?)?;
            Some(CcLink::Symbol { path: resolve(parts.next()?), line, column })
        }
        "range" => {
            // path:SL:SC-EL:EC
            let (path, span) = target.rsplit_once(':').and_then(|(rest, ec)| {
                let (rest, el_sc) = rest.rsplit_once('-')?;
                // rest = path:SL:SC? no: split from the left side below.
                Some((rest.to_string(), format!("{el_sc}:{ec}")))
            })?;
            let mut left = path.rsplitn(3, ':');
            let sc = number(left.next()?)?;
            let sl = number(left.next()?)?;
            let file = left.next()?;
            let mut right = span.splitn(2, ':');
            let el = number(right.next()?)?;
            let ec = number(right.next()?)?;
            Some(CcLink::Range { path: resolve(file), start: (sl, sc), end: (el, ec) })
        }
        _ => None,
    }
}

/// Percent-decoding of a link target (`%20`).
fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = s.get(i + 1..i + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Every `cc://` URL of a markdown text, in order: `[text](cc://…)` links
/// and bare URLs.
pub fn links_in(markdown: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = markdown;
    while let Some(at) = rest.find("cc://") {
        let tail = &rest[at..];
        let end = tail
            .find(|c: char| c.is_whitespace() || matches!(c, ')' | ']' | '>' | '"' | '\''))
            .unwrap_or(tail.len());
        found.push(tail[..end].trim_end_matches([',', '.', ';', ':']).to_string());
        rest = &tail[end..];
    }
    found
}

/// The notes of the panel.
#[derive(Debug, Default)]
pub struct Notes {
    notes: Vec<AgentNote>,
    next_id: u64,
    /// The note the panel scrolls to and marks (`agentNotes:open`).
    pub focused: Option<u64>,
}

impl Notes {
    pub fn all(&self) -> &[AgentNote] {
        &self.notes
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    pub fn get(&self, id: u64) -> Option<&AgentNote> {
        self.notes.iter().find(|n| n.id == id)
    }

    /// Adds a note and returns its id.
    pub fn push(&mut self, title: impl Into<String>, markdown: impl Into<String>) -> u64 {
        self.next_id += 1;
        self.notes.push(AgentNote { id: self.next_id, title: title.into(), markdown: markdown.into() });
        self.next_id
    }

    /// Replaces the title and/or text of a note; false when there is none.
    pub fn update(&mut self, id: u64, title: Option<String>, markdown: Option<String>) -> bool {
        let Some(note) = self.notes.iter_mut().find(|n| n.id == id) else { return false };
        if let Some(title) = title {
            note.title = title;
        }
        if let Some(markdown) = markdown {
            note.markdown = markdown;
        }
        true
    }

    pub fn remove(&mut self, id: u64) {
        self.notes.retain(|n| n.id != id);
        if self.focused == Some(id) {
            self.focused = None;
        }
    }

    pub fn clear(&mut self) {
        self.notes.clear();
        self.focused = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> Option<&'static Path> {
        Some(Path::new("/repo"))
    }

    #[test]
    fn file_and_folder_links_resolve_against_the_root() {
        assert_eq!(parse_link("cc://file/src/a.rs", root()), Some(CcLink::File("/repo/src/a.rs".into())));
        assert_eq!(parse_link("cc://folder/src", root()), Some(CcLink::Folder("/repo/src".into())));
        assert_eq!(parse_link("cc://file//abs/b.rs", root()), Some(CcLink::File("/abs/b.rs".into())), "an absolute path stays");
        assert_eq!(parse_link("cc://file/my%20dir/a.rs", root()), Some(CcLink::File("/repo/my dir/a.rs".into())));
    }

    #[test]
    fn symbol_and_range_links_are_one_based_and_become_zero_based() {
        assert_eq!(
            parse_link("cc://symbol/src/a.rs:12:5", root()),
            Some(CcLink::Symbol { path: "/repo/src/a.rs".into(), line: 11, column: 4 })
        );
        assert_eq!(
            parse_link("cc://range/src/a.rs:3:1-5:9", root()),
            Some(CcLink::Range { path: "/repo/src/a.rs".into(), start: (2, 0), end: (4, 8) })
        );
        assert_eq!(parse_link("cc://range/src/a.rs:3:1-5:9", None).unwrap().preview(), "src/a.rs:3:1-5:9");
    }

    #[test]
    fn other_schemes_and_malformed_links_are_none() {
        for bad in ["https://x", "cc://nope/a", "cc://symbol/a.rs:x:1", "cc://symbol/a.rs:0:1", "cc://range/a.rs:1:1", "cc://file"] {
            assert_eq!(parse_link(bad, root()), None, "{bad}");
        }
    }

    #[test]
    fn links_are_found_in_markdown_text() {
        let md = "See [`a`](cc://file/src/a.rs) and cc://symbol/src/b.rs:2:3, then (cc://folder/lib).";
        assert_eq!(links_in(md), ["cc://file/src/a.rs", "cc://symbol/src/b.rs:2:3", "cc://folder/lib"]);
    }

    #[test]
    fn notes_push_update_and_clear() {
        let mut notes = Notes::default();
        let a = notes.push("Overview", "# Hi");
        let b = notes.push("More", "text");
        assert_eq!((a, b), (1, 2));
        assert!(notes.update(a, Some("Overview 2".into()), None));
        assert_eq!(notes.get(a).unwrap().title, "Overview 2");
        assert_eq!(notes.get(a).unwrap().markdown, "# Hi");
        assert!(!notes.update(99, None, None));
        notes.focused = Some(b);
        notes.remove(b);
        assert!(notes.focused.is_none() && notes.all().len() == 1);
        notes.clear();
        assert!(notes.is_empty());
    }
}
