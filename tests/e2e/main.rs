//! End-to-end tests: drive the real `SpatialCodingApp` headlessly with
//! egui_kittest. Widgets are found through the AccessKit tree; what is
//! painted on the canvas is checked through the app state.
//!
//! Run with `cargo test --test e2e`.

mod common;

mod canvas;
mod cst_panel;
mod edges_panel;
mod fake_lsp;
mod files_panel;
mod folders_panel;
mod file_tree;
mod layout_panel;
mod lsp_panel;
mod manual_fixtures;
mod panel_chrome;
mod performance_panel;
mod settings;
mod shortcuts;
mod sidebar_right;
mod startup;
mod toolbar;
mod welcome;
