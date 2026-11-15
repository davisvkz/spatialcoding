//! Classify a `DependencyLink` as `Direct`, `ReExport` or `Barrel`.
//!
//! No new graph: the input is the links the LSP indexer already produced plus
//! the text of their source files. Re-export is detected per-line from the
//! source statement; barrel is a second pass that promotes the re-exports of
//! a file whose name is a conventional index (`index.{ts,tsx,js,jsx,mjs,cjs}`,
//! `mod.rs`, `__init__.py`) once it accumulates [`BARREL_MIN_REEXPORTS`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::analysis::dependencies::{DependencyLink, LineSpan};
use crate::edges::edge_store::EdgeKind;

/// Minimum re-export count for a conventional index file to be considered a
/// barrel. Matches the "typically ≥ 4 re-exports" heuristic in TASK-34.
pub const BARREL_MIN_REEXPORTS: usize = 4;

/// Classifies one link against the text of its source file. Does not look at
/// the target file: the point is "what does the source statement do?".
pub fn classify_link(link: &DependencyLink, source_text: &str) -> EdgeKind {
    if is_reexport_line(&link.source, source_text, link.usage) {
        EdgeKind::ReExport
    } else {
        EdgeKind::Direct
    }
}

/// Promotes every `ReExport` of a barrel source file to `Barrel`. Takes the
/// (kind, source) pairs in link order and returns the updated kinds.
pub fn promote_barrels<'a>(kinds: &[EdgeKind], sources: &'a [&'a Path]) -> Vec<EdgeKind> {
    assert_eq!(kinds.len(), sources.len());
    let mut reexport_count: HashMap<PathBuf, usize> = HashMap::new();
    for (kind, source) in kinds.iter().zip(sources.iter()) {
        if *kind == EdgeKind::ReExport {
            *reexport_count.entry(source.to_path_buf()).or_default() += 1;
        }
    }
    let barrels: std::collections::HashSet<PathBuf> = reexport_count
        .into_iter()
        .filter(|(path, count)| is_barrel_filename(path) && *count >= BARREL_MIN_REEXPORTS)
        .map(|(path, _)| path)
        .collect();
    kinds
        .iter()
        .zip(sources.iter())
        .map(|(kind, source)| {
            if *kind == EdgeKind::ReExport && barrels.contains(*source) {
                EdgeKind::Barrel
            } else {
                *kind
            }
        })
        .collect()
}

fn is_barrel_filename(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    matches!(
        name,
        "index.ts"
            | "index.tsx"
            | "index.js"
            | "index.jsx"
            | "index.mjs"
            | "index.cjs"
            | "mod.rs"
            | "__init__.py"
    )
}

fn is_reexport_line(source_path: &Path, source_text: &str, usage: LineSpan) -> bool {
    let Some(first_line) = source_text.lines().nth(usage.start) else {
        return false;
    };
    let trimmed = first_line.trim_start();
    let ext = source_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    match ext {
        "rs" => {
            // `pub use …` and `pub(crate) use …`.
            trimmed.starts_with("pub use ")
                || trimmed.starts_with("pub(") && trimmed.contains(") use ")
        }
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" => {
            // `export … from '…'` / `export * from '…'` / `export { … } from '…'`
            trimmed.starts_with("export ") && trimmed.contains(" from ")
        }
        "py" => {
            // `from X import Y` counts as a re-export only when the file
            // declares an `__all__` manifest somewhere (conventional barrel).
            trimmed.starts_with("from ")
                && trimmed.contains(" import ")
                && source_text.contains("__all__")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn link(source: &str, usage_line: usize) -> DependencyLink {
        DependencyLink {
            source: PathBuf::from(source),
            target: PathBuf::from("/does/not/matter.rs"),
            label: "fn".into(),
            source_label: None,
            usage: LineSpan {
                start: usage_line,
                end: usage_line,
            },
            definition: LineSpan { start: 0, end: 0 },
            name_span: None,
            source_name_span: None,
        }
    }

    #[test]
    fn rust_pub_use_is_reexport_but_use_alone_is_not() {
        let text = "use std::io;\npub use crate::inner::Thing;\npub(crate) use foo::Bar;\n";
        assert_eq!(classify_link(&link("/p/lib.rs", 0), text), EdgeKind::Direct);
        assert_eq!(
            classify_link(&link("/p/lib.rs", 1), text),
            EdgeKind::ReExport
        );
        assert_eq!(
            classify_link(&link("/p/lib.rs", 2), text),
            EdgeKind::ReExport
        );
    }

    #[test]
    fn typescript_export_from_is_reexport() {
        let text = "import { x } from './a';\nexport { y } from './b';\nexport * from './c';\n";
        assert_eq!(classify_link(&link("/p/x.ts", 0), text), EdgeKind::Direct);
        assert_eq!(classify_link(&link("/p/x.ts", 1), text), EdgeKind::ReExport);
        assert_eq!(classify_link(&link("/p/x.ts", 2), text), EdgeKind::ReExport);
    }

    #[test]
    fn python_reexport_needs_dunder_all() {
        let bare = "from .a import x\nfrom .b import y\n";
        let with_all = "__all__ = ['x', 'y']\nfrom .a import x\nfrom .b import y\n";
        assert_eq!(classify_link(&link("/p/m.py", 0), bare), EdgeKind::Direct);
        assert_eq!(
            classify_link(&link("/p/m.py", 1), with_all),
            EdgeKind::ReExport
        );
    }

    #[test]
    fn barrel_promotes_only_index_like_files_with_four_or_more_reexports() {
        let idx = PathBuf::from("/p/src/index.ts");
        let other = PathBuf::from("/p/src/util.ts");
        let kinds = vec![
            EdgeKind::ReExport,
            EdgeKind::ReExport,
            EdgeKind::ReExport,
            EdgeKind::ReExport,
            EdgeKind::ReExport,
            EdgeKind::Direct,
        ];
        let paths = vec![
            idx.as_path(),
            idx.as_path(),
            idx.as_path(),
            idx.as_path(),
            other.as_path(),
            idx.as_path(),
        ];
        let promoted = promote_barrels(&kinds, &paths);
        assert_eq!(
            promoted,
            vec![
                EdgeKind::Barrel,
                EdgeKind::Barrel,
                EdgeKind::Barrel,
                EdgeKind::Barrel,
                EdgeKind::ReExport, // other: not a barrel filename
                EdgeKind::Direct,
            ]
        );
    }

    #[test]
    fn barrel_needs_the_threshold_count() {
        let idx = PathBuf::from("/p/mod.rs");
        let kinds = vec![EdgeKind::ReExport, EdgeKind::ReExport, EdgeKind::ReExport];
        let paths = vec![idx.as_path(), idx.as_path(), idx.as_path()];
        assert_eq!(promote_barrels(&kinds, &paths), kinds);
    }
}
