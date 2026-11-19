//! Namespace / package view of folders (Folders panel › view): a folder can
//! be named by the namespace its files declare instead of its path.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::nodes::node_store::NodeStore;

/// What names a folder frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum NamespaceView {
    /// The folder name.
    #[default]
    Folders,
    /// The declared namespace or package, else the folder name.
    Namespaces,
    /// `folder (namespace)`.
    Both,
    /// No names.
    None,
}

/// Lines read from the top of a file looking for its declaration.
const SCAN_LINES: usize = 60;

/// The namespace or package a file declares: Go `package x`, Java and
/// Kotlin `package a.b`, C# `namespace A.B`.
pub fn namespace_of(path: &Path, content: &str) -> Option<String> {
    let keyword = match path.extension()?.to_str()? {
        "go" | "java" | "kt" | "kts" => "package",
        "cs" => "namespace",
        _ => return None,
    };
    content.lines().take(SCAN_LINES).find_map(|line| {
        let rest = line.trim().strip_prefix(keyword)?;
        if !rest.starts_with(char::is_whitespace) {
            return None;
        }
        let name: String = rest
            .trim()
            .chars()
            .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '.'))
            .collect();
        (!name.is_empty()).then_some(name)
    })
}

/// The namespace of an open folder: the first one its direct files declare.
pub fn folder_namespace(store: &NodeStore, folder: u64) -> Option<String> {
    let folder = store.get_folder(folder)?;
    folder
        .children_ids
        .iter()
        .filter_map(|id| store.get_file(*id))
        .find_map(|file| namespace_of(&file.path, &file.content))
}

/// The text of a folder's name for `view`; `None` means no name is drawn.
pub fn folder_text(view: NamespaceView, folder: &str, namespace: Option<&str>) -> Option<String> {
    match (view, namespace) {
        (NamespaceView::None, _) => None,
        (NamespaceView::Folders, _) | (NamespaceView::Namespaces, None) | (NamespaceView::Both, None) => Some(folder.to_string()),
        (NamespaceView::Namespaces, Some(ns)) => Some(ns.to_string()),
        (NamespaceView::Both, Some(ns)) => Some(format!("{folder} ({ns})")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn go_java_kotlin_and_csharp_declare_namespaces() {
        let ns = |p: &str, c: &str| namespace_of(Path::new(p), c);
        assert_eq!(ns("a.go", "// c\npackage shop\n\nimport \"x\"\n").as_deref(), Some("shop"));
        assert_eq!(ns("A.java", "package com.acme.shop;\nclass A {}").as_deref(), Some("com.acme.shop"));
        assert_eq!(ns("A.kt", "package com.acme\n").as_deref(), Some("com.acme"));
        assert_eq!(ns("A.cs", "using System;\nnamespace Acme.Shop\n{\n}").as_deref(), Some("Acme.Shop"));
        assert_eq!(ns("A.cs", "namespace Acme.Shop;").as_deref(), Some("Acme.Shop"));
        assert_eq!(ns("a.rs", "package x"), None, "Rust has no such declaration");
        assert_eq!(ns("a.go", "packages x"), None, "a keyword, not a prefix");
    }

    #[test]
    fn the_view_decides_the_folder_text() {
        use NamespaceView::*;
        assert_eq!(folder_text(Folders, "shop", Some("com.acme")).as_deref(), Some("shop"));
        assert_eq!(folder_text(Namespaces, "shop", Some("com.acme")).as_deref(), Some("com.acme"));
        assert_eq!(folder_text(Namespaces, "shop", Option::None).as_deref(), Some("shop"), "no declaration: the folder");
        assert_eq!(folder_text(Both, "shop", Some("com.acme")).as_deref(), Some("shop (com.acme)"));
        assert_eq!(folder_text(None, "shop", Some("x")), Option::None);
    }

    #[test]
    fn a_folder_takes_the_namespace_of_its_files() {
        let mut store = NodeStore::default();
        let a = store.add_file("/p/shop/a.go".into(), "package shop\n".into(), egui::Pos2::ZERO);
        let folder = store.add_folder("/p/shop".into(), 1);
        store.get_folder_mut(folder).unwrap().children_ids.push(a);
        assert_eq!(folder_namespace(&store, folder).as_deref(), Some("shop"));
    }
}
