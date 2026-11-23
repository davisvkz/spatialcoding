//! Where the layout puts the files of a real checkout (TASK-95). Ignored: it
//! needs a clone, named by `LAYOUT_REPORT_DIR`, and prints instead of asserting.
//!
//! ```sh
//! LAYOUT_REPORT_DIR=/path/to/redux LAYOUT_REPORT_SUBDIR=src \
//!   cargo test --test e2e layout_report -- --ignored --nocapture
//! ```
//!
//! The language server is off, so the edges are the static ones. The report
//! lists the four files of `codecanvas/comparativo-spatialcoding-v2.md` and
//! then every file of the subfolder, as `column x`, `y`, and the size.

use std::path::{Path, PathBuf};

use crate::common::{app, AppExt};

fn files_below(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            files_below(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn report(app: &mut crate::common::App, label: &str, files: &[PathBuf], root: &Path) {
    let ids: Vec<(PathBuf, u64)> = files.iter().map(|p| (p.clone(), app.open(p))).collect();
    app.settle_edges();
    spatialcoding::workspace::apply_layout_keeping_selection(app.app_state_mut());
    app.run();
    let state = app.app_state();
    println!("== {label}: {} files, {} edges", ids.len(), state.edge_store.all_edges().count());
    let mut rows: Vec<(f32, f32, String, f32, f32)> = ids
        .iter()
        .map(|(path, id)| {
            let rect = state.node_store.get_file(*id).unwrap().world_rect(state.settings.node_width, state.settings.font_size);
            (rect.min.x, rect.min.y, path.strip_prefix(root).unwrap().display().to_string(), rect.width(), rect.height())
        })
        .collect();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    for (x, y, name, w, h) in rows {
        println!("  x {x:>8.0}  y {y:>8.0}  {w:>4.0}x{h:<6.0} {name}");
    }
    let mut targets: Vec<String> = state
        .edge_store
        .all_edges()
        .map(|e| {
            let name = |id| state.node_store.get_file(id).unwrap().path.file_name().unwrap().to_string_lossy().into_owned();
            format!("{} -> {} ({:?})", name(e.source_node), name(e.target_node), e.kind)
        })
        .collect();
    targets.sort();
    for edge in targets {
        println!("    {edge}");
    }
}

#[test]
#[ignore = "needs LAYOUT_REPORT_DIR"]
fn layout_report() {
    let Ok(dir) = std::env::var("LAYOUT_REPORT_DIR") else { return };
    let root = PathBuf::from(dir);
    let sub = std::env::var("LAYOUT_REPORT_SUBDIR").unwrap_or_else(|_| "src".into());
    for (label, four) in [("four files", true), ("whole folder", false)] {
        let mut app = app();
        app.app_state_mut().lsp.manager.set_launcher(std::sync::Arc::new(|_, _| Err("offline".into())));
        app.state_mut().open_folder(root.clone());
        app.run();
        let files: Vec<PathBuf> = if four {
            ["index.ts", "createStore.ts", "applyMiddleware.ts", "compose.ts"].iter().map(|f| root.join(&sub).join(f)).collect()
        } else {
            let mut all = Vec::new();
            files_below(&root.join(&sub), &mut all);
            all
        };
        report(&mut app, label, &files, &root);
    }
}
