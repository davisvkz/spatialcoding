//! An in-process fake language server for the LSP tab tests.
//!
//! It speaks real LSP over pipes, but understands little:
//! - symbols are the `fn`, `struct`, `function`, `class` and `const`
//!   declarations found with a regex, as one flat list;
//! - references are whole-word matches of the word under the cursor in every
//!   `.rs` and `.ts` file under the workspace root.
//!
//! Plug it in with `state.lsp.manager.set_launcher(fake_lsp::launcher())`.

use std::collections::HashMap;
use std::io::{BufReader, PipeWriter};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use regex::Regex;
use serde_json::{json, Value};
use spatialcoding::analysis::lsp::client::Connection;
use spatialcoding::analysis::lsp::manager::Launcher;
use spatialcoding::analysis::lsp::protocol::{
    path_to_uri, read_message, uri_to_path, write_message,
};

pub fn launcher() -> Launcher {
    Arc::new(|_server, root| Ok(start(root.to_path_buf())))
}

fn start(root: PathBuf) -> Connection {
    let (client_reader, server_writer) = std::io::pipe().unwrap();
    let (server_reader, client_writer) = std::io::pipe().unwrap();
    thread::spawn(move || serve(root, BufReader::new(server_reader), server_writer));
    Connection {
        reader: Box::new(client_reader),
        writer: Box::new(client_writer),
        child: None,
        stderr: None,
    }
}

/// The capabilities the client declares in `initialize`: every request it
/// sends (definition, typeDefinition, implementation, call hierarchy,
/// references, document symbols, hover) with link support where it parses
/// links, and `workspace/configuration`.
fn check_declared_capabilities(params: &Value) -> Result<(), String> {
    let text_document = &params["capabilities"]["textDocument"];
    for request in ["documentSymbol", "references", "hover", "callHierarchy"] {
        if !text_document[request].is_object() {
            return Err(format!("textDocument.{request} is not declared"));
        }
    }
    for request in ["definition", "typeDefinition", "implementation"] {
        if text_document[request]["linkSupport"] != true {
            return Err(format!("textDocument.{request}.linkSupport is not declared"));
        }
    }
    if params["capabilities"]["workspace"]["configuration"] != true {
        return Err("workspace.configuration is not declared".into());
    }
    Ok(())
}

fn serve(root: PathBuf, mut reader: BufReader<std::io::PipeReader>, mut writer: PipeWriter) {
    let mut open: HashMap<PathBuf, String> = HashMap::new();
    while let Ok(Some(msg)) = read_message(&mut reader) {
        let params = &msg["params"];
        let doc_path = params["textDocument"]["uri"].as_str().and_then(uri_to_path);
        let text_of = |open: &HashMap<PathBuf, String>, p: &Path| {
            open.get(p)
                .cloned()
                .unwrap_or_else(|| std::fs::read_to_string(p).unwrap_or_default())
        };
        let result = match msg["method"].as_str() {
            Some("initialize") => {
                // A client that calls a request without declaring it fails every
                // test that starts this server, with the reason as the error.
                if let Err(missing) = check_declared_capabilities(params) {
                    let error = json!({"code": -32600, "message": format!("initialize: {missing}")});
                    let _ = write_message(&mut writer, &json!({"jsonrpc": "2.0", "id": msg["id"], "error": error}));
                    continue;
                }
                json!({"capabilities": {
                    "documentSymbolProvider": true, "referencesProvider": true, "definitionProvider": true,
                    "callHierarchyProvider": true,
                }})
            }
            Some("textDocument/didOpen") => {
                open.insert(
                    doc_path.unwrap(),
                    params["textDocument"]["text"].as_str().unwrap().into(),
                );
                continue;
            }
            Some("textDocument/didChange") => {
                open.insert(
                    doc_path.unwrap(),
                    params["contentChanges"][0]["text"].as_str().unwrap().into(),
                );
                continue;
            }
            Some("textDocument/documentSymbol") => symbols(&text_of(&open, &doc_path.unwrap())),
            Some("textDocument/references") => {
                let path = doc_path.unwrap();
                let line = params["position"]["line"].as_u64().unwrap() as usize;
                let col = params["position"]["character"].as_u64().unwrap() as usize;
                match word_at(&text_of(&open, &path), line, col) {
                    Some(word) => references(&root, &word),
                    None => json!([]),
                }
            }
            Some("textDocument/definition") => {
                let path = doc_path.unwrap();
                let line = params["position"]["line"].as_u64().unwrap() as usize;
                let col = params["position"]["character"].as_u64().unwrap() as usize;
                match word_at(&text_of(&open, &path), line, col) {
                    Some(word) => definition(&root, &word),
                    None => json!([]),
                }
            }
            Some("textDocument/prepareCallHierarchy") => {
                let path = doc_path.unwrap();
                let line = params["position"]["line"].as_u64().unwrap() as usize;
                let col = params["position"]["character"].as_u64().unwrap() as usize;
                match word_at(&text_of(&open, &path), line, col) {
                    Some(word) => call_item(&root, &word).map_or(json!([]), |item| json!([item])),
                    None => json!([]),
                }
            }
            Some("callHierarchy/incomingCalls") => incoming_calls(&root, params["item"]["name"].as_str().unwrap_or("")),
            Some("callHierarchy/outgoingCalls") => json!([]),
            Some("exit") => break,
            _ if msg.get("id").is_some() => Value::Null,
            _ => continue,
        };
        let reply = json!({"jsonrpc": "2.0", "id": msg["id"], "result": result});
        if write_message(&mut writer, &reply).is_err() {
            break;
        }
    }
}

fn range(line: usize, start: usize, end: usize) -> Value {
    json!({"start": {"line": line, "character": start}, "end": {"line": line, "character": end}})
}

fn symbols(text: &str) -> Value {
    let re = Regex::new(r"^\s*(?:pub\s+)?(?:export\s+)?(fn|struct|function|class|const)\s+(\w+)")
        .unwrap();
    let items: Vec<Value> = text
        .lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let caps = re.captures(line)?;
            let name = caps.get(2).unwrap();
            let kind = match &caps[1] {
                "fn" | "function" => 12,
                "struct" => 23,
                "class" => 5,
                _ => 14,
            };
            Some(json!({
                "name": name.as_str(), "kind": kind,
                "range": range(i, 0, line.len()),
                "selectionRange": range(i, name.start(), name.end()),
            }))
        })
        .collect();
    Value::Array(items)
}

fn word_at(text: &str, line: usize, col: usize) -> Option<String> {
    let line = text.lines().nth(line)?;
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let start = line[..col.min(line.len())]
        .rfind(|c: char| !is_word(c))
        .map_or(0, |i| i + 1);
    let end = line[start..]
        .find(|c: char| !is_word(c))
        .map_or(line.len(), |i| start + i);
    (start < end).then(|| line[start..end].to_string())
}

fn references(root: &Path, word: &str) -> Value {
    let re = Regex::new(&format!(r"\b{}\b", regex::escape(word))).unwrap();
    let mut files = Vec::new();
    collect(root, &mut files);
    let mut out = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        for (i, line) in text.lines().enumerate() {
            for m in re.find_iter(line) {
                out.push(json!({"uri": path_to_uri(&file), "range": range(i, m.start(), m.end())}));
            }
        }
    }
    Value::Array(out)
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e == "rs" || e == "ts" || e == "tsx")
        {
            out.push(path);
        }
    }
}

/// Where `word` is declared: a `LocationLink` with the declaration line and the name.
fn definition(root: &Path, word: &str) -> Value {
    let re = Regex::new(&format!(r"^\s*(?:pub\s+)?(?:export\s+)?(?:fn|struct|function|class|const)\s+({})\b", regex::escape(word))).unwrap();
    let mut files = Vec::new();
    collect(root, &mut files);
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        for (i, line) in text.lines().enumerate() {
            if let Some(caps) = re.captures(line) {
                let name = caps.get(1).unwrap();
                return json!([{
                    "targetUri": path_to_uri(&file),
                    "targetRange": range(i, 0, line.len()),
                    "targetSelectionRange": range(i, name.start(), name.end()),
                }]);
            }
        }
    }
    json!([])
}

/// The `CallHierarchyItem` of the function `word`, if it is declared.
fn call_item(root: &Path, word: &str) -> Option<Value> {
    let link = definition(root, word);
    let link = link.as_array()?.first()?.clone();
    Some(json!({
        "name": word, "kind": 12, "uri": link["targetUri"],
        "range": link["targetRange"], "selectionRange": link["targetSelectionRange"],
    }))
}

/// The functions that mention `word` outside its declaration, as callers.
fn incoming_calls(root: &Path, word: &str) -> Value {
    let re = Regex::new(&format!(r"\b{}\b", regex::escape(word))).unwrap();
    let decl = Regex::new(r"^\s*(?:pub\s+)?(?:export\s+)?(?:fn|function)\s+(\w+)").unwrap();
    let mut files = Vec::new();
    collect(root, &mut files);
    let mut out = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if decl.captures(line).is_some_and(|c| &c[1] == word) {
                continue;
            }
            let Some(m) = re.find(line) else { continue };
            // The enclosing function: the nearest declaration above.
            let Some((start, caps)) = (0..=i).rev().find_map(|j| decl.captures(lines[j]).map(|c| (j, c))) else { continue };
            let name = caps.get(1).unwrap();
            out.push(json!({
                "from": {
                    "name": name.as_str(), "kind": 12, "uri": path_to_uri(&file),
                    "range": range(start, 0, lines[start].len()),
                    "selectionRange": range(start, name.start(), name.end()),
                },
                "fromRanges": [range(i, m.start(), m.end())],
            }));
        }
    }
    Value::Array(out)
}
