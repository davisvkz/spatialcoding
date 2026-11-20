//! Frame-time measurement on a large project (TASK-62): opens every file of
//! this repository on the canvas and times the frames at overview, L2 and L1
//! zoom. Ignored by default; run it in release mode:
//!
//! `cargo test --release --test e2e perf -- --ignored --nocapture`

use std::path::Path;
use std::time::Instant;

use spatialcoding::analysis::dependencies::LineSpan;
use spatialcoding::analysis::lsp::manager::Launcher;
use spatialcoding::edges::edge_store::{EdgeKind, NameSpan};

use crate::common::{app, AppExt};

const FRAMES: u32 = 10;

fn ms_per_frame(harness: &mut crate::common::App, zoom: f32) -> f64 {
    {
        let vp = &mut harness.app_state_mut().viewport;
        vp.zoom = zoom;
        vp.target_zoom = zoom;
    }
    // One warm-up frame: highlighting and CST caches fill here.
    harness.step();
    let start = Instant::now();
    for _ in 0..FRAMES {
        harness.step();
    }
    start.elapsed().as_secs_f64() * 1000.0 / f64::from(FRAMES)
}

/// Three edges out of every file, to files a few steps away in id order, from
/// a name on line 0 to a name on line 1: about as many as a real project has.
fn add_synthetic_edges(state: &mut spatialcoding::state::AppState) {
    let mut ids: Vec<u64> = state.node_store.file_nodes.keys().copied().collect();
    ids.sort_unstable();
    state.edge_store.clear();
    for (i, &src) in ids.iter().enumerate() {
        for step in [1, 7, 31] {
            let tgt = ids[(i + step) % ids.len()];
            if tgt == src {
                continue;
            }
            let name = |line| Some(NameSpan { line, start_byte: 0, end_byte: 2 });
            state.edge_store.add_full(
                src,
                tgt,
                Some("synthetic".into()),
                None,
                Some(LineSpan { start: 0, end: 0 }),
                Some(LineSpan { start: 1, end: 1 }),
                name(1),
                name(0),
                EdgeKind::Direct,
            );
        }
    }
}

/// Time of the per-frame pieces the canvas runs, one call each.
fn breakdown(state: &mut spatialcoding::state::AppState) {
    use spatialcoding::edges::ports::collect_ports;
    let time = |label: &str, f: &mut dyn FnMut()| {
        let start = Instant::now();
        f();
        println!("  {label:<28} {:8.2} ms", start.elapsed().as_secs_f64() * 1000.0);
    };
    let (w, f) = (state.settings.node_width, state.settings.font_size);
    let pad = state.layout_settings.folder_padding;
    time("update_folder_bounds", &mut || state.node_store.update_folder_bounds(w, f, pad));
    time("hidden_folders", &mut || drop(state.node_store.hidden_folders()));
    time("layout_hidden_folders", &mut || drop(state.node_store.layout_hidden_folders()));
    time("zoom_collapsed", &mut || drop(spatialcoding::folder_actions::zoom_collapsed(state, 0.02)));
    time("folder_view", &mut || drop(spatialcoding::folder_actions::folder_view(state)));
    time("is_file_hidden x all files", &mut || {
        let ids: Vec<u64> = state.node_store.file_nodes.keys().copied().collect();
        for id in ids {
            let _ = state.node_store.is_file_hidden(id);
        }
    });
    time("collect_ports", &mut || {
        drop(collect_ports(&state.edge_store, &state.node_store, &state.edge_style, &state.selection, w, f))
    });
    time("collapse_proxies", &mut || drop(state.node_store.collapse_proxies()));
    time("Snapshot::capture", &mut || drop(spatialcoding::undo::Snapshot::capture(state)));
}

#[test]
#[ignore = "timing measurement; run with --release --ignored --nocapture"]
fn frame_time_with_the_whole_repository_open() {
    let root = std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
    let mut harness = app();
    // No language server: the real indexing is slow and unrelated to drawing.
    // The edges are synthetic, see `add_synthetic_edges`.
    let no_server: Launcher = std::sync::Arc::new(|_, _| Err("perf test: no language server".into()));
    harness.app_state_mut().lsp.manager.set_launcher(no_server);
    harness.state_mut().open_folder(root);
    harness.run();
    let opened = Instant::now();
    spatialcoding::workspace::open_all_files(harness.app_state_mut());
    harness.step();
    add_synthetic_edges(harness.app_state_mut());
    spatialcoding::workspace::apply_layout_keeping_selection(harness.app_state_mut());
    harness.step();
    let state = harness.app_state();
    println!(
        "opened {} files, {} folders, {} edges in {:.0} ms",
        state.node_store.file_nodes.len(),
        state.node_store.folder_nodes.len(),
        state.edge_store.all_edges().count(),
        opened.elapsed().as_secs_f64() * 1000.0,
    );
    // Frames that only wait for the failing server's retries to settle.
    for _ in 0..80 {
        harness.step();
    }
    breakdown(harness.app_state_mut());
    for (label, zoom) in [("overview (L5)", 0.005), ("L4", 0.02), ("L3", 0.06), ("L2", 0.2), ("L1", 0.6)] {
        println!("{label:>14}: {:7.2} ms/frame", ms_per_frame(&mut harness, zoom));
    }
}
