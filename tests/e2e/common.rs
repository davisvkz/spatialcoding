//! Shared fixture and input helpers for the e2e tests.

use std::path::{Path, PathBuf};

use egui::{Event, Modifiers, MouseWheelUnit, PointerButton, Pos2, Vec2};
use egui_kittest::Harness;
use spatialcoding::app::SpatialCodingApp;
use spatialcoding::state::AppState;

use crate::fake_lsp;

pub type App = Harness<'static, SpatialCodingApp>;

/// A point well inside the canvas: right of the left sidebar, below the
/// toolbar, left of the collapsed right sidebar.
pub const CANVAS_SPOT: Pos2 = Pos2::new(750.0, 450.0);

/// A small multi-language project on disk.
///
/// ```text
/// demo-proj*/
///   Cargo.toml  README.md
///   src/main.rs   (mod util;)   src/util.rs
///   web/app.ts    (import ./a)  web/a.ts
///   assets/logo.png             (binary)
///   .hidden/secret.txt          (dot folder, hidden by default)
///   target/debug/out.txt        (always skipped)
/// ```
pub struct Fixture {
    _dir: tempfile::TempDir,
    pub root: PathBuf,
    pub name: String,
}

impl Fixture {
    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
}

pub fn fixture() -> Fixture {
    let dir = tempfile::Builder::new().prefix("demo-proj").tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let write = |rel: &str, content: &[u8]| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    };
    write("Cargo.toml", b"[package]\nname = \"demo\"\n");
    write("README.md", b"# demo\n");
    write("src/main.rs", b"mod util;\n\nfn main() {\n    util::hello();\n}\n");
    write("src/util.rs", b"pub fn hello() {}\n");
    write("web/app.ts", b"import { a } from './a';\nconsole.log(a);\n");
    write("web/a.ts", b"export const a = 1;\n");
    write("assets/logo.png", b"\x89PNG\r\n\x1a\n\0\0\0");
    write(".hidden/secret.txt", b"hidden\n");
    write("target/debug/out.txt", b"build output\n");
    let name = root.file_name().unwrap().to_string_lossy().into_owned();
    Fixture { _dir: dir, root, name }
}

/// kittest's default frame time: long enough that animations finish in one frame.
pub const DEFAULT_STEP_DT: f32 = 1.0 / 4.0;

/// The real app in a 1400x900 window, with nothing loaded. Its language
/// servers are the in-process fake of `fake_lsp.rs`, so no test starts a real
/// one.
pub fn app() -> App {
    app_stepping(DEFAULT_STEP_DT)
}

/// Like [`app`], but each frame advances `step_dt` seconds (use 1/60 to watch
/// an animation frame by frame).
pub fn app_stepping(step_dt: f32) -> App {
    let mut harness = Harness::builder()
        .with_size(Vec2::new(1400.0, 900.0))
        .with_step_dt(step_dt)
        .build_eframe(|cc| SpatialCodingApp::new(cc));
    harness.state_mut().state_mut().lsp.manager.set_launcher(fake_lsp::launcher());
    harness.run();
    harness
}

/// The app with `fx` loaded as the workspace.
pub fn app_with(fx: &Fixture) -> App {
    app_with_stepping(fx, DEFAULT_STEP_DT)
}

pub fn app_with_stepping(fx: &Fixture, step_dt: f32) -> App {
    let mut harness = app_stepping(step_dt);
    harness.state_mut().open_folder(fx.root.clone());
    harness.run();
    harness
}

pub trait AppExt {
    fn app_state(&self) -> &AppState;
    fn app_state_mut(&mut self) -> &mut AppState;
    /// Opens a file of the workspace the same way a click in the tree does,
    /// and waits for the dependency edges.
    fn open(&mut self, path: &Path) -> u64;
    /// Steps frames until the language servers answered every question of
    /// the dependency indexer.
    fn settle_edges(&mut self);
    /// World-space center of a file node.
    fn node_center(&self, id: u64) -> Pos2;
    /// Pans the view so `world` shows at screen position `screen`.
    fn show_at(&mut self, world: Pos2, screen: Pos2);
    fn node_position(&self, id: u64) -> Pos2;
    fn selected(&self) -> Vec<u64>;
    /// Runs a command-palette command, as if it was picked.
    fn execute_palette_command(&mut self, command: spatialcoding::ui::command_palette::PaletteCommand);
}

impl AppExt for App {
    fn app_state(&self) -> &AppState {
        self.state().state()
    }

    fn app_state_mut(&mut self) -> &mut AppState {
        self.state_mut().state_mut()
    }

    fn open(&mut self, path: &Path) -> u64 {
        let id = spatialcoding::workspace::open_file(self.app_state_mut(), path);
        self.settle_edges();
        id
    }

    fn settle_edges(&mut self) {
        // `run()` gives up while the app repaints for pending answers: step.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        self.step();
        while self.app_state().deps.is_waiting() {
            assert!(std::time::Instant::now() < deadline, "timed out waiting for the dependency edges");
            std::thread::sleep(std::time::Duration::from_millis(5));
            self.step();
        }
        self.run();
    }

    fn node_center(&self, id: u64) -> Pos2 {
        let s = self.app_state();
        s.node_store
            .get_file(id)
            .unwrap()
            .world_rect(s.settings.node_width, s.settings.font_size)
            .center()
    }

    fn show_at(&mut self, world: Pos2, screen: Pos2) {
        let vp = &mut self.app_state_mut().viewport;
        vp.offset = screen.to_vec2() / vp.zoom - world.to_vec2();
        self.run();
    }

    fn node_position(&self, id: u64) -> Pos2 {
        self.app_state().node_store.get_file(id).unwrap().position
    }

    fn execute_palette_command(&mut self, command: spatialcoding::ui::command_palette::PaletteCommand) {
        self.state_mut().execute_command(command);
    }

    fn selected(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self.app_state().selection.selected_ids().collect();
        ids.sort();
        ids
    }
}

/// Queues one event and runs a frame for it.
fn send(harness: &mut App, event: Event) {
    harness.input_mut().events.push(event);
    harness.step();
}

fn button(harness: &mut App, pos: Pos2, pressed: bool, modifiers: Modifiers) {
    press(harness, pos, PointerButton::Primary, pressed, modifiers);
}

fn press(harness: &mut App, pos: Pos2, button: PointerButton, pressed: bool, modifiers: Modifiers) {
    send(
        harness,
        Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers,
        },
    );
}

/// Left click at a screen position, holding `modifiers`.
pub fn click_at(harness: &mut App, pos: Pos2, modifiers: Modifiers) {
    harness.input_mut().modifiers = modifiers;
    send(harness, Event::PointerMoved(pos));
    button(harness, pos, true, modifiers);
    button(harness, pos, false, modifiers);
    harness.input_mut().modifiers = Modifiers::NONE;
    harness.run();
}

/// Left double-click at a screen position. egui needs both clicks within
/// 0.3 s, so use an app stepping 1/60 s frames (`app_with_stepping`).
pub fn double_click_at(harness: &mut App, pos: Pos2) {
    send(harness, Event::PointerMoved(pos));
    for _ in 0..2 {
        button(harness, pos, true, Modifiers::NONE);
        button(harness, pos, false, Modifiers::NONE);
    }
    for _ in 0..30 {
        harness.step();
    }
}

/// Left-button drag from `from` to `to` in several small moves.
pub fn drag(harness: &mut App, from: Pos2, to: Pos2) {
    drag_with(harness, PointerButton::Primary, from, to);
}

/// Drag with any mouse button from `from` to `to` in several small moves.
pub fn drag_with(harness: &mut App, button: PointerButton, from: Pos2, to: Pos2) {
    send(harness, Event::PointerMoved(from));
    press(harness, from, button, true, Modifiers::NONE);
    const STEPS: usize = 10;
    for i in 1..=STEPS {
        let t = i as f32 / STEPS as f32;
        send(harness, Event::PointerMoved(from + (to - from) * t));
    }
    press(harness, to, button, false, Modifiers::NONE);
    harness.run();
}

/// Holds or releases a key across frames (unlike `press_key`, which taps it).
pub fn hold_key(harness: &mut App, key: egui::Key, down: bool) {
    send(
        harness,
        Event::Key {
            key,
            physical_key: None,
            pressed: down,
            repeat: false,
            modifiers: Modifiers::NONE,
        },
    );
}

/// Mouse wheel over `pos`, holding `modifiers`, then runs until the app settles.
pub fn wheel(harness: &mut App, pos: Pos2, delta: Vec2, modifiers: Modifiers) {
    wheel_frame(harness, pos, delta, modifiers);
    harness.run();
}

/// Mouse wheel over `pos`, holding `modifiers`; stops right after the frame
/// that handled the wheel event.
pub fn wheel_frame(harness: &mut App, pos: Pos2, delta: Vec2, modifiers: Modifiers) {
    harness.input_mut().modifiers = modifiers;
    send(harness, Event::PointerMoved(pos));
    send(
        harness,
        Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta,
            modifiers,
        },
    );
    harness.input_mut().modifiers = Modifiers::NONE;
}

/// True if the last frame asked egui to draw another one right away.
pub fn wants_repaint(harness: &App) -> bool {
    harness
        .output()
        .viewport_output
        .values()
        .any(|v| v.repaint_delay.is_zero())
}

pub fn assert_close(a: Pos2, b: Pos2) {
    assert!((a - b).length() < 1e-2, "{a:?} != {b:?}");
}

/// Every shape the last frame painted, with `Shape::Vec` flattened.
pub fn frame_shapes(app: &App) -> Vec<egui::Shape> {
    fn flatten(shape: &egui::Shape, out: &mut Vec<egui::Shape>) {
        match shape {
            egui::Shape::Vec(v) => v.iter().for_each(|s| flatten(s, out)),
            s => out.push(s.clone()),
        }
    }
    let mut out = Vec::new();
    for clipped in &app.output().shapes {
        flatten(&clipped.shape, &mut out);
    }
    out
}

/// How many painted text shapes read exactly `text`.
pub fn texts_equal(shapes: &[egui::Shape], text: &str) -> usize {
    shapes
        .iter()
        .filter(|s| matches!(s, egui::Shape::Text(t) if t.galley.job.text == text))
        .count()
}

/// Types `text` like [`type_text`] but steps one frame instead of running to
/// rest: a debounced background search keeps asking for repaints.
pub fn type_text_stepping(harness: &mut App, text: &str) {
    send(harness, Event::Text(text.to_owned()));
    harness.step();
}

/// Types `text` into the focused text field, as one text event.
pub fn type_text(harness: &mut App, text: &str) {
    send(harness, Event::Text(text.to_owned()));
    harness.run();
}
