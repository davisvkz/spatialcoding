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
    assert!(moved.x > 150.0 && moved.x <= 200.0 + 1e-3, "moved {moved:?}");
    assert!(moved.y > 75.0 && moved.y <= 100.0 + 1e-3, "moved {moved:?}");
    assert_eq!(app.node_position(b), other_before, "unselected node stays put");
}

#[test]
fn right_drag_pans_without_moving_the_selected_node() {
    let (mut app, _fx, a, _, sa, _) = two_nodes();
    click_at(&mut app, sa, Modifiers::NONE);
    let (node_before, offset_before) = (app.node_position(a), app.app_state().viewport.offset);

    drag_with(&mut app, PointerButton::Secondary, sa, sa + Vec2::new(100.0, 50.0));

    assert_ne!(app.app_state().viewport.offset, offset_before, "view panned");
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

    assert_ne!(app.app_state().viewport.offset, offset_before, "view panned");
    assert_eq!(app.node_position(a), node_before, "selected node stays put");
}

#[test]
fn ctrl_wheel_zooms_around_the_cursor() {
    let (mut app, _fx, _, _, _, _) = two_nodes();
    let anchor = app.app_state().viewport.screen_to_world(CANVAS_SPOT);

    wheel(&mut app, CANVAS_SPOT, Vec2::new(0.0, 100.0), Modifiers::CTRL);

    let vp = &app.app_state().viewport;
    assert!(vp.zoom > 0.5, "zoomed in: {}", vp.zoom);
    assert_close(vp.screen_to_world(CANVAS_SPOT), anchor);

    wheel(&mut app, CANVAS_SPOT, Vec2::new(0.0, -400.0), Modifiers::CTRL);
    assert!(app.app_state().viewport.zoom < 0.5, "zoomed out");
}

#[test]
fn smooth_zoom_animates_over_frames_keeping_the_cursor_anchored() {
    let (mut app, ..) = two_nodes_stepping(1.0 / 60.0);
    assert!(app.app_state().settings.smooth_zoom, "on by default");
    let anchor = app.app_state().viewport.screen_to_world(CANVAS_SPOT);

    wheel_frame(&mut app, CANVAS_SPOT, Vec2::new(0.0, 100.0), Modifiers::CTRL);

    let (first, target) = {
        let vp = &app.app_state().viewport;
        (vp.zoom, vp.target_zoom)
    };
    assert!(target > 0.5, "target zoomed in: {target}");
    assert!(first > 0.5 && first < target, "first frame is part way: {first} of {target}");
    assert!(wants_repaint(&app), "keeps repainting while animating");
    assert_close(app.app_state().viewport.screen_to_world(CANVAS_SPOT), anchor);

    let mut prev = first;
    for frame in 0..120 {
        app.step();
        let vp = &app.app_state().viewport;
        assert!(vp.zoom >= prev, "frame {frame}: zoom went back {prev} -> {}", vp.zoom);
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

    wheel_frame(&mut app, CANVAS_SPOT, Vec2::new(0.0, 100.0), Modifiers::CTRL);

    let vp = &app.app_state().viewport;
    assert!(vp.zoom > 0.5);
    assert_eq!(vp.zoom, vp.target_zoom, "no animation");
    assert_close(vp.screen_to_world(CANVAS_SPOT), anchor);
}

#[test]
fn plain_wheel_pans() {
    let (mut app, _fx, _, _, _, _) = two_nodes();
    let before = app.app_state().viewport.offset;

    wheel(&mut app, CANVAS_SPOT, Vec2::new(0.0, -40.0), Modifiers::NONE);

    let vp = &app.app_state().viewport;
    assert_eq!(vp.zoom, 0.5, "no zoom without Ctrl");
    assert!(vp.offset.y < before.y, "panned: {:?} -> {:?}", before, vp.offset);
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
    app.input_mut().events.push(egui::Event::PointerMoved(mouse));
    app.step();

    app.press_key(egui::Key::F);
    app.run();

    let center = Pos2::new(700.0, 450.0); // window is 1400x900
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
