use egui::{Key, Modifiers, PointerButton, Pos2, Vec2};

use crate::common::*;

/// Two nodes side by side at zoom 0.5, `a` centered on `CANVAS_SPOT`.
/// Returns (app, a, b, screen center of a, screen center of b).
fn two_nodes() -> (App, Fixture, u64, u64, Pos2, Pos2) {
    two_nodes_stepping(DEFAULT_STEP_DT)
}

fn two_nodes_stepping(step_dt: f32) -> (App, Fixture, u64, u64, Pos2, Pos2) {
    let fx = fixture();
    let mut app = app_with_stepping(&fx, step_dt);
    let a = app.open(&fx.path("src/main.rs"));
    let b = app.open(&fx.path("src/util.rs"));
    {
        let vp = &mut app.app_state_mut().viewport;
        vp.zoom = 0.5;
        vp.target_zoom = 0.5;
    }
    let a_center = app.node_center(a);
    app.show_at(a_center, CANVAS_SPOT);
    let vp = &app.app_state().viewport;
    let (sa, sb) = (
        vp.world_to_screen(app.node_center(a)),
        vp.world_to_screen(app.node_center(b)),
    );
    (app, fx, a, b, sa, sb)
}

#[test]
fn click_selects_shift_click_toggles_and_empty_click_clears() {
    let (mut app, _fx, a, b, sa, sb) = two_nodes();

    click_at(&mut app, sa, Modifiers::NONE);
    assert_eq!(app.selected(), [a]);
    assert_eq!(app.app_state().selection.primary, Some(a));

    click_at(&mut app, sb, Modifiers::SHIFT);
    assert_eq!(app.selected(), [a, b]);
    assert_eq!(app.app_state().selection.primary, Some(b));

    click_at(&mut app, sa, Modifiers::SHIFT);
    assert_eq!(app.selected(), [b]);

    click_at(&mut app, sa, Modifiers::NONE);
    assert_eq!(app.selected(), [a], "plain click replaces the selection");

    click_at(&mut app, Pos2::new(750.0, 800.0), Modifiers::NONE);
    assert!(app.selected().is_empty());
}

#[test]
fn dragging_moves_the_selected_node_in_world_units() {
    let (mut app, _fx, a, b, sa, _) = two_nodes();
    click_at(&mut app, sa, Modifiers::NONE);
    let before = app.node_position(a);
    let other_before = app.node_position(b);

    drag(&mut app, sa, sa + Vec2::new(100.0, 50.0));

    let moved = app.node_position(a) - before;
    // Screen delta / zoom 0.5; egui may swallow the first few pixels before the drag starts.
    assert!(
        moved.x > 150.0 && moved.x <= 200.0 + 1e-3,
        "moved {moved:?}"
    );
    assert!(moved.y > 75.0 && moved.y <= 100.0 + 1e-3, "moved {moved:?}");
    assert_eq!(
        app.node_position(b),
        other_before,
        "unselected node stays put"
    );
}

#[test]
fn right_drag_pans_without_moving_the_selected_node() {
    let (mut app, _fx, a, _, sa, _) = two_nodes();
    click_at(&mut app, sa, Modifiers::NONE);
    let (node_before, offset_before) = (app.node_position(a), app.app_state().viewport.offset);

    drag_with(
        &mut app,
        PointerButton::Secondary,
        sa,
        sa + Vec2::new(100.0, 50.0),
    );

    assert_ne!(
        app.app_state().viewport.offset,
        offset_before,
        "view panned"
    );
    assert_eq!(app.node_position(a), node_before, "selected node stays put");
}

#[test]
fn space_drag_pans_without_moving_the_selected_node() {
    let (mut app, _fx, a, _, sa, _) = two_nodes();
    click_at(&mut app, sa, Modifiers::NONE);
    let (node_before, offset_before) = (app.node_position(a), app.app_state().viewport.offset);

    hold_key(&mut app, Key::Space, true);
    drag(&mut app, sa, sa + Vec2::new(100.0, 50.0));
    hold_key(&mut app, Key::Space, false);

    assert_ne!(
        app.app_state().viewport.offset,
        offset_before,
        "view panned"
    );
    assert_eq!(app.node_position(a), node_before, "selected node stays put");
}

#[test]
fn ctrl_wheel_zooms_around_the_cursor() {
    let (mut app, _fx, _, _, _, _) = two_nodes();
    let anchor = app.app_state().viewport.screen_to_world(CANVAS_SPOT);

    wheel(
        &mut app,
        CANVAS_SPOT,
        Vec2::new(0.0, 100.0),
        Modifiers::CTRL,
    );

    let vp = &app.app_state().viewport;
    assert!(vp.zoom > 0.5, "zoomed in: {}", vp.zoom);
    assert_close(vp.screen_to_world(CANVAS_SPOT), anchor);

    wheel(
        &mut app,
        CANVAS_SPOT,
        Vec2::new(0.0, -400.0),
        Modifiers::CTRL,
    );
    assert!(app.app_state().viewport.zoom < 0.5, "zoomed out");
}

#[test]
fn smooth_zoom_animates_over_frames_keeping_the_cursor_anchored() {
    let (mut app, ..) = two_nodes_stepping(1.0 / 60.0);
    assert!(app.app_state().settings.smooth_zoom, "on by default");
    let anchor = app.app_state().viewport.screen_to_world(CANVAS_SPOT);

    wheel_frame(
        &mut app,
        CANVAS_SPOT,
        Vec2::new(0.0, 100.0),
        Modifiers::CTRL,
    );

    let (first, target) = {
        let vp = &app.app_state().viewport;
        (vp.zoom, vp.target_zoom)
    };
    assert!(target > 0.5, "target zoomed in: {target}");
    assert!(
        first > 0.5 && first < target,
        "first frame is part way: {first} of {target}"
    );
    assert!(wants_repaint(&app), "keeps repainting while animating");
    assert_close(
        app.app_state().viewport.screen_to_world(CANVAS_SPOT),
        anchor,
    );

    let mut prev = first;
    for frame in 0..120 {
        app.step();
        let vp = &app.app_state().viewport;
        assert!(
            vp.zoom >= prev,
            "frame {frame}: zoom went back {prev} -> {}",
            vp.zoom
        );
        assert_close(vp.screen_to_world(CANVAS_SPOT), anchor);
        prev = vp.zoom;
    }
    let vp = &app.app_state().viewport;
    assert_eq!(vp.zoom, target);
    assert!(vp.zoom_center.is_none(), "animation finished");
}

#[test]
fn without_smooth_zoom_the_zoom_applies_at_once() {
    let (mut app, ..) = two_nodes_stepping(1.0 / 60.0);
    app.app_state_mut().settings.smooth_zoom = false;
    let anchor = app.app_state().viewport.screen_to_world(CANVAS_SPOT);

    wheel_frame(
        &mut app,
        CANVAS_SPOT,
        Vec2::new(0.0, 100.0),
        Modifiers::CTRL,
    );

    let vp = &app.app_state().viewport;
    assert!(vp.zoom > 0.5);
    assert_eq!(vp.zoom, vp.target_zoom, "no animation");
    assert_close(vp.screen_to_world(CANVAS_SPOT), anchor);
}

#[test]
fn plain_wheel_pans() {
    let (mut app, _fx, _, _, _, _) = two_nodes();
    let before = app.app_state().viewport.offset;

    wheel(
        &mut app,
        CANVAS_SPOT,
        Vec2::new(0.0, -40.0),
        Modifiers::NONE,
    );

    let vp = &app.app_state().viewport;
    assert_eq!(vp.zoom, 0.5, "no zoom without Ctrl");
    assert!(
        vp.offset.y < before.y,
        "panned: {:?} -> {:?}",
        before,
        vp.offset
    );
    assert_eq!(vp.offset.x, before.x);
}

#[test]
fn shift_f_fits_every_node_on_screen() {
    let (mut app, _fx, a, b, _, _) = two_nodes();
    app.show_at(Pos2::new(50_000.0, 50_000.0), CANVAS_SPOT);

    app.press_key_modifiers(Modifiers::SHIFT, egui::Key::F);
    app.run();

    let s = app.app_state();
    let screen = egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 900.0));
    let visible = s.viewport.visible_world_rect(screen);
    for id in [a, b] {
        let rect = s
            .node_store
            .get_file(id)
            .unwrap()
            .world_rect(s.settings.node_width, s.settings.font_size);
        assert!(visible.contains_rect(rect), "node {id} off screen");
    }
}

#[test]
fn f_centers_the_view_on_the_point_under_the_mouse() {
    let (mut app, _fx, _, _, _, _) = two_nodes();
    let mouse = Pos2::new(1000.0, 300.0);
    let target = app.app_state().viewport.screen_to_world(mouse);
    app.input_mut()
        .events
        .push(egui::Event::PointerMoved(mouse));
    app.step();

    app.press_key(egui::Key::F);
    app.run();

    // The point under the mouse lands at the middle of the canvas, which
    // excludes the side panels.
    let center = app.app_state().canvas_rect.center();
    assert_close(app.app_state().viewport.world_to_screen(target), center);
}

#[test]
fn escape_clears_the_selection() {
    let (mut app, _fx, a, _, sa, _) = two_nodes();
    click_at(&mut app, sa, Modifiers::NONE);
    assert_eq!(app.selected(), [a]);

    app.press_key(egui::Key::Escape);
    app.run();
    assert!(app.selected().is_empty());
}

/// Every shape of the last frame, with nested `Shape::Vec`s flattened.
/// Strongest alpha among bezier edges whose stroke is blue-dominant.
fn max_bezier_alpha(shapes: &[egui::Shape]) -> u8 {
    shapes
        .iter()
        .filter_map(|s| match s {
            egui::Shape::CubicBezier(b) => match b.stroke.color {
                egui::epaint::ColorMode::Solid(c) => {
                    let [r, g, bl, a] = c.to_srgba_unmultiplied();
                    (bl > r && bl >= g).then_some(a)
                }
                _ => None,
            },
            _ => None,
        })
        .max()
        .unwrap_or(0)
}

#[test]
fn selecting_a_node_lifts_its_edges_without_adding_symbol_badges() {
    let fx = fixture();
    let mut app = app_with(&fx);
    let main = app.open(&fx.path("src/main.rs"));
    let util = app.open(&fx.path("src/util.rs"));
    assert_eq!(
        app.app_state().edge_store.all_edges().count(),
        2,
        "main imports util and references util::hello"
    );
    {
        let s = app.app_state_mut();
        s.node_store.get_file_mut(main).unwrap().position = Pos2::new(0.0, 0.0);
        s.node_store.get_file_mut(util).unwrap().position = Pos2::new(900.0, 0.0);
        s.viewport.zoom = 0.6;
        s.viewport.target_zoom = 0.6;
        s.right_panel = None;
        // Alphas are read from solid strokes; the default gradient paints
        // per-vertex colors instead.
        s.edge_style.use_gradient = false;
    }
    app.show_at(Pos2::new(800.0, 60.0), Pos2::new(700.0, 300.0));

    let idle = frame_shapes(&app);
    let idle_alpha = max_bezier_alpha(&idle);
    // Depth opacity: both files share `src/`, so the idle edge is fully
    // opaque (folder distance 0). Far-apart folders fade (geometry tests).
    assert_eq!(idle_alpha, 255, "same-folder idle edge");

    app.app_state_mut().selection.select(main);
    app.run();
    let selected = frame_shapes(&app);
    assert!(
        max_bezier_alpha(&selected) > 200,
        "the selection's edge is opaque light blue"
    );
    assert_eq!(
        texts_equal(&selected, "hello") - texts_equal(&idle, "hello"),
        0,
        "selection must not add blue symbol-name badges"
    );
}

fn painted_token_rect(app: &App, token: &str, within: egui::Rect) -> egui::Rect {
    frame_shapes(app)
        .iter()
        .find_map(|shape| {
            let egui::Shape::Text(text) = shape else {
                return None;
            };
            let at = text.galley.job.text.find(token)?;
            let chars = text.galley.job.text[..at].chars().count();
            let left = text
                .galley
                .pos_from_ccursor(egui::text::CCursor::new(chars));
            let right = text
                .galley
                .pos_from_ccursor(egui::text::CCursor::new(chars + token.chars().count()));
            let rect = egui::Rect::from_min_max(left.min, egui::pos2(right.min.x, left.max.y))
                .translate(text.pos.to_vec2());
            within.contains(rect.center()).then_some(rect)
        })
        .unwrap_or_else(|| panic!("token {token} not painted within {within:?}"))
}

#[test]
fn edge_endpoints_touch_the_usage_and_definition_glyphs_at_different_zooms() {
    let fx = fixture();
    let mut app = app_with(&fx);
    let source = app.open(&fx.path("src/main.rs"));
    let target = app.open(&fx.path("src/util.rs"));
    app.set_size(Vec2::new(2400.0, 900.0));
    {
        let s = app.app_state_mut();
        // Glyph anchors are the Ports-off mode.
        s.edge_style.show_ports = false;
        s.left_panel = None;
        s.edge_style.relationships
            [spatialcoding::edges::style::RelationshipType::Imports.index()]
        .visible = false;
        s.node_store.get_file_mut(source).unwrap().position = Pos2::ZERO;
        s.node_store.get_file_mut(target).unwrap().position = Pos2::new(900.0, 0.0);
        s.selection.select(source);
    }
    for zoom in [0.6, 1.0, 1.4] {
        app.app_state_mut().viewport.zoom = zoom;
        app.app_state_mut().viewport.target_zoom = zoom;
        app.show_at(Pos2::new(800.0, 60.0), Pos2::new(1200.0, 300.0));
        let screen_rect = |id| {
            let s = app.app_state();
            s.viewport.world_to_screen_rect(
                s.node_store
                    .get_file(id)
                    .unwrap()
                    .world_rect(s.settings.node_width, s.settings.font_size),
            )
        };
        let usage = painted_token_rect(&app, "hello", screen_rect(source));
        let definition = painted_token_rect(&app, "hello", screen_rect(target));
        let endpoints = frame_shapes(&app)
            .iter()
            .find_map(|shape| match shape {
                egui::Shape::CubicBezier(b) => Some((b.points[0], b.points[3])),
                _ => None,
            })
            .unwrap();
        assert!(
            (endpoints.0 - usage.right_center()).length() < 1.5,
            "source must touch the usage name at zoom {zoom}: {endpoints:?} vs {usage:?}"
        );
        assert!(
            (endpoints.1 - definition.left_center()).length() < 1.5,
            "arrow must touch the definition name at zoom {zoom}: {endpoints:?} vs {definition:?}"
        );
    }
}

#[test]
fn ports_mark_connected_lines_on_the_border_and_edges_end_there() {
    let fx = fixture();
    let mut app = app_with(&fx);
    let source = app.open(&fx.path("src/main.rs"));
    let target = app.open(&fx.path("src/util.rs"));
    app.set_size(Vec2::new(2400.0, 900.0));
    {
        let s = app.app_state_mut();
        s.left_panel = None;
        assert!(s.edge_style.show_ports, "ports are on by default");
        s.edge_style.relationships
            [spatialcoding::edges::style::RelationshipType::Imports.index()]
        .visible = false;
        s.node_store.get_file_mut(source).unwrap().position = Pos2::ZERO;
        s.node_store.get_file_mut(target).unwrap().position = Pos2::new(900.0, 0.0);
        s.selection.select(source);
    }
    app.show_at(Pos2::new(800.0, 60.0), Pos2::new(1200.0, 300.0));
    let s = app.app_state();
    let edge = s
        .edge_store
        .all_edges()
        .find(|e| e.relationship == spatialcoding::edges::style::RelationshipType::References)
        .unwrap();
    let port = |id, line: usize, right: bool| {
        let node = s.node_store.get_file(id).unwrap();
        let rect = node.world_rect(s.settings.node_width, s.settings.font_size);
        let y = node.line_anchor_y(line, s.settings.font_size).unwrap();
        let x = if right { rect.right() } else { rect.left() };
        s.viewport.world_to_screen(Pos2::new(x, y))
    };
    let usage_port = port(source, edge.source_span.unwrap().start, true);
    let definition_port = port(target, edge.target_span.unwrap().start, false);
    let shapes = frame_shapes(&app);
    let endpoints = shapes
        .iter()
        .find_map(|shape| match shape {
            egui::Shape::CubicBezier(b) => Some((b.points[0], b.points[3])),
            _ => None,
        })
        .unwrap();
    assert!((endpoints.0 - usage_port).length() < 0.5, "{endpoints:?} vs {usage_port:?}");
    assert!((endpoints.1 - definition_port).length() < 0.5, "{endpoints:?} vs {definition_port:?}");
    for at in [usage_port, definition_port] {
        assert!(
            shapes.iter().any(|shape| matches!(
                shape,
                egui::Shape::Circle(c) if (c.center - at).length() < 0.5 && c.fill.a() > 0
            )),
            "a port circle is painted at {at:?}"
        );
        assert!(
            shapes.iter().any(|shape| matches!(
                shape,
                egui::Shape::Rect(r) if r.rect.min.y < at.y && at.y < r.rect.max.y
                    && r.rect.width() > 100.0 && r.fill.a() > 0 && r.fill.a() < 80
            )),
            "a translucent band marks the row at {at:?}"
        );
    }

    app.app_state_mut().edge_style.show_ports = false;
    app.run();
    assert!(
        !frame_shapes(&app).iter().any(|shape| matches!(
            shape,
            egui::Shape::Circle(c) if (c.center - usage_port).length() < 0.5
        )),
        "Ports off paints no circles"
    );

    // Title-only zoom draws blocks: no ports even with Ports on.
    {
        let s = app.app_state_mut();
        s.edge_style.show_ports = true;
        s.viewport.zoom = 0.08;
        s.viewport.target_zoom = 0.08;
    }
    app.run();
    assert!(
        !frame_shapes(&app)
            .iter()
            .any(|shape| matches!(shape, egui::Shape::Circle(_))),
        "title-only zoom paints no ports"
    );
}

#[test]
fn file_nodes_show_and_navigate_to_code_after_line_fifty() {
    use spatialcoding::nodes::file_node::{line_height, CODE_PADDING, CODE_PAD_X, HEADER_HEIGHT};
    let fx = fixture();
    let content = "// filler\n".repeat(119) + "pub fn last_line() {}\n";
    std::fs::write(fx.path("src/util.rs"), content).unwrap();
    std::fs::write(
        fx.path("src/main.rs"),
        "mod util;\nfn main() { util::last_line(); }\n",
    )
    .unwrap();
    let mut app = app_with(&fx);
    // The edge must arrive from the right: keep the grid placement (main.rs
    // right of util.rs) instead of the auto layout's importer-left order.
    app.app_state_mut().layout_settings.auto_layout = false;
    app.app_state_mut().edge_style.show_ports = false;
    app.app_state_mut().edge_style.relationships
        [spatialcoding::edges::style::RelationshipType::Imports.index()]
    .visible = false;
    let id = app.open(&fx.path("src/util.rs"));
    app.open(&fx.path("src/main.rs"));
    let s = app.app_state();
    let node = s.node_store.get_file(id).unwrap();
    let last_y =
        node.position.y + HEADER_HEIGHT + CODE_PADDING + 119.5 * line_height(s.settings.font_size);
    assert!(
        node.world_rect(s.settings.node_width, s.settings.font_size)
            .bottom()
            > last_y,
        "node must include the complete file"
    );
    spatialcoding::workspace::focus_location(app.app_state_mut(), &fx.path("src/util.rs"), 119);
    app.run();
    assert!(
        frame_shapes(&app).iter().any(
            |shape| matches!(shape, egui::Shape::Text(t) if t.galley.job.text.contains("last_line"))
        ),
        "last source line must be painted"
    );
    let s = app.app_state();
    let last_on_screen = s.viewport.world_to_screen(Pos2::new(
        s.node_store.get_file(id).unwrap().position.x,
        last_y,
    ));
    assert!(
        (300.0..600.0).contains(&last_on_screen.y),
        "navigation reaches the real line, not line 50"
    );
    let file_rect = s.viewport.world_to_screen_rect(
        s.node_store
            .get_file(id)
            .unwrap()
            .world_rect(s.settings.node_width, s.settings.font_size),
    );
    let name = painted_token_rect(&app, "last_line", file_rect);
    let target = frame_shapes(&app)
        .iter()
        .find_map(|shape| match shape {
            egui::Shape::CubicBezier(b) => Some(b.points[3]),
            _ => None,
        })
        .unwrap();
    assert!(
        (target - name.right_center()).length() < 1.5,
        "edge reaches a late definition from the right"
    );
}

#[test]
fn minified_edges_touch_identifier_bars_and_collapsed_nodes_use_header_fallbacks() {
    use spatialcoding::nodes::file_node::{line_height, CODE_PADDING, CODE_PAD_X, HEADER_HEIGHT};
    let fx = fixture();
    let mut app = app_with(&fx);
    let source = app.open(&fx.path("src/main.rs"));
    let target = app.open(&fx.path("src/util.rs"));
    {
        let s = app.app_state_mut();
        s.edge_style.show_ports = false;
        s.left_panel = None;
        s.edge_style.relationships
            [spatialcoding::edges::style::RelationshipType::Imports.index()]
        .visible = false;
        s.node_store.get_file_mut(source).unwrap().position = Pos2::ZERO;
        s.node_store.get_file_mut(target).unwrap().position = Pos2::new(900.0, 0.0);
        s.viewport.zoom = 0.25;
        s.viewport.target_zoom = 0.25;
        s.edge_style.hide_short_pct = 0.0;
        s.selection.select(source);
    }
    app.show_at(Pos2::new(800.0, 60.0), CANVAS_SPOT);
    let s = app.app_state();
    let edge = s
        .edge_store
        .all_edges()
        .find(|e| e.relationship == spatialcoding::edges::style::RelationshipType::References)
        .unwrap();
    let name = edge.name_span.unwrap();
    let node = s.node_store.get_file(target).unwrap();
    let line = node.content.lines().nth(name.line).unwrap();
    let col = line[..name.start_byte].chars().count() as f32;
    let expected = s.viewport.world_to_screen(egui::pos2(
        node.position.x + CODE_PAD_X + col * s.settings.font_size * 0.6,
        node.position.y
            + HEADER_HEIGHT
            + CODE_PADDING
            + (name.line as f32 + 0.5) * line_height(s.settings.font_size),
    ));
    let end = |app: &App| {
        frame_shapes(app)
            .iter()
            .find_map(|shape| match shape {
                egui::Shape::CubicBezier(b) => Some(b.points[3]),
                _ => None,
            })
            .unwrap()
    };
    assert!(
        (end(&app) - expected).length() < 0.01,
        "minified edge touches the name bar"
    );

    app.app_state_mut()
        .node_store
        .get_file_mut(target)
        .unwrap()
        .collapsed = true;
    app.run();
    let s = app.app_state();
    let node = s.node_store.get_file(target).unwrap();
    let fallback = s
        .viewport
        .world_to_screen(node.position + egui::vec2(0.0, HEADER_HEIGHT * 0.5));
    assert!(
        (end(&app) - fallback).length() < 0.01,
        "collapsed definition uses its header"
    );
}

/// Locate a real painted token, not an approximation of the monospace advance.
pub fn token_center(app: &App, token: &str) -> Pos2 {
    fn in_shape(shape: &egui::Shape, token: &str, clip: egui::Rect) -> Option<Pos2> {
        if let egui::Shape::Vec(shapes) = shape {
            return shapes.iter().find_map(|s| in_shape(s, token, clip));
        }
        let egui::Shape::Text(text) = shape else {
            return None;
        };
        let at = text.galley.job.text.find(token)?;
        let chars = text.galley.job.text[..at].chars().count();
        let left = text
            .galley
            .pos_from_ccursor(egui::text::CCursor::new(chars));
        let right = text
            .galley
            .pos_from_ccursor(egui::text::CCursor::new(chars + token.chars().count()));
        let at = text.pos + egui::vec2((left.min.x + right.min.x) * 0.5, left.center().y);
        clip.contains(at).then_some(at)
    }
    app.output()
        .shapes
        .iter()
        .find_map(|clipped| in_shape(&clipped.shape, token, clipped.clip_rect))
        .unwrap_or_else(|| panic!("token not painted: {token}"))
}

pub fn symbol_canvas() -> (App, Fixture, u64, u64) {
    let fx = fixture();
    std::fs::write(
        fx.path("web/a.ts"),
        "export const value = 1;\nexport function greet() {}\nexport class Client {}\n",
    )
    .unwrap();
    std::fs::write(
        fx.path("web/app.ts"),
        "console.log(value);\ngreet();\nnew Client();\n",
    )
    .unwrap();
    let mut app = app_with(&fx);
    let uses = app.open(&fx.path("web/app.ts"));
    let defs = app.open(&fx.path("web/a.ts"));
    {
        let s = app.app_state_mut();
        s.node_store.get_file_mut(defs).unwrap().position = Pos2::ZERO;
        s.node_store.get_file_mut(uses).unwrap().position = Pos2::new(900.0, 0.0);
        s.viewport.zoom = 1.0;
        s.viewport.target_zoom = 1.0;
        s.left_panel = None;
    }
    app.show_at(Pos2::new(350.0, 40.0), CANVAS_SPOT);
    (app, fx, uses, defs)
}

fn bright_edges(app: &App) -> usize {
    frame_shapes(app)
        .iter()
        .filter(|shape| match shape {
            egui::Shape::CubicBezier(b) => matches!(b.stroke.color,
            egui::epaint::ColorMode::Solid(c) if c.to_srgba_unmultiplied()[3] > 200),
            _ => false,
        })
        .count()
}

#[test]
fn clicking_variable_function_and_class_names_focuses_only_their_edges() {
    let (mut app, _fx, _, defs) = symbol_canvas();
    assert_eq!(app.app_state().edge_store.all_edges().count(), 3);
    for name in ["value", "greet", "Client"] {
        let at = token_center(&app, name);
        click_at(&mut app, at, Modifiers::NONE);
        assert_eq!(app.app_state().selection.symbols().len(), 1, "{name}");
        assert_eq!(app.app_state().selection.symbols()[0].node_id, defs);
        assert_eq!(
            bright_edges(&app),
            1,
            "only {name}'s edge, not all file dependencies"
        );
    }
}

#[test]
fn shift_click_toggles_symbols_and_file_or_empty_click_and_escape_clear_them() {
    let (mut app, _fx, _, defs) = symbol_canvas();
    let a = token_center(&app, "value");
    let b = token_center(&app, "greet");
    click_at(&mut app, a, Modifiers::NONE);
    click_at(&mut app, b, Modifiers::SHIFT);
    assert_eq!(app.app_state().selection.symbols().len(), 2);
    assert_eq!(bright_edges(&app), 2);
    click_at(&mut app, a, Modifiers::SHIFT);
    assert_eq!(app.app_state().selection.symbols().len(), 1);
    assert_eq!(bright_edges(&app), 1);

    // Blank padding in the file keeps ordinary whole-file selection available.
    let padding = app
        .app_state()
        .viewport
        .world_to_screen(Pos2::new(600.0, 10.0));
    click_at(&mut app, padding, Modifiers::NONE);
    assert_eq!(app.selected(), [defs]);
    assert!(app.app_state().selection.symbols().is_empty());
    assert_eq!(bright_edges(&app), 3);
    click_at(&mut app, a, Modifiers::NONE);
    app.press_key(egui::Key::Escape);
    app.run();
    assert!(app.app_state().selection.symbols().is_empty());
    click_at(&mut app, a, Modifiers::NONE);
    click_at(&mut app, Pos2::new(800.0, 800.0), Modifiers::NONE);
    assert!(app.app_state().selection.symbols().is_empty());
}

#[test]
fn clicking_an_lsp_usage_selects_its_definition_without_moving_the_camera() {
    let (mut app, _fx, uses, defs) = symbol_canvas();
    app.show_at(Pos2::new(1100.0, 40.0), CANVAS_SPOT);
    let before = app.app_state().viewport.offset;
    let at = token_center(&app, "greet");
    click_at(&mut app, at, Modifiers::NONE);
    let s = app.app_state();
    assert_eq!(
        s.selection.primary,
        Some(uses),
        "clicked file remains the drag target"
    );
    assert_eq!(s.selection.symbols().len(), 1);
    assert_eq!(s.selection.symbols()[0].node_id, defs);
    assert_eq!(s.viewport.offset, before);
    spatialcoding::workspace::remove_nodes(app.app_state_mut(), &[defs]);
    assert!(
        app.app_state().selection.symbols().is_empty(),
        "closed definitions cannot leave stale focus"
    );
}

#[test]
fn an_unreferenced_definition_is_selectable_and_edge_rebuilds_keep_symbol_focus() {
    let (mut app, _fx, uses, defs) = symbol_canvas();
    let at = token_center(&app, "greet");
    click_at(&mut app, at, Modifiers::NONE);
    let selected = app.app_state().selection.symbols().to_vec();
    spatialcoding::workspace::rebuild_edges(app.app_state_mut());
    app.settle_edges();
    assert_eq!(app.app_state().selection.symbols(), selected);
    assert_eq!(bright_edges(&app), 1, "focus is independent of edge IDs");

    spatialcoding::workspace::remove_nodes(app.app_state_mut(), &[uses]);
    app.app_state_mut().selection.clear();
    app.run();
    let at = token_center(&app, "greet");
    click_at(&mut app, at, Modifiers::NONE);
    assert_eq!(app.app_state().selection.symbols().len(), 1);
    assert_eq!(app.app_state().selection.symbols()[0].node_id, defs);
    assert_eq!(bright_edges(&app), 0, "there are no cross-file edges left");
    let spans = app
        .app_state()
        .edge_store
        .focused_spans(&app.app_state().selection, &app.app_state().edge_style);
    assert_eq!(spans[&defs].len(), 1, "the declaration stays marked");
}
