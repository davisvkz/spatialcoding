//! SpatialCoding: spatial code visualization on an infinite canvas.
//!
//! The binary in `main.rs` only parses arguments and starts eframe; everything
//! else lives here so integration tests can drive the real app.

pub mod analysis;
pub mod app;
pub mod canvas;
pub mod edges;
pub mod fs;
pub mod git;
pub mod highlight;
pub mod input;
pub mod layout;
pub mod nodes;
pub mod server;
pub mod state;
pub mod ui;
pub mod workspace;
