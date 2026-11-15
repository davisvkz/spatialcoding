use egui_kittest::kittest::Queryable;
use spatialcoding::cli::StartupOptions;

use crate::common::*;

#[test]
fn open_and_layout_options_prepare_the_canvas_and_fit_the_view() {
    let fx = fixture();
    let mut app = app();
    let opts = StartupOptions {
        dir: Some(fx.root.clone()),
        open: vec!["src/main.rs".into(), "src/util.rs".into()],
        open_all: false,
        layout: true,
    };

    app.state_mut().apply_startup(&opts);
    // The layout is applied again once the language server answered.
    app.settle_edges();

    let s = app.app_state();
    assert_eq!(s.node_store.file_nodes.len(), 2);
    assert_eq!(
        s.edge_store.all_edges().count(),
        2,
        "module import plus symbol reference"
    );
    let by_name = |name: &str| {
        s.node_store
            .file_nodes
            .values()
            .find(|n| n.file_name() == name)
            .unwrap()
    };
    assert!(by_name("util.rs").position.x > by_name("main.rs").position.x);

    // The first frame fitted both nodes inside the canvas.
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0));
    let visible = s.viewport.visible_world_rect(screen);
    for node in s.node_store.file_nodes.values() {
        let rect = node.world_rect(s.settings.node_width, s.settings.font_size);
        assert!(
            visible.contains_rect(rect),
            "{} off screen",
            node.file_name()
        );
    }
    app.get_by_label(&fx.name);
}

#[test]
fn open_all_option_opens_every_text_file() {
    let fx = fixture();
    let mut app = app();
    app.state_mut().apply_startup(&StartupOptions {
        dir: Some(fx.root.clone()),
        open_all: true,
        ..Default::default()
    });
    app.settle_edges();
    assert_eq!(app.app_state().node_store.file_nodes.len(), 6);
}

#[test]
fn no_directory_keeps_the_welcome_screen() {
    let mut app = app();
    app.state_mut().apply_startup(&StartupOptions::default());
    app.run();
    app.get_by_label("Open folder...");
}
