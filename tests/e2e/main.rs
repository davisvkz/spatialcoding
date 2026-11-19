//! End-to-end tests: drive the real `SpatialCodingApp` headlessly with
//! egui_kittest. Widgets are found through the AccessKit tree; what is
//! painted on the canvas is checked through the app state.
//!
//! Run with `cargo test --test e2e`.

mod common;

mod canvas;
mod command_palette;
mod agent_notes;
mod cst_panel;
mod edges_panel;
mod fake_lsp;
mod files_panel;
mod folders_panel;
mod git_changes;
mod ignore;
mod file_tree;
mod layout_panel;
mod lsp_panel;
mod manual_fixtures;
mod mcp;
mod panel_chrome;
mod performance_panel;
mod pins;
mod search_panel;
mod settings;
mod shell;
mod shortcuts;
mod showcase;
mod sidebar_right;
mod startup;
mod toolbar;
mod watcher;
mod welcome;
