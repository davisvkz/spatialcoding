//! End-to-end tests: drive the real `SpatialCodingApp` headlessly with
//! egui_kittest. Widgets are found through the AccessKit tree; what is
//! painted on the canvas is checked through the app state.
//!
//! Run with `cargo test --test e2e`.

mod common;

mod canvas;
mod cst_panel;
mod file_tree;
mod manual_fixtures;
mod settings;
mod shortcuts;
mod sidebar_right;
mod startup;
mod toolbar;
mod welcome;
