//! Frame-time measurement on a large project (TASK-62): opens every file of
//! this repository on the canvas and times the frames at overview, L2 and L1
//! zoom. Ignored by default; run it in release mode:
//!
//! `cargo test --release --test e2e perf -- --ignored --nocapture`

use std::path::Path;
use std::time::{Duration, Instant};

use spatialcoding::analysis::dependencies::LineSpan;
use spatialcoding::analysis::lsp::manager::{Launcher, LspManager, ServerStatus};
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
            let name = |line| {
                Some(NameSpan {
                    line,
                    start_byte: 0,
                    end_byte: 2,
                })
            };
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
        println!(
            "  {label:<28} {:8.2} ms",
            start.elapsed().as_secs_f64() * 1000.0
        );
    };
    let (w, f) = (state.settings.node_width, state.settings.font_size);
    let pad = state.layout_settings.folder_padding;
    time("update_folder_bounds", &mut || {
        state.node_store.update_folder_bounds(w, f, pad)
    });
    time("hidden_folders", &mut || {
        drop(state.node_store.hidden_folders())
    });
    time("layout_hidden_folders", &mut || {
        drop(state.node_store.layout_hidden_folders())
    });
    time("zoom_collapsed", &mut || {
        drop(spatialcoding::folder_actions::zoom_collapsed(state, 0.02))
    });
    time("folder_view", &mut || {
        drop(spatialcoding::folder_actions::folder_view(state))
    });
    time("is_file_hidden x all files", &mut || {
        let ids: Vec<u64> = state.node_store.file_nodes.keys().copied().collect();
        for id in ids {
            let _ = state.node_store.is_file_hidden(id);
        }
    });
    time("collect_ports", &mut || {
        drop(collect_ports(
            &state.edge_store,
            &state.node_store,
            &state.edge_style,
            &state.selection,
            w,
            f,
        ))
    });
    time("collapse_proxies", &mut || {
        drop(state.node_store.collapse_proxies())
    });
    time("Snapshot::capture", &mut || {
        drop(spatialcoding::undo::Snapshot::capture(state))
    });
}

#[test]
#[ignore = "timing measurement; run with --release --ignored --nocapture"]
fn frame_time_with_the_whole_repository_open() {
    let root = std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
    let mut harness = app();
    // No language server: the real indexing is slow and unrelated to drawing.
    // The edges are synthetic, see `add_synthetic_edges`.
    let no_server: Launcher =
        std::sync::Arc::new(|_, _| Err("perf test: no language server".into()));
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
    // Let asynchronous failures settle before measuring steady rendering.
    for _ in 0..80 {
        harness.step();
    }
    breakdown(harness.app_state_mut());
    for (label, zoom) in [
        ("overview (L5)", 0.005),
        ("L4", 0.02),
        ("L3", 0.06),
        ("L2", 0.2),
        ("L1", 0.6),
    ] {
        println!(
            "{label:>14}: {:7.2} ms/frame",
            ms_per_frame(&mut harness, zoom)
        );
    }
}

/// Opens this repository on the canvas (no language server, synthetic edges)
/// and times what the other tests skip: the cold open, the first frame with
/// cold highlight/CST caches, a continuous zoom sweep through the levels that
/// draw text, and the tessellation of a text frame (TASK-83).
///
/// `cargo test --release --test e2e perf::open_and_zoom -- --ignored --nocapture`
#[test]
#[ignore = "timing measurement; run with --release --ignored --nocapture"]
fn open_and_zoom_sweep_with_the_whole_repository() {
    let root = std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
    let mut harness = app();
    let no_server: Launcher =
        std::sync::Arc::new(|_, _| Err("perf test: no language server".into()));
    harness.app_state_mut().lsp.manager.set_launcher(no_server);
    harness.state_mut().open_folder(root);
    harness.run();
    let set_zoom = |harness: &mut crate::common::App, zoom: f32| {
        harness
            .app_state_mut()
            .viewport
            .zoom_to(zoom, crate::common::CANVAS_SPOT);
    };
    set_zoom(&mut harness, 0.6);

    let ms = |start: Instant| start.elapsed().as_secs_f64() * 1000.0;
    // The pieces of the open, to see where its time goes.
    {
        let state = harness.app_state();
        let dir = state.loaded_dir.clone().unwrap();
        let start = Instant::now();
        let tree = spatialcoding::fs::loader::load_directory_filtered(
            &dir, &dir, state.settings.show_dot_folders, &state.ignore_set(),
        )
        .unwrap();
        println!("  load_directory_filtered:   {:.1} ms", ms(start));
        let mut paths = Vec::new();
        fn walk(node: &spatialcoding::fs::tree::TreeNode, out: &mut Vec<std::path::PathBuf>) {
            match node {
                spatialcoding::fs::tree::TreeNode::File { path, .. } => out.push(path.clone()),
                spatialcoding::fs::tree::TreeNode::Dir { children, .. } => {
                    children.iter().for_each(|c| walk(c, out));
                }
            }
        }
        walk(&tree, &mut paths);
        let start = Instant::now();
        for path in &paths {
            let _ = spatialcoding::fs::loader::read_file_content(path);
        }
        println!("  read {} files:           {:.1} ms", paths.len(), ms(start));
    }
    let start = Instant::now();
    let opened = spatialcoding::workspace::open_all_files(harness.app_state_mut()).len();
    println!("open_all_files: {opened} files in {:.1} ms", ms(start));
    let start = Instant::now();
    spatialcoding::workspace::rebuild_edges(harness.app_state_mut());
    println!("  rebuild_edges alone:       {:.1} ms", ms(start));
    add_synthetic_edges(harness.app_state_mut());
    spatialcoding::workspace::apply_layout_keeping_selection(harness.app_state_mut());
    // Centre the view on the middle file so the sweep always has nodes in sight.
    {
        let state = harness.app_state_mut();
        let (w, f) = (state.settings.node_width, state.settings.font_size);
        let mut ids: Vec<u64> = state.node_store.file_nodes.keys().copied().collect();
        ids.sort_unstable();
        let middle = state.node_store.file_nodes[&ids[ids.len() / 2]].world_rect(w, f);
        let spot = crate::common::CANVAS_SPOT;
        let zoom = state.viewport.zoom;
        state.viewport.offset = egui::vec2(
            spot.x / zoom - middle.center().x,
            spot.y / zoom - middle.center().y,
        );
    }
    // Zoomed out to L2 the screen holds many files, none of them colored yet.
    set_zoom(&mut harness, 0.12);
    let start = Instant::now();
    harness.step();
    println!("first frame at L2 (many cold files): {:.1} ms", ms(start));
    set_zoom(&mut harness, 0.6);
    let start = Instant::now();
    harness.step();
    println!("first frame at L1 (cold highlight):  {:.1} ms", ms(start));
    set_zoom(&mut harness, 0.06);
    let start = Instant::now();
    harness.step();
    println!("first frame at L3 (cold CST tint):   {:.1} ms", ms(start));
    for _ in 0..80 {
        harness.step();
    }

    // Zoom 1.0 -> 0.25 -> 1.0 around the middle of the screen, 2 % a frame.
    // Frames at L1 draw text; the ones below 0.3 draw bars.
    let (mut text, mut bars) = (Vec::new(), Vec::new());
    let mut zoom = 1.0_f32;
    let mut factor = 0.98_f32;
    set_zoom(&mut harness, zoom);
    for _ in 0..140 {
        zoom *= factor;
        if zoom < 0.25 {
            factor = 1.0 / 0.98;
        }
        set_zoom(&mut harness, zoom);
        let frame = Instant::now();
        harness.step();
        let took = frame.elapsed();
        if took > Duration::from_millis(30) {
            println!("  slow frame {:.1} ms at zoom {zoom:.3}", took.as_secs_f64() * 1000.0);
        }
        if zoom >= 0.3 { text.push(took) } else { bars.push(took) }
    }
    report_frames("zoom sweep, text (L1)", text);
    report_frames("zoom sweep, bars (L2)", bars);
    // The same levels with the zoom held still.
    for (label, zoom) in [("still, text (zoom 0.6)", 0.6), ("still, text (zoom 0.35)", 0.35), ("still, bars (zoom 0.2)", 0.2)] {
        set_zoom(&mut harness, zoom);
        harness.step();
        let samples = (0..60)
            .map(|_| {
                let frame = Instant::now();
                harness.step();
                frame.elapsed()
            })
            .collect();
        report_frames(label, samples);
    }
    let ctx = harness.ctx.clone();
    let galleys = ctx.fonts(|f| f.num_galleys_in_cache());
    let [atlas_w, atlas_h] = ctx.fonts(|f| f.font_image_size());
    println!("galleys in cache after the sweep: {galleys}, font atlas {atlas_w}x{atlas_h}");

    // Tessellation of a frame that draws text.
    set_zoom(&mut harness, 0.6);
    harness.step();
    let shapes = harness.output().shapes.clone();
    let count = shapes.len();
    let start = Instant::now();
    let primitives = ctx.tessellate(shapes, 1.0);
    println!(
        "tessellate at L1: {count} shapes -> {} primitives in {:.2} ms",
        primitives.len(),
        ms(start),
    );
}

/// The same Cargo project for both servers. Most symbols have no cross-file
/// references; one in four files calls a function in the next file.
fn indexing_fixture(root: &Path) {
    const GROUPS: usize = 16;
    const FILES_PER_GROUP: usize = 20;
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"canvas-perf\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    let src = root.join("src");
    std::fs::create_dir(&src).unwrap();
    let mut lib = String::new();
    for group in 0..GROUPS {
        let module = format!("group_{group}");
        lib.push_str(&format!("mod {module};\n"));
        let dir = src.join(module);
        std::fs::create_dir(&dir).unwrap();
        let mut modules = String::new();
        for file in 0..FILES_PER_GROUP {
            modules.push_str(&format!("mod file_{file};\n"));
            let id = group * FILES_PER_GROUP + file;
            let use_next = if file % 4 == 0 {
                format!("    super::file_{}::item_{}();\n", file + 1, id + 1)
            } else {
                String::new()
            };
            let text = format!("pub fn item_{id}() {{\n{use_next}}}\nfn private_{id}() {{}}\n");
            std::fs::write(dir.join(format!("file_{file}.rs")), text).unwrap();
        }
        std::fs::write(dir.join("mod.rs"), modules).unwrap();
    }
    std::fs::write(src.join("lib.rs"), lib).unwrap();
}

fn report_frames(label: &str, mut samples: Vec<Duration>) {
    samples.sort_unstable();
    let ms = |sample: Duration| sample.as_secs_f64() * 1000.0;
    let percentile = |p: usize| ms(samples[(samples.len() * p).div_ceil(100) - 1]);
    println!(
        "{label}: {} frames, p50 {:.2} ms, p95 {:.2} ms, p99 {:.2} ms, max {:.2} ms",
        samples.len(),
        percentile(50),
        percentile(95),
        percentile(99),
        ms(*samples.last().unwrap()),
    );
}

fn measure_indexing(real: bool) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    indexing_fixture(&root);
    let mut harness = app();
    if real {
        harness.app_state_mut().lsp.manager = LspManager::default();
    }
    harness.state_mut().open_folder(root);
    harness.run();
    {
        let vp = &mut harness.app_state_mut().viewport;
        vp.zoom = 0.02;
        vp.target_zoom = vp.zoom;
    }
    let start = Instant::now();
    let opened = spatialcoding::workspace::open_all_files(harness.app_state_mut()).len();
    println!(
        "opened {opened} files in {:.2} ms",
        start.elapsed().as_secs_f64() * 1000.0
    );

    let frame_interval = Duration::from_secs_f64(1.0 / 60.0);
    let mut frames = Vec::new();
    while harness.app_state().deps.is_waiting() {
        assert!(
            start.elapsed() < Duration::from_secs(90),
            "dependency indexing did not settle"
        );
        let frame = Instant::now();
        harness.step();
        let elapsed = frame.elapsed();
        frames.push(elapsed);
        std::thread::sleep(frame_interval.saturating_sub(elapsed));
    }
    report_frames("indexing", frames);
    let state = harness.app_state();
    let progress = state.deps.progress(&state.lsp.manager);
    assert_eq!(progress.len(), 1);
    assert_eq!(progress[0].status, Some(ServerStatus::Ready));
    assert_eq!(progress[0].done, progress[0].files);
    assert!(progress[0].error.is_none(), "{progress:?}");
    let edges = state.edge_store.all_edges().count();
    let lsp_edges = state
        .edge_store
        .all_edges()
        .filter(|edge| edge.name_span.is_some())
        .count();
    println!(
        "settled in {:.2} s with {edges} edges ({lsp_edges} with LSP definition spans)",
        start.elapsed().as_secs_f64()
    );
    assert!(
        lsp_edges >= 80,
        "expected the cross-file calls to reach the canvas"
    );

    for _ in 0..10 {
        harness.step();
    }
    report_frames(
        "steady",
        (0..100)
            .map(|_| {
                let start = Instant::now();
                harness.step();
                start.elapsed()
            })
            .collect(),
    );
}

#[test]
#[ignore = "timing measurement; run with --release --ignored --nocapture"]
fn indexing_frame_time_fake_lsp() {
    measure_indexing(false);
}

#[test]
#[ignore = "timing measurement; requires rust-analyzer on PATH"]
fn indexing_frame_time_rust_analyzer() {
    measure_indexing(true);
}
