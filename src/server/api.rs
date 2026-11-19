//! The tools an agent can use (`codecanvas/mcp-e-servidor-local.md`), as
//! pure functions over [`AppState`]. The MCP server (`--mcp`) forwards each
//! call here through the bridge; tests call it directly.
//!
//! | Tool | What it does |
//! |---|---|
//! | `list_commands` | discovers the commands of the canvas and the UI |
//! | `use_command` | runs one (`canvas:openFiles`, `layout:apply`, `agentNotes:push`…) |
//! | `query` | reads state: canvas, nodes, folders, edges, backwards edges, layout… |
//! | `search` | code, paths and symbols of the folder |
//! | `lsp_servers` | detects and inspects the language servers |
//! | `read_docs` | reads the guide or one of its sections |

use serde_json::{json, Value};

use crate::state::AppState;

/// One MCP tool: name, description and JSON schema of its arguments.
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: fn() -> Value,
}

/// The tools, in the order `tools/list` returns them.
pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "list_commands",
            description: "List the commands of the canvas and the UI (id, title, shortcut, whether they apply now).",
            schema: || json!({"type": "object", "properties": {}}),
        },
        Tool {
            name: "use_command",
            description: "Run a command by id, for example canvas:openAll, layout:apply, viewport:fitNodes, folder:collapseAll or agentNotes:push (arguments {title, markdown}).",
            schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "id": {"type": "string", "description": "Command id from list_commands"},
                        "args": {"type": "object", "description": "Arguments of the command"}
                    },
                    "required": ["id"]
                })
            },
        },
        Tool {
            name: "query",
            description: "Read state. what: canvas, nodes, folders, edges, backwards, layout, viewport, selection, pins, notes or settings. Edges carry source and target paths, relationship, kind, symbol kind and whether they run backwards.",
            schema: || {
                json!({
                    "type": "object",
                    "properties": {"what": {"type": "string", "enum": ["canvas", "nodes", "folders", "edges", "backwards", "layout", "viewport", "selection", "pins", "notes", "settings"]}},
                    "required": ["what"]
                })
            },
        },
        Tool {
            name: "search",
            description: "Search the open folder. mode: content (text in files), paths (file and folder names) or symbols (declarations).",
            schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "query": {"type": "string"},
                        "mode": {"type": "string", "enum": ["content", "paths", "symbols"]},
                        "limit": {"type": "integer"}
                    },
                    "required": ["query"]
                })
            },
        },
        Tool {
            name: "lsp_servers",
            description: "Detect or inspect the language servers. action: detect (which commands are found) or status (running servers of this workspace).",
            schema: || {
                json!({
                    "type": "object",
                    "properties": {"action": {"type": "string", "enum": ["detect", "status"]}}
                })
            },
        },
        Tool {
            name: "read_docs",
            description: "Read the guide of the app, one section of it, or with list=true the list of sections.",
            schema: || {
                json!({
                    "type": "object",
                    "properties": {"section": {"type": "string"}, "list": {"type": "boolean"}}
                })
            },
        },
    ]
}

/// Runs the tool `name`. Errors are plain text for the agent.
pub fn handle(state: &mut AppState, name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "list_commands" => Ok(list_commands(state)),
        "use_command" => {
            let id = args.get("id").and_then(Value::as_str).ok_or("id is required")?;
            let command_args = args.get("args").cloned().unwrap_or(Value::Null);
            crate::commands::run(state, id, &command_args)
        }
        "query" => query(state, args.get("what").and_then(Value::as_str).ok_or("what is required")?),
        "search" => search(state, args),
        "lsp_servers" => lsp_servers(state, args.get("action").and_then(Value::as_str).unwrap_or("detect")),
        "read_docs" => read_docs(args),
        other => Err(format!("unknown tool {other}")),
    }
}

fn list_commands(state: &AppState) -> Value {
    Value::Array(
        crate::commands::all()
            .iter()
            .map(|c| {
                json!({
                    "id": c.id,
                    "title": c.title,
                    "shortcut": c.shortcut_label(),
                    "enabled": (c.enabled)(state),
                })
            })
            .collect(),
    )
}

fn path_of(state: &AppState, path: &std::path::Path) -> String {
    let root = state.loaded_dir.as_deref();
    root.and_then(|r| path.strip_prefix(r).ok()).unwrap_or(path).to_string_lossy().into_owned()
}

fn query(state: &AppState, what: &str) -> Result<Value, String> {
    let store = &state.node_store;
    let (node_width, font) = (state.settings.node_width, state.settings.font_size);
    let node_rect = |id: u64| store.get_file(id).map(|n| n.world_rect(node_width, font));
    Ok(match what {
        "canvas" => json!({
            "folder": state.loaded_dir.as_ref().map(|d| d.display().to_string()),
            "nodes": store.file_nodes.len(),
            "folders": store.folder_nodes.len(),
            "edges": state.edge_store.all_edges().count(),
            "selected": state.selection.count(),
            "zoom": state.viewport.zoom,
            "level": crate::nodes::lod::LodLevel::from_zoom(state.viewport.zoom).number(),
            "pins": state.lsp.pins.len(),
            "notes": state.notes.all().len(),
        }),
        "nodes" => {
            let mut nodes: Vec<&crate::nodes::file_node::FileNode> = store.file_nodes.values().collect();
            nodes.sort_by_key(|n| n.id);
            Value::Array(
                nodes
                    .into_iter()
                    .map(|n| {
                        let rect = n.world_rect(node_width, font);
                        json!({
                            "id": n.id,
                            "path": path_of(state, &n.path),
                            "x": rect.min.x, "y": rect.min.y, "width": rect.width(), "height": rect.height(),
                            "lines": n.line_count,
                            "collapsed": n.collapsed,
                            "hidden": store.is_file_hidden(n.id),
                            "selected": state.selection.is_selected(n.id),
                            "changed": state.changes.is_changed(&n.path),
                        })
                    })
                    .collect(),
            )
        }
        "folders" => {
            let root = state.loaded_dir.clone().unwrap_or_default();
            let mut folders: Vec<_> = store.folder_nodes.values().collect();
            folders.sort_by_key(|f| f.id);
            Value::Array(
                folders
                    .into_iter()
                    .map(|f| {
                        json!({
                            "id": f.id,
                            "path": path_of(state, &f.path),
                            "depth": store.folder_depth(f.id, &root),
                            "collapsed": f.collapsed,
                            "files": f.file_count,
                        })
                    })
                    .collect(),
            )
        }
        "edges" | "backwards" => {
            let only_backwards = what == "backwards";
            let mut edges: Vec<Value> = Vec::new();
            for e in state.edge_store.all_edges() {
                let (Some(a), Some(b)) = (node_rect(e.source_node), node_rect(e.target_node)) else { continue };
                let backwards = crate::edges::geometry::is_backwards(a, b);
                if only_backwards && !backwards {
                    continue;
                }
                let name = |id: u64| store.get_file(id).map(|n| path_of(state, &n.path));
                edges.push(json!({
                    "source": name(e.source_node),
                    "target": name(e.target_node),
                    "relationship": e.relationship.label(),
                    "kind": format!("{:?}", e.kind),
                    "symbol": e.label,
                    "source_line": e.source_span.map(|s| s.start + 1),
                    "target_line": e.target_span.map(|s| s.start + 1),
                    "change": format!("{:?}", e.change),
                    "backwards": backwards,
                }));
            }
            edges.sort_by_key(|e| (e["source"].as_str().map(str::to_owned), e["target"].as_str().map(str::to_owned)));
            Value::Array(edges)
        }
        "layout" => serde_json::to_value(&state.layout_settings).map_err(|e| e.to_string())?,
        "viewport" => json!({
            "zoom": state.viewport.zoom,
            "offset": [state.viewport.offset.x, state.viewport.offset.y],
            "canvas": [state.canvas_rect.width(), state.canvas_rect.height()],
            "level": crate::nodes::lod::LodLevel::from_zoom(state.viewport.zoom).number(),
        }),
        "selection" => json!({
            "nodes": state.selection.selected_ids().filter_map(|id| store.get_file(id).map(|n| path_of(state, &n.path))).collect::<Vec<_>>(),
            "symbols": state.selection.symbols().len(),
        }),
        "pins" => Value::Array(
            state
                .lsp
                .pins
                .pins()
                .iter()
                .map(|p| {
                    json!({
                        "name": p.name,
                        "path": path_of(state, &p.path),
                        "line": p.position.line + 1,
                        "results": p.graph().map(|g| g.entries.len()),
                        "error": match &p.fetch { crate::ui::symbol_pins::PinFetch::Failed(e) => Some(e.clone()), _ => None },
                    })
                })
                .collect(),
        ),
        "notes" => Value::Array(state.notes.all().iter().map(|n| json!({"id": n.id, "title": n.title, "markdown": n.markdown})).collect()),
        "settings" => json!({
            "node_width": state.settings.node_width,
            "font_size": state.settings.font_size,
            "edge_style": serde_json::to_value(&state.edge_style).map_err(|e| e.to_string())?,
        }),
        other => return Err(format!("unknown query {other}")),
    })
}

fn search(state: &AppState, args: &Value) -> Result<Value, String> {
    let query = args.get("query").and_then(Value::as_str).ok_or("query is required")?;
    let mode = args.get("mode").and_then(Value::as_str).unwrap_or("content");
    let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(50) as usize;
    let root = state.loaded_dir.clone().ok_or("open a folder first")?;
    let files = crate::workspace::folder_text_files(state);
    Ok(match mode {
        "content" => {
            let (hits, cut) = crate::search::content::search_content(&files, query);
            json!({
                "truncated": cut || hits.len() > limit,
                "hits": hits.iter().take(limit).map(|h| json!({"path": path_of(state, &h.path), "line": h.line + 1, "text": h.text})).collect::<Vec<_>>(),
            })
        }
        "paths" => {
            let hits = crate::search::files::search_files(&root, &files, query);
            json!({"hits": hits.iter().take(limit).map(|h| h.relative.clone()).collect::<Vec<_>>()})
        }
        "symbols" => {
            let hits = crate::search::symbols::search_symbols(&files, query);
            json!({"hits": hits.iter().take(limit).map(|h| json!({
                "name": h.symbol.name, "kind": h.symbol.kind, "path": path_of(state, &h.symbol.path), "line": h.symbol.line + 1,
            })).collect::<Vec<_>>()})
        }
        other => return Err(format!("unknown search mode {other}")),
    })
}

fn lsp_servers(state: &AppState, action: &str) -> Result<Value, String> {
    use crate::analysis::lsp::ServerId;
    Ok(match action {
        "detect" => Value::Array(
            ServerId::ALL
                .iter()
                .map(|server| match server.command() {
                    Ok(command) => json!({"language": server.name(), "found": true, "command": command}),
                    Err(e) => json!({"language": server.name(), "found": false, "error": e, "env": server.env_var()}),
                })
                .collect(),
        ),
        "status" => {
            let progress = state.deps.progress(&state.lsp.manager);
            Value::Array(
                progress
                    .iter()
                    .map(|p| json!({
                        "language": p.server.name(),
                        "files": p.files,
                        "done": p.done,
                        "status": p.status.as_ref().map(|s| format!("{s:?}")),
                        "error": p.error,
                    }))
                    .collect(),
            )
        }
        other => return Err(format!("unknown action {other}")),
    })
}

fn read_docs(args: &Value) -> Result<Value, String> {
    use crate::ui::docs_dialog::{sections, GUIDE};
    if args.get("list").and_then(Value::as_bool) == Some(true) {
        return Ok(json!(sections(GUIDE)));
    }
    let Some(name) = args.get("section").and_then(Value::as_str) else {
        return Ok(json!(GUIDE));
    };
    // The text from `## name` to the next `## `.
    let heading = format!("## {name}");
    let start = GUIDE.find(&heading).ok_or_else(|| format!("no section {name}; try list=true"))?;
    let body = &GUIDE[start..];
    let end = body[heading.len()..].find("\n## ").map_or(body.len(), |i| heading.len() + i);
    Ok(json!(body[..end].trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_six_tools_of_the_reference_are_listed_with_schemas() {
        let names: Vec<&str> = tools().iter().map(|t| t.name).collect();
        assert_eq!(names, ["list_commands", "use_command", "query", "search", "lsp_servers", "read_docs"]);
        for tool in tools() {
            assert_eq!((tool.schema)()["type"], "object", "{}", tool.name);
            assert!(!tool.description.is_empty());
        }
    }

    #[test]
    fn list_commands_and_use_command_share_the_registry() {
        let mut state = AppState::default();
        let list = handle(&mut state, "list_commands", &Value::Null).unwrap();
        let ids: Vec<&str> = list.as_array().unwrap().iter().filter_map(|c| c["id"].as_str()).collect();
        assert!(ids.contains(&"layout:apply") && ids.contains(&"agentNotes:push"));
        handle(&mut state, "use_command", &json!({"id": "agentNotes:push", "args": {"title": "T", "markdown": "# Hi"}})).unwrap();
        assert_eq!(state.notes.all().len(), 1);
        assert!(handle(&mut state, "use_command", &json!({"id": "no:such"})).is_err());
        assert!(handle(&mut state, "use_command", &json!({})).is_err(), "id is required");
    }

    #[test]
    fn query_reads_the_canvas_notes_and_layout() {
        let mut state = AppState::default();
        handle(&mut state, "use_command", &json!({"id": "agentNotes:push", "args": {"markdown": "text"}})).unwrap();
        let canvas = handle(&mut state, "query", &json!({"what": "canvas"})).unwrap();
        assert_eq!((canvas["nodes"].as_u64(), canvas["notes"].as_u64()), (Some(0), Some(1)));
        assert_eq!(handle(&mut state, "query", &json!({"what": "nodes"})).unwrap(), json!([]));
        let layout = handle(&mut state, "query", &json!({"what": "layout"})).unwrap();
        assert_eq!(layout["file_spacing"], 500.0);
        assert!(handle(&mut state, "query", &json!({"what": "nope"})).is_err());
        assert!(handle(&mut state, "query", &json!({})).is_err());
    }

    #[test]
    fn read_docs_lists_reads_and_cuts_sections() {
        let list = read_docs(&json!({"list": true})).unwrap();
        assert!(list.as_array().unwrap().iter().any(|s| s == "Move around"));
        let section = read_docs(&json!({"section": "Move around"})).unwrap();
        let text = section.as_str().unwrap();
        assert!(text.starts_with("## Move around") && !text.contains("## Folders"), "{text}");
        assert!(read_docs(&json!({"section": "Nope"})).is_err());
        assert!(read_docs(&json!({})).unwrap().as_str().unwrap().contains("SpatialCoding"));
    }

    #[test]
    fn lsp_servers_detects_every_language_and_search_needs_a_folder() {
        let mut state = AppState::default();
        let found = handle(&mut state, "lsp_servers", &json!({"action": "detect"})).unwrap();
        assert_eq!(found.as_array().unwrap().len(), 4);
        assert!(handle(&mut state, "search", &json!({"query": "x"})).is_err(), "no folder open");
        assert!(handle(&mut state, "lsp_servers", &json!({"action": "x"})).is_err());
    }
}
