//! The LSP tab against the in-process fake server in `fake_lsp.rs`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Modifiers, Pos2};
use egui_kittest::kittest::Queryable;
use spatialcoding::analysis::lsp::install::{Download, Downloader, Installer, Platform, Probe};
use spatialcoding::analysis::lsp::resolve_command;
use spatialcoding::nodes::file_node::{line_height, CODE_PADDING, HEADER_HEIGHT};
use spatialcoding::state::RightPanel;
use spatialcoding::ui::lsp_panel::LineHighlight;

use crate::common::*;
use crate::fake_lsp;

fn canvas_product(app: &mut App, fx: &Fixture) -> (u64, Pos2) {
    std::fs::write(fx.path("src/product.rs"), concat!(
        "pub struct Product {\n    pub id: u64,\n}\n",
        "impl Product {\n    pub fn new() -> Product {\n        Product { id: 1 }\n    }\n}\n",
        "impl Default for Product {\n    fn default() -> Self {\n        Product::new()\n    }\n}\n",
    )).unwrap();
    let id = app.open(&fx.path("src/product.rs"));
    app.app_state_mut().left_panel = None;
    app.app_state_mut().right_panel = None;
    app.app_state_mut().selection.clear();
    let font = app.app_state().settings.font_size;
    let node = app.app_state().node_store.get_file(id).unwrap();
    let origin = node.position + egui::vec2(0.0, HEADER_HEIGHT + CODE_PADDING);
    app.show_at(origin, Pos2::new(350.0, 180.0));
    // Find the actual glyph bounds, not an estimated column width.
    let at = frame_shapes(app)
        .iter()
        .find_map(|s| {
            let egui::Shape::Text(text) = s else {
                return None;
            };
            let offset = text.galley.job.text.find("Product")?;
            let chars = text.galley.job.text[..offset].chars().count();
            let left = text
                .galley
                .pos_from_ccursor(egui::text::CCursor::new(chars));
            let right = text
                .galley
                .pos_from_ccursor(egui::text::CCursor::new(chars + 7));
            let p = text.pos + egui::vec2((left.min.x + right.min.x) * 0.5, left.center().y);
            (p.y < 180.0 + line_height(font)).then_some(p)
        })
        .expect("the definition name is painted");
    (id, at)
}

/// Steps frames until `done`, while the fake server answers.
fn wait_until(app: &mut App, what: &str, done: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(app) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
        app.step();
    }
}

#[test]
fn canvas_query_on_hover_pins_a_preview_that_leaving_drops_and_a_click_keeps() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    let (id, at) = canvas_product(&mut app, &fx);
    app.input_mut().events.push(egui::Event::PointerMoved(at));
    for _ in 0..8 {
        app.step();
    }
    assert!(app.app_state().lsp.pins.pins().is_empty(), "off by default");
    let (offset, zoom) = (app.app_state().viewport.offset, app.app_state().viewport.zoom);

    app.app_state_mut().settings.lsp.query_on_hover = true;
    wait_until(&mut app, "the hover preview", |a| a.app_state().lsp.pins.pins().first().is_some_and(|p| p.graph().is_some()));
    let pins = app.app_state().lsp.pins.pins();
    assert_eq!((pins.len(), pins[0].preview, pins[0].name.as_str(), pins[0].node_id), (1, true, "Product", id));
    assert_eq!(app.app_state().lsp.pins.len(), 0, "a preview is not counted as pinned");
    assert!(app.selected().is_empty() && app.app_state().selection.symbols().is_empty());
    assert_eq!((app.app_state().viewport.offset, app.app_state().viewport.zoom), (offset, zoom));
    assert_eq!(app.app_state().node_store.file_nodes.len(), 1, "a preview opens no files");

    // Moving away drops it.
    app.input_mut().events.push(egui::Event::PointerMoved(Pos2::new(900.0, 700.0)));
    for _ in 0..3 {
        app.step();
    }
    assert!(app.app_state().lsp.pins.pins().is_empty(), "the preview goes with the pointer");

    // Resting again and clicking keeps it as a real pin.
    app.input_mut().events.push(egui::Event::PointerMoved(at));
    wait_until(&mut app, "the preview again", |a| a.app_state().lsp.pins.pins().first().is_some_and(|p| p.graph().is_some()));
    click_at(&mut app, at, Modifiers::NONE);
    for _ in 0..3 {
        app.step();
    }
    let pins = app.app_state().lsp.pins.pins();
    assert!(pins.len() == 1 && !pins[0].preview, "the click keeps the pin: {pins:?}");
    assert_eq!(app.app_state().lsp.pins.len(), 1);
}

/// The app with `fx` loaded and the fake language server plugged in.
fn app_with_fake_lsp(fx: &Fixture) -> App {
    let mut app = app_with(fx);
    app.app_state_mut()
        .lsp
        .manager
        .set_launcher(fake_lsp::launcher());
    app
}

/// Opens `rel`, selects it with a click on the canvas and opens the LSP tab.
fn select_and_open_lsp(app: &mut App, fx: &Fixture, rel: &str) -> u64 {
    let id = app.open(&fx.path(rel));
    let center = app.node_center(id);
    app.show_at(center, CANVAS_SPOT);
    click_at(app, CANVAS_SPOT, Modifiers::NONE);
    assert_eq!(app.selected(), [id]);

    app.app_state_mut().right_panel = Some(RightPanel::Layout);
    app.run();
    app.get_by_label("LSP Symbols").click();
    settle(app);
    assert_eq!(app.app_state().right_panel, Some(RightPanel::LspSymbols));
    id
}

/// Steps frames until the LSP tab has no answer on its way. Answers come from
/// a server thread, and the tab repaints while it waits, so `run()` alone
/// would give up after its 4 frames. A click can start a new request a frame
/// later (the tab follows the new selection), so wait for two quiet frames.
fn settle(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut quiet = 0;
    while quiet < 2 {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the language server"
        );
        app.step();
        if app.app_state().lsp.is_waiting() {
            quiet = 0;
            std::thread::sleep(Duration::from_millis(10));
        } else {
            quiet += 1;
        }
    }
    app.run();
}

#[test]
fn lsp_tab_asks_for_a_selection() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    app.app_state_mut().right_panel = Some(RightPanel::LspSymbols);
    app.run();

    app.get_by_label("Select a file node to see its symbols.");
}

#[test]
fn files_without_a_server_say_so() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    select_and_open_lsp(&mut app, &fx, "README.md");

    app.get_by_label("No language server for Markdown.");
}

#[test]
fn lsp_tab_lists_the_symbols_of_the_selected_file() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    let id = select_and_open_lsp(&mut app, &fx, "src/main.rs");

    app.get_by_label("fn main  L3");
    app.get_by_label("Click a symbol to pin its graph.");

    app.get_by_label("fn main  L3").hover();
    app.run();
    assert_eq!(
        app.app_state().lsp.hovered,
        Some(LineHighlight {
            node_id: id,
            start_line: 2,
            end_line: 2
        })
    );
    app.get_by_label("Layout").click();
    app.run();
    assert_eq!(
        app.app_state().lsp.hovered,
        None,
        "other tabs drop the hover"
    );
}

#[test]
fn clicking_a_symbol_row_pins_it_and_a_result_focuses_its_line() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    // This test opens main.rs by clicking a result, not automatically.
    app.app_state_mut().settings.lsp.auto_open_matches = false;
    let util = select_and_open_lsp(&mut app, &fx, "src/util.rs");

    app.get_by_label("fn hello  L1").click();
    settle(&mut app);
    assert_eq!(app.app_state().selection.symbols().len(), 1, "LSP rows share canvas symbol focus");
    let pins = app.app_state().lsp.pins.pins();
    assert_eq!((pins.len(), pins[0].name.as_str(), pins[0].preview), (1, "hello", false));
    app.get_by_label("Pinned Symbols (1)");
    app.get_by_label("Files (2)");
    app.get_by_label_contains("Results (");
    app.get_by_label("ref  main.rs:4");

    // main.rs is not on the canvas yet, and the view looks elsewhere.
    assert_eq!(app.app_state().node_store.file_nodes.len(), 1);
    app.show_at(Pos2::new(-5000.0, -5000.0), CANVAS_SPOT);

    app.get_by_label("ref  main.rs:4").click();
    settle(&mut app);

    let s = app.app_state();
    assert_eq!(s.node_store.file_nodes.len(), 2, "main.rs was opened");
    let main = s.selection.primary.unwrap();
    assert_ne!(main, util);
    assert!(s.node_store.get_file(main).unwrap().path.ends_with("src/main.rs"));
    assert_eq!(s.lsp.target, Some(LineHighlight { node_id: main, start_line: 3, end_line: 3 }));
    assert!(s.pending_focus.is_none(), "the canvas applied the focus");

    // Line 4 of main.rs is now in the middle of the canvas.
    let node = s.node_store.get_file(main).unwrap();
    let rect = node.world_rect(s.settings.node_width, s.settings.font_size);
    let line_y = rect.top() + HEADER_HEIGHT + CODE_PADDING + 3.5 * line_height(s.settings.font_size);
    let on_screen = s.viewport.world_to_screen(Pos2::new(rect.center().x, line_y));
    assert!(
        (300.0..1100.0).contains(&on_screen.x) && (300.0..600.0).contains(&on_screen.y),
        "line shown near the canvas center: {on_screen:?}"
    );

    // The pin stays listed, the target line is painted on the node.
    app.get_by_label("Pinned Symbols (1)");
    let color = s.theme.selection.gamma_multiply(0.4);
    let painted = app.output().shapes.iter().any(|c| matches!(&c.shape, egui::Shape::Rect(r) if r.fill == color));
    assert!(painted, "target line highlighted");
}

#[test]
fn a_missing_server_explains_how_to_configure_it() {
    let fx = fixture();
    let mut app = app_with(&fx);
    // What the default launcher reports when nothing is on PATH.
    app.app_state_mut()
        .lsp
        .manager
        .set_launcher(Arc::new(|server, _| {
            Err(resolve_command(server, |_| None, |_| false).unwrap_err())
        }));
    select_and_open_lsp(&mut app, &fx, "src/main.rs");

    app.get_by_label_contains("No Rust language server found on PATH (tried rust-analyzer)");
}

#[test]
fn scrolled_lsp_rows_keep_correct_hover_and_pointer_click_bounds() {
    let fx = fixture();
    let content: String = (0..35)
        .map(|i| format!("pub fn symbol_{i}() {{}}\n"))
        .collect();
    std::fs::write(fx.path("src/util.rs"), content).unwrap();
    let mut app = app_with_fake_lsp(&fx);
    let node = app.open(&fx.path("src/util.rs"));
    app.set_size(egui::vec2(1400.0, 400.0));
    app.app_state_mut().left_panel = None;
    app.app_state_mut().selection.select(node);
    app.app_state_mut().right_panel = Some(RightPanel::LspSymbols);
    settle(&mut app);

    wheel(
        &mut app,
        Pos2::new(1200.0, 250.0),
        egui::vec2(0.0, -10000.0),
        Modifiers::NONE,
    );
    let label = "fn symbol_34  L35";
    let bounds = app.get_by_label(label).raw_bounds().unwrap();
    assert!(
        bounds.y0 > 60.0 && bounds.y1 < 400.0,
        "last symbol is visible: {bounds:?}"
    );
    app.get_by_label(label).hover();
    app.run();
    assert_eq!(
        app.app_state().lsp.hovered,
        Some(LineHighlight {
            node_id: node,
            start_line: 34,
            end_line: 34,
        })
    );

    let center = Pos2::new(
        ((bounds.x0 + bounds.x1) * 0.5) as f32,
        ((bounds.y0 + bounds.y1) * 0.5) as f32,
    );
    click_at_stepping(&mut app, center, Modifiers::NONE);
    settle(&mut app);
    assert_eq!(app.app_state().lsp.pins.pins().first().map(|p| p.name.as_str()), Some("symbol_34"));
}

#[test]
fn auto_open_matches_opens_the_reference_files_up_to_max_files() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    select_and_open_lsp(&mut app, &fx, "src/util.rs");
    app.get_by_label("fn hello  L1").click();
    settle(&mut app);
    app.settle_edges();
    let names: Vec<String> = app
        .app_state()
        .node_store
        .file_nodes
        .values()
        .map(|n| n.file_name().to_owned())
        .collect();
    assert!(
        names.contains(&"main.rs".to_owned()),
        "main.rs opened by Auto Open Matches: {names:?}"
    );
}

#[test]
fn symbol_graph_history_and_clear_symbols() {
    let fx = fixture();
    std::fs::write(
        fx.path("src/util.rs"),
        "pub fn hello() {}\npub fn bye() {}\n",
    )
    .unwrap();
    let mut app = app_with_fake_lsp(&fx);
    app.app_state_mut().settings.lsp.auto_open_matches = false;
    select_and_open_lsp(&mut app, &fx, "src/util.rs");
    app.get_by_label_contains("Click a symbol to show LSP graph results.");
    assert!(app.get_by_label("Previous symbol").is_disabled());
    assert!(app.get_by_label("Clear Symbols").is_disabled());
    let pinned = |app: &App| app.app_state().lsp.pins.pins().iter().map(|p| p.name.clone()).collect::<Vec<_>>();

    app.get_by_label("fn hello  L1").click();
    settle(&mut app);
    app.get_by_label("fn bye  L2").click();
    settle(&mut app);
    assert_eq!(pinned(&app), ["bye"], "a plain click replaces the pin");

    app.get_by_label("Previous symbol").click();
    settle(&mut app);
    assert_eq!(pinned(&app), ["hello"]);
    app.get_by_label("Next symbol").click();
    settle(&mut app);
    assert_eq!(pinned(&app), ["bye"]);

    app.get_by_label("Clear 1 Symbol").click();
    app.run();
    assert!(app.app_state().lsp.pins.is_empty());
    assert!(app.app_state().selection.symbols().is_empty());
    assert!(
        !app.app_state().selection.selected.is_empty(),
        "the file stays selected"
    );
}

#[test]
fn advanced_settings_card_opens_from_the_gear_and_edits_lsp_settings() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    select_and_open_lsp(&mut app, &fx, "src/util.rs");
    assert!(app.query_by_label("LSP Advanced Settings").is_none());
    app.get_by_label("Show advanced settings").click();
    app.run();
    app.get_by_label("LSP Advanced Settings");
    for label in [
        "Query On Hover",
        "Auto Open Matches",
        "Keep Zoom",
        "Smooth Transition",
        "Dim Dependency Edges",
        "Reveal Through Collapse",
        "Out of View",
    ] {
        app.get_by_label(label).click();
        app.run();
    }
    let lsp = &app.app_state().settings.lsp;
    assert!(
        lsp.query_on_hover && !lsp.auto_open_matches && lsp.keep_zoom && !lsp.smooth_transition
    );
    assert!(!lsp.dim_edges && lsp.reveal_through_collapse && lsp.out_of_view);
    app.get_by_label("Close LSP Advanced Settings").click();
    app.run();
    assert!(app.query_by_label("LSP Advanced Settings").is_none());
}

#[test]
fn query_on_hover_over_a_list_row_pins_a_preview_without_selecting() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    app.app_state_mut().settings.lsp.query_on_hover = true;
    app.app_state_mut().settings.lsp.auto_open_matches = false;
    select_and_open_lsp(&mut app, &fx, "src/util.rs");
    app.get_by_label("fn hello  L1").hover();
    for _ in 0..3 {
        app.step();
    }
    wait_until(&mut app, "the row preview", |a| a.app_state().lsp.pins.pins().first().is_some_and(|p| p.graph().is_some()));
    let pin = &app.app_state().lsp.pins.pins()[0];
    assert!(pin.preview && pin.name == "hello");
    assert!(app.app_state().selection.symbols().is_empty(), "hovering does not select");
}

#[test]
fn language_servers_dialog_lists_servers_and_file_counts() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    select_and_open_lsp(&mut app, &fx, "src/util.rs");
    app.get_by_label("LSP menu").click();
    app.run();
    app.get_by_label("Language servers").click();
    app.run();
    app.get_by_label("Repository files");
    app.get_by_label("Rust");
    app.get_by_label("TypeScript");
    assert!(app.get_by_label("Load project environment").is_disabled());
    app.get_by_label("Canvas").click();
    app.run();
    app.get_by_label("Canvas files");
    app.get_by_label("1");
}

/// A registry with one `pyright` package, for the Install button.
struct FakeRegistry(std::collections::HashMap<String, Vec<u8>>);

impl Downloader for FakeRegistry {
    fn get(&self, url: &str) -> Result<Download, String> {
        let body = self.0.get(url).ok_or_else(|| format!("{url}: 404"))?.clone();
        Ok(Download { len: Some(body.len() as u64), reader: Box::new(std::io::Cursor::new(body)) })
    }
}

fn fake_pyright_registry() -> FakeRegistry {
    use base64::Engine as _;
    use sha2::Digest as _;
    use std::io::Write as _;
    let mut builder = tar::Builder::new(Vec::new());
    let content = b"//";
    let mut header = tar::Header::new_gnu();
    header.set_size(content.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append_data(&mut header, "package/langserver.index.js", &content[..]).unwrap();
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&builder.into_inner().unwrap()).unwrap();
    let tgz = gz.finish().unwrap();
    let meta = serde_json::json!({
        "version": "1.0.0",
        "dist": {
            "tarball": "https://registry.npmjs.org/pyright/-/pyright-1.0.0.tgz",
            "integrity": format!("sha512-{}", base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(&tgz))),
        },
    });
    FakeRegistry(
        [
            ("https://registry.npmjs.org/pyright/latest".to_string(), meta.to_string().into_bytes()),
            ("https://registry.npmjs.org/pyright/-/pyright-1.0.0.tgz".to_string(), tgz),
        ]
        .into(),
    )
}

fn linux_with_node(node: Option<(u32, u32, u32)>) -> Platform {
    Platform { os: "linux".into(), arch: "x86_64".into(), node, has_go: true, musl: false }
}

fn open_language_servers(app: &mut App, fx: &Fixture) {
    select_and_open_lsp(app, fx, "src/util.rs");
    app.get_by_label("LSP menu").click();
    app.run();
    app.get_by_label("Language servers").click();
    app.run();
}

#[test]
fn language_servers_dialog_installs_a_missing_server() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    let dir = tempfile::tempdir().unwrap();
    app.app_state_mut().lsp.installer = Installer::with(
        dir.path().to_path_buf(),
        Arc::new(fake_pyright_registry()),
        Some(linux_with_node(Some((24, 0, 0)))),
        Probe { env: |_| None, exists: |_| false },
    );
    open_language_servers(&mut app, &fx);
    assert!(app.get_all_by_label("Not installed").count() >= 4);
    app.get_by_label("Install Python").click();
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.query_by_label("Installed (SpatialCoding)").is_none() {
        assert!(Instant::now() < deadline, "the install did not finish");
        app.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(dir.path().join("python/node_modules/pyright/langserver.index.js").is_file());
    assert!(app.query_by_label("Install Python").is_none());
    assert!(app.app_state().toasts.iter().any(|t| t.text == "Python language server installed"));
    // Remove deletes the managed copy and the row offers Install again.
    app.get_by_label("Remove").click();
    app.run();
    assert!(!dir.path().join("python").exists());
    app.get_by_label("Install Python");
}

#[test]
fn a_failed_install_shows_why_and_offers_retry() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    let dir = tempfile::tempdir().unwrap();
    app.app_state_mut().lsp.installer = Installer::with(
        dir.path().to_path_buf(),
        Arc::new(FakeRegistry(Default::default())),
        Some(linux_with_node(Some((24, 0, 0)))),
        Probe { env: |_| None, exists: |_| false },
    );
    open_language_servers(&mut app, &fx);
    app.get_by_label("Install Python").click();
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.query_by_label("Retry Python").is_none() {
        assert!(Instant::now() < deadline, "the install did not fail");
        app.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    app.get_by_label("Failed");
    assert!(app.app_state().toasts.iter().any(|t| t.text.contains("Could not install the Python language server")));
    assert!(!dir.path().join("python").exists());
}

#[test]
fn install_is_disabled_without_node_and_servers_on_path_show_installed() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    let dir = tempfile::tempdir().unwrap();
    app.app_state_mut().lsp.installer = Installer::with(
        dir.path().to_path_buf(),
        Arc::new(FakeRegistry(Default::default())),
        Some(linux_with_node(None)),
        Probe { env: |_| None, exists: |program| program == "gopls" },
    );
    open_language_servers(&mut app, &fx);
    assert!(app.get_by_label("Install Python").is_disabled());
    assert!(app.get_by_label("Install TypeScript").is_disabled());
    assert!(!app.get_by_label("Install Rust").is_disabled());
    app.get_by_label("Installed (PATH)");
    assert!(app.query_by_label("Install Go").is_none());
}

#[test]
fn the_palette_opens_the_language_servers_dialog() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    assert!(!app.app_state().lsp.show_servers);
    spatialcoding::commands::run(app.app_state_mut(), "lsp:openLanguageServers", &serde_json::Value::Null).unwrap();
    app.run();
    app.get_by_label("Language Servers");
}

#[test]
fn keep_zoom_and_out_of_view_tags() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    {
        let lsp = &mut app.app_state_mut().settings.lsp;
        lsp.keep_zoom = true;
        lsp.out_of_view = true;
    }
    select_and_open_lsp(&mut app, &fx, "src/util.rs");
    app.get_by_label("fn hello  L1").click();
    settle(&mut app);
    app.settle_edges();
    {
        let s = app.app_state_mut();
        s.viewport.zoom = 0.3;
        s.viewport.target_zoom = 0.3;
    }
    app.show_at(Pos2::new(-20_000.0, -20_000.0), CANVAS_SPOT);
    let canvas = egui::Rect::from_min_max(Pos2::new(40.0, 30.0), Pos2::new(1060.0, 900.0));
    let tags = spatialcoding::ui::pin_overlay::out_of_view(app.app_state(), canvas);
    assert_eq!(tags.len(), 2, "util.rs and main.rs are off screen: {tags:?}");

    app.get_by_label("ref  main.rs:4").click();
    settle(&mut app);
    assert_eq!(app.app_state().viewport.zoom, 0.3, "Keep Zoom");
}

#[test]
fn auto_open_matches_also_serves_pins_from_the_canvas_but_not_previews() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    let util = app.open(&fx.path("src/util.rs"));
    app.app_state_mut().right_panel = None;

    // Off: the pin's results stay unopened.
    app.app_state_mut().settings.lsp.auto_open_matches = false;
    spatialcoding::ui::symbol_pins::pin(None, app.app_state_mut(), util, 0, 7, false);
    wait_until(&mut app, "the pin graph", |a| a.app_state().lsp.pins.pins().first().is_some_and(|p| p.graph().is_some()));
    app.step();
    assert_eq!(app.app_state().node_store.file_nodes.len(), 1, "Auto Open Matches off");

    // On, from a preview: nothing opens until a click keeps it.
    app.app_state_mut().lsp.pins.clear();
    app.app_state_mut().settings.lsp.auto_open_matches = true;
    spatialcoding::ui::symbol_pins::pin_token(None, app.app_state_mut(), util, 0, 7, spatialcoding::ui::symbol_pins::PinMode::Preview);
    wait_until(&mut app, "the preview graph", |a| a.app_state().lsp.pins.pins().first().is_some_and(|p| p.graph().is_some()));
    app.step();
    assert_eq!(app.app_state().node_store.file_nodes.len(), 1, "a preview opens nothing");
    spatialcoding::ui::symbol_pins::pin(None, app.app_state_mut(), util, 0, 7, false);
    wait_until(&mut app, "main.rs to open", |a| a.app_state().node_store.file_nodes.len() == 2);
    assert!(app.app_state().node_store.file_nodes.values().any(|n| n.path.ends_with("src/main.rs")));
    assert!(!app.app_state().lsp.pins.pins()[0].preview);
}

#[test]
fn reveal_through_collapse_keeps_the_files_of_pinned_symbols_visible() {
    let fx = fixture();
    let mut app = app_with_fake_lsp(&fx);
    let util = app.open(&fx.path("src/util.rs"));
    let main = app.open(&fx.path("src/main.rs"));
    app.app_state_mut().settings.lsp.auto_open_matches = false;
    spatialcoding::ui::symbol_pins::pin(None, app.app_state_mut(), util, 0, 7, false);
    wait_until(&mut app, "the pin graph", |a| a.app_state().lsp.pins.pins().first().is_some_and(|p| p.graph().is_some()));
    app.app_state_mut().settings.lsp.reveal_through_collapse = false;
    assert!(!spatialcoding::folder_actions::folder_view(app.app_state()).revealed_files.contains(&main));
    app.app_state_mut().settings.lsp.reveal_through_collapse = true;
    let view = spatialcoding::folder_actions::folder_view(app.app_state());
    assert!(view.revealed_files.contains(&main) && view.revealed_files.contains(&util), "the pin's results and its token");
}

#[test]
fn the_activity_line_names_the_language_server_while_it_loads() {
    let fx = fixture();
    // A launcher that never finishes starting keeps the server in Starting.
    let gate = std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let held = gate.clone();
    let mut app = app_with(&fx);
    app.app_state_mut().lsp.manager.set_launcher(Arc::new(move |_, _| {
        let (lock, cv) = &*held;
        let mut open = lock.lock().unwrap();
        while !*open {
            open = cv.wait(open).unwrap();
        }
        Err("released".into())
    }));
    // Not `app.open`: it waits for the (held) servers.
    let util = spatialcoding::workspace::open_file(app.app_state_mut(), &fx.path("src/util.rs"));
    app.app_state_mut().settings.show_activity_indicator = true;
    spatialcoding::ui::symbol_pins::pin(None, app.app_state_mut(), util, 0, 7, false);
    app.step();
    app.step();
    let root = app.app_state().loaded_dir.clone().unwrap();
    assert_eq!(
        app.app_state().lsp.manager.loading_message(&root).as_deref(),
        Some("Rust language server loading\u{2026}")
    );
    let painted = frame_shapes(&app).iter().any(|s| matches!(s, egui::Shape::Text(t) if t.galley.job.text == "Rust language server loading\u{2026}"));
    assert!(painted, "the activity line is on the canvas");
    // Release the launcher so the worker thread ends.
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
}
