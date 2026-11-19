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

/// The Search panel's filters: `.*` regular expression, `Aa` case sensitive
/// and `Word` whole words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Options {
    pub regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
}

/// The pattern of `query` under `options`; `Err` explains an invalid regex.
pub fn build_regex(query: &str, options: Options) -> Result<regex::Regex, String> {
    let body = if options.regex { query.to_string() } else { regex::escape(query) };
    let body = if options.whole_word { format!(r"\b(?:{body})\b") } else { body };
    regex::RegexBuilder::new(&body)
        .case_insensitive(!options.case_sensitive)
        .build()
        .map_err(|e| e.to_string().lines().last().unwrap_or("invalid pattern").trim().to_string())
}

/// [`search_content`] with the filters of [`Options`]. An invalid regex is
/// an error message instead of hits.
pub fn search_content_with(files: &[PathBuf], query: &str, options: Options) -> Result<(Vec<ContentHit>, bool), String> {
    if query.trim().is_empty() {
        return Ok((Vec::new(), false));
    }
    let re = build_regex(query.trim(), options)?;
    let mut hits = Vec::new();
    for path in files {
        let Ok(text) = read_file_content(path) else { continue };
        for (line, raw) in text.lines().enumerate() {
            let Some(m) = re.find(raw) else { continue };
            if m.as_str().is_empty() {
                continue;
            }
            let start = raw[..m.start()].chars().count();
            let end = start + m.as_str().chars().count();
            hits.push(cut_line(path, line, raw, start, end));
            if hits.len() >= MAX_HITS {
                return Ok((hits, true));
            }
        }
    }
    Ok((hits, false))
}

/// The hit for the match at characters `start..end` of `raw`: the line
/// trimmed and shortened around it.
fn cut_line(path: &Path, line: usize, raw: &str, start: usize, end: usize) -> ContentHit {
    let chars: Vec<char> = raw.chars().collect();
    let lead = chars.iter().take_while(|c| c.is_whitespace()).count().min(start);
    let from = lead.max(start.saturating_sub(MAX_LINE_CHARS / 2));
    let to = (from + MAX_LINE_CHARS).min(chars.len());
    ContentHit {
        path: path.to_path_buf(),
        line,
        text: chars[from..to].iter().collect(),
        range: (start - from, (end - from).min(to - from)),
    }
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
    Some(cut_line(path, line, raw, start, start + needle.len()))
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
    fn filters_regex_case_and_whole_word() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(dir.path(), "a.txt", "Hello world\nhelloworld\nfoo123 bar\n");
        let files = [a];
        let find = |q: &str, o: Options| search_content_with(&files, q, o).unwrap().0.iter().map(|h| h.line).collect::<Vec<_>>();
        assert_eq!(find("hello", Options::default()), [0, 1], "case-insensitive literal");
        assert_eq!(find("hello", Options { case_sensitive: true, ..Default::default() }), [1], "Aa");
        assert_eq!(find("hello", Options { whole_word: true, ..Default::default() }), [0], "Word");
        assert_eq!(find(r"foo\d+", Options { regex: true, ..Default::default() }), [2], ".*");
        assert_eq!(find("foo.", Options::default()), Vec::<usize>::new(), "a literal dot is not a wildcard");
        let hit = &search_content_with(&files, r"w\w+d", Options { regex: true, ..Default::default() }).unwrap().0[0];
        let (s, e) = hit.range;
        assert_eq!(hit.text.chars().skip(s).take(e - s).collect::<String>(), "world");
    }

    #[test]
    fn an_invalid_regex_is_an_error_message() {
        let err = search_content_with(&[], "(", Options { regex: true, ..Default::default() }).unwrap_err();
        assert!(!err.is_empty(), "{err}");
        assert_eq!(search_content_with(&[], "  ", Options::default()).unwrap(), (vec![], false));
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
