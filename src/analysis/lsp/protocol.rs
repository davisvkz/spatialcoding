//! Pure LSP helpers: `Content-Length` framing, `file://` URIs, and parsing of
//! `documentSymbol` and `references` results.

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{Location, LspSymbol, Position, Range, SymbolKind};

/// Writes one JSON-RPC message with its `Content-Length` header.
pub fn write_message(w: &mut impl Write, msg: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(msg)?;
    write!(w, "Content-Length: {}\r\n\r\n", body.len())?;
    w.write_all(&body)?;
    w.flush()
}

/// Reads one message. `Ok(None)` at a clean end of stream, before any header.
pub fn read_message(r: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut length = None;
    let mut line = String::new();
    let mut first = true;
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 {
            if first {
                return Ok(None);
            }
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "end of stream in header"));
        }
        first = false;
        let header = line.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let length = length
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length"))?;
    let mut body = vec![0; length];
    r.read_exact(&mut body)?;
    serde_json::from_slice(&body).map(Some).map_err(io::Error::from)
}

/// `file://` URI of an absolute path, percent-encoding everything but
/// unreserved characters and `/`.
pub fn path_to_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    let text = path.to_string_lossy();
    for &b in text.as_bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            uri.push(b as char);
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    uri
}

/// Path of a `file://` URI; `None` for other schemes or invalid encodings.
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok().map(PathBuf::from)
}

pub fn position_json(p: Position) -> Value {
    serde_json::json!({ "line": p.line, "character": p.character })
}

fn parse_position(v: &Value) -> Option<Position> {
    Some(Position {
        line: v.get("line")?.as_u64()? as usize,
        character: v.get("character")?.as_u64()? as usize,
    })
}

fn parse_range(v: &Value) -> Option<Range> {
    Some(Range {
        start: parse_position(v.get("start")?)?,
        end: parse_position(v.get("end")?)?,
    })
}

/// Parses a `textDocument/documentSymbol` result: either nested
/// `DocumentSymbol[]` or flat `SymbolInformation[]`. Malformed entries are
/// skipped.
pub fn parse_document_symbols(result: &Value) -> Vec<LspSymbol> {
    let Some(items) = result.as_array() else {
        return Vec::new();
    };
    items.iter().filter_map(parse_symbol).collect()
}

fn parse_symbol(v: &Value) -> Option<LspSymbol> {
    let name = v.get("name")?.as_str()?.to_string();
    let kind = SymbolKind::from_lsp(v.get("kind")?.as_u64()?);
    let detail = v
        .get("detail")
        .and_then(Value::as_str)
        .filter(|d| !d.is_empty())
        .map(String::from);
    if let Some(location) = v.get("location") {
        // SymbolInformation: flat, no selection range.
        let range = parse_range(location.get("range")?)?;
        return Some(LspSymbol {
            name,
            detail,
            kind,
            range,
            selection_range: range,
            children: Vec::new(),
        });
    }
    let range = parse_range(v.get("range")?)?;
    let selection_range = v.get("selectionRange").and_then(parse_range).unwrap_or(range);
    let children = v
        .get("children")
        .map(parse_document_symbols)
        .unwrap_or_default();
    Some(LspSymbol {
        name,
        detail,
        kind,
        range,
        selection_range,
        children,
    })
}

/// Parses a `textDocument/references` (or definition) result: `Location[]`,
/// `LocationLink[]`, a single `Location`, or `null`. Sorted by file and line.
pub fn parse_locations(result: &Value) -> Vec<Location> {
    let items: Vec<&Value> = match result {
        Value::Array(items) => items.iter().collect(),
        Value::Object(_) => vec![result],
        _ => Vec::new(),
    };
    let mut locations: Vec<Location> = items
        .into_iter()
        .filter_map(|v| {
            let (uri, range) = match v.get("uri") {
                Some(uri) => (uri, v.get("range")?),
                None => (
                    v.get("targetUri")?,
                    v.get("targetSelectionRange").or_else(|| v.get("targetRange"))?,
                ),
            };
            Some(Location {
                path: uri_to_path(uri.as_str()?)?,
                range: parse_range(range)?,
            })
        })
        .collect();
    locations.sort();
    locations.dedup();
    locations
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Cursor;

    #[test]
    fn framing_round_trips_several_messages() {
        let mut buf = Vec::new();
        write_message(&mut buf, &json!({"id": 1, "method": "a"})).unwrap();
        write_message(&mut buf, &json!({"text": "ação 🚀"})).unwrap();

        let mut r = Cursor::new(buf);
        assert_eq!(read_message(&mut r).unwrap(), Some(json!({"id": 1, "method": "a"})));
        assert_eq!(read_message(&mut r).unwrap(), Some(json!({"text": "ação 🚀"})));
        assert_eq!(read_message(&mut r).unwrap(), None, "clean end of stream");
    }

    #[test]
    fn read_message_ignores_other_headers_and_header_case() {
        let body = r#"{"ok":true}"#;
        let raw = format!(
            "content-length: {}\r\nContent-Type: application/vscode-jsonrpc; charset=utf-8\r\n\r\n{body}",
            body.len()
        );
        let msg = read_message(&mut Cursor::new(raw)).unwrap();
        assert_eq!(msg, Some(json!({"ok": true})));
    }

    #[test]
    fn read_message_rejects_missing_length_and_truncated_input() {
        let err = read_message(&mut Cursor::new("X-Foo: 1\r\n\r\n{}")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        let err = read_message(&mut Cursor::new("Content-Length: 10\r\n\r\n{}")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
        let err = read_message(&mut Cursor::new("Content-Length: 2\r\n")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn uris_round_trip_spaces_and_non_ascii() {
        let path = Path::new("/tmp/my proj/ação#1.rs");
        let uri = path_to_uri(path);
        assert_eq!(uri, "file:///tmp/my%20proj/a%C3%A7%C3%A3o%231.rs");
        assert_eq!(uri_to_path(&uri).unwrap(), path);
        assert_eq!(uri_to_path("file:///a/b.rs").unwrap(), Path::new("/a/b.rs"));
        assert_eq!(uri_to_path("file://localhost/a").unwrap(), Path::new("/a"));
        assert!(uri_to_path("https://x/a").is_none());
        assert!(uri_to_path("file:///bad%zz").is_none());
    }

    fn range(l0: usize, c0: usize, l1: usize, c1: usize) -> Value {
        json!({"start": {"line": l0, "character": c0}, "end": {"line": l1, "character": c1}})
    }

    #[test]
    fn parses_nested_document_symbols() {
        let result = json!([{
            "name": "S", "kind": 23, "detail": "",
            "range": range(0, 0, 3, 1), "selectionRange": range(0, 7, 0, 8),
            "children": [{
                "name": "x", "kind": 8, "detail": "u32",
                "range": range(1, 4, 1, 10), "selectionRange": range(1, 4, 1, 5)
            }]
        }, {"name": "broken"}]);
        let symbols = parse_document_symbols(&result);
        assert_eq!(symbols.len(), 1, "malformed entries are skipped");
        let s = &symbols[0];
        assert_eq!((s.name.as_str(), s.kind, s.detail.as_deref()), ("S", SymbolKind::Struct, None));
        assert_eq!(s.range.end.line, 3);
        assert_eq!(s.selection_range.start.character, 7);
        let x = &s.children[0];
        assert_eq!((x.name.as_str(), x.kind, x.detail.as_deref()), ("x", SymbolKind::Field, Some("u32")));
    }

    #[test]
    fn parses_flat_symbol_information() {
        let result = json!([{
            "name": "main", "kind": 12, "containerName": "",
            "location": {"uri": "file:///p/main.go", "range": range(4, 0, 6, 1)}
        }]);
        let symbols = parse_document_symbols(&result);
        assert_eq!(symbols[0].name, "main");
        assert_eq!(symbols[0].kind, SymbolKind::Function);
        assert_eq!(symbols[0].selection_range, symbols[0].range);
        assert!(parse_document_symbols(&Value::Null).is_empty());
    }

    #[test]
    fn parses_locations_links_and_null_sorted() {
        let result = json!([
            {"uri": "file:///p/b.rs", "range": range(9, 1, 9, 4)},
            {"targetUri": "file:///p/a.rs", "targetRange": range(0, 0, 5, 0),
             "targetSelectionRange": range(2, 3, 2, 6)},
            {"uri": "file:///p/b.rs", "range": range(1, 1, 1, 4)},
            {"uri": "file:///p/b.rs", "range": range(1, 1, 1, 4)},
        ]);
        let locs = parse_locations(&result);
        let summary: Vec<(String, usize)> = locs
            .iter()
            .map(|l| (l.path.display().to_string(), l.range.start.line))
            .collect();
        assert_eq!(summary, [("/p/a.rs".into(), 2), ("/p/b.rs".into(), 1), ("/p/b.rs".into(), 9)]);

        let single = json!({"uri": "file:///p/c.rs", "range": range(0, 0, 0, 1)});
        assert_eq!(parse_locations(&single).len(), 1);
        assert!(parse_locations(&Value::Null).is_empty());
    }
}
