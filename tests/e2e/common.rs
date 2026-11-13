//! Shared fixture and input helpers for the e2e tests.

use std::path::{Path, PathBuf};

use egui::{Event, Modifiers, MouseWheelUnit, PointerButton, Pos2, Vec2};
use egui_kittest::Harness;
use spatialcoding::app::SpatialCodingApp;
use spatialcoding::state::AppState;

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

/// The real app in a 1400x900 window, with nothing loaded.
pub fn app() -> App {
    let mut harness = Harness::builder()
        .with_size(Vec2::new(1400.0, 900.0))
        .build_eframe(|cc| SpatialCodingApp::new(cc));
    harness.run();
    harness
}

/// The app with `fx` loaded as the workspace.
pub fn app_with(fx: &Fixture) -> App {
    let mut harness = app();
    harness.state_mut().open_folder(fx.root.clone());
    harness.run();
    harness
}

pub trait AppExt {
    fn app_state(&self) -> &AppState;
    fn app_state_mut(&mut self) -> &mut AppState;
    /// Opens a file of the workspace the same way a click in the tree does.
    fn open(&mut self, path: &Path) -> u64;
    /// World-space center of a file node.
    fn node_center(&self, id: u64) -> Pos2;
    /// Pans the view so `world` shows at screen position `screen`.
    fn show_at(&mut self, world: Pos2, screen: Pos2);
    fn node_position(&self, id: u64) -> Pos2;
    fn selected(&self) -> Vec<u64>;
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
        self.run();
        id
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
    send(
        harness,
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
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

/// Left-button drag from `from` to `to` in several small moves.
pub fn drag(harness: &mut App, from: Pos2, to: Pos2) {
    send(harness, Event::PointerMoved(from));
    button(harness, from, true, Modifiers::NONE);
    const STEPS: usize = 10;
    for i in 1..=STEPS {
        let t = i as f32 / STEPS as f32;
        send(harness, Event::PointerMoved(from + (to - from) * t));
    }
    button(harness, to, false, Modifiers::NONE);
    harness.run();
}

/// Mouse wheel over `pos`, holding `modifiers`.
pub fn wheel(harness: &mut App, pos: Pos2, delta: Vec2, modifiers: Modifiers) {
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
    harness.run();
}

pub fn assert_close(a: Pos2, b: Pos2) {
    assert!((a - b).length() < 1e-2, "{a:?} != {b:?}");
}
