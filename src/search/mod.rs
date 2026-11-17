//! Searching the loaded folder: fuzzy matching, file names, file contents and
//! top-level symbols. Pure functions over paths and text, shared by the
//! command palette and the Search panel.

pub mod content;
pub mod files;
pub mod fuzzy;
pub mod symbols;
