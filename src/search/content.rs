//! Text search inside the folder's files. Reads from disk, so run it on a
//! worker thread.

use std::path::{Path, PathBuf};

use crate::fs::loader::read_file_content;

/// Stop after this many matching lines.
pub const MAX_HITS: usize = 500;
/// Matched lines are cut to this many characters around the match.
const MAX_LINE_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentHit {
    pub path: PathBuf,
    /// 0-based line number.
    pub line: usize,
    /// The line, trimmed and shortened.
    pub text: String,
    /// Character range of the match within `text`.
    pub range: (usize, usize),
}

/// Lines containing `query`, case-insensitively, in file order. Files that
/// cannot be read as text are skipped. Returns the hits and whether the list
/// was cut at [`MAX_HITS`].
pub fn search_content(files: &[PathBuf], query: &str) -> (Vec<ContentHit>, bool) {
    let needle: Vec<char> = query.trim().chars().flat_map(|c| c.to_lowercase()).collect();
    let mut hits = Vec::new();
    if needle.is_empty() {
        return (hits, false);
    }
    for path in files {
        let Ok(text) = read_file_content(path) else { continue };
        for (line, raw) in text.lines().enumerate() {
            if let Some(hit) = match_line(path, line, raw, &needle) {
                hits.push(hit);
                if hits.len() >= MAX_HITS {
                    return (hits, true);
                }
            }
        }
    }
    (hits, false)
}

fn match_line(path: &Path, line: usize, raw: &str, needle: &[char]) -> Option<ContentHit> {
    let lower: Vec<char> = raw.chars().flat_map(|c| c.to_lowercase()).collect();
    // Lowercasing can change the length for a few characters; skip those
    // lines rather than point at the wrong place.
    if lower.len() != raw.chars().count() {
        return None;
    }
    let start = lower.windows(needle.len()).position(|w| w == needle)?;
    let chars: Vec<char> = raw.chars().collect();
    let lead = chars.iter().take_while(|c| c.is_whitespace()).count().min(start);
    let from = lead.max(start.saturating_sub(MAX_LINE_CHARS / 2));
    let to = (from + MAX_LINE_CHARS).min(chars.len());
    let text: String = chars[from..to].iter().collect();
    Some(ContentHit {
        path: path.to_path_buf(),
        line,
        text,
        range: (start - from, (start - from + needle.len()).min(to - from)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, text).unwrap();
        p
    }

    #[test]
    fn finds_lines_case_insensitively_with_line_numbers() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(dir.path(), "a.rs", "fn main() {}\n    println!(\"Hello\");\n");
        let (hits, cut) = search_content(std::slice::from_ref(&a), "HELLO");
        assert!(!cut);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 1);
        assert_eq!(hits[0].path, a);
        let (s, e) = hits[0].range;
        assert_eq!(hits[0].text.chars().skip(s).take(e - s).collect::<String>(), "Hello");
    }

    #[test]
    fn skips_binary_files_and_empty_queries() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("b.bin");
        std::fs::write(&bin, b"hello\0\0").unwrap();
        assert!(search_content(&[bin], "hello").0.is_empty());
        let a = write(dir.path(), "a.txt", "hello");
        assert!(search_content(&[a], "  ").0.is_empty());
    }

    #[test]
    fn results_are_capped() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(dir.path(), "a.txt", &"x\n".repeat(MAX_HITS + 10));
        let (hits, cut) = search_content(&[a], "x");
        assert_eq!(hits.len(), MAX_HITS);
        assert!(cut);
    }
}
