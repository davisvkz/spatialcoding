//! SpatialCoding: spatial code visualization on an infinite canvas.
//!
//! The binary in `main.rs` only parses arguments and starts eframe; everything
//! else lives here so integration tests can drive the real app.

pub mod agent_notes;
pub mod analysis;
pub mod app;
pub mod canvas;
pub mod cli;
pub mod commands;
pub mod edges;
pub mod folder_actions;
pub mod fs;
pub mod git;
pub mod highlight;
pub mod input;
pub mod layout;
pub mod namespaces;
pub mod nodes;
pub mod search;
pub mod server;
pub mod settings_store;
pub mod state;
pub mod undo;
pub mod ui;
pub mod workspace;
