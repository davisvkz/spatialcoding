//! Rust module declarations that resolve using only the supplied file snapshot.

use std::path::{Path, PathBuf};

use tree_sitter::Node;

use super::{
    candidates_in, normalize, push, LocalAnalysis, OpenFiles, RelationCandidates, RelationshipType,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModuleIssueReason {
    Conditional,
    UnsupportedAttribute,
    OutsideWorkspace,
    Missing,
    Ambiguous,
}

#[derive(Debug)]
pub(crate) struct ModuleIssue {
    pub path: PathBuf,
    pub line: usize,
    pub name: String,
    pub reason: ModuleIssueReason,
}

struct ModuleDirectory {
    // In a non-root file foo.rs, `mod bar;` uses foo/bar.rs, but a
    // file-level #[path] is relative to foo.rs's containing directory.
    default: PathBuf,
    explicit: PathBuf,
}

impl ModuleDirectory {
    fn of_file(path: &Path) -> Option<Self> {
        let parent = path.parent()?;
        let stem = path.file_stem()?;
        let default = if matches!(stem.to_str(), Some("lib" | "main" | "mod")) {
            parent.to_path_buf()
        } else {
            parent.join(stem)
        };
        Some(Self {
            default,
            explicit: parent.to_path_buf(),
        })
    }

    fn inline(
        &self,
        root: &Path,
        name: &str,
        path: Option<&str>,
    ) -> Result<Self, ModuleIssueReason> {
        let dir = match path {
            Some(path) => self.explicit_path(root, path)?,
            None => self.default.join(name),
        };
        Ok(Self {
            default: dir.clone(),
            explicit: dir,
        })
    }

    fn explicit_path(&self, root: &Path, value: &str) -> Result<PathBuf, ModuleIssueReason> {
        let value = Path::new(value);
        if !value.is_relative() {
            return Err(ModuleIssueReason::OutsideWorkspace);
        }
        normalize(&self.explicit.join(value))
            .filter(|path| path.starts_with(root))
            .ok_or(ModuleIssueReason::OutsideWorkspace)
    }
}

type ModuleContext = Result<ModuleDirectory, ModuleIssueReason>;

pub(super) fn analyze(
    root: &Path,
    open: &OpenFiles<'_>,
    (path, text): (&Path, &str),
    source: Node<'_>,
    out: &mut LocalAnalysis,
) {
    let Some(dir) = ModuleDirectory::of_file(path) else {
        return;
    };
    let mut scopes = vec![(source, Ok(dir))];
    while let Some((scope, directory)) = scopes.pop() {
        let mut cursor = scope.walk();
        let directory = if scope
            .named_children(&mut cursor)
            .any(|node| node.kind() == "inner_attribute_item" && conditional_attribute(node, text))
        {
            Err(ModuleIssueReason::Conditional)
        } else {
            directory
        };
        for item in scope
            .named_children(&mut cursor)
            .filter(|node| node.kind() == "mod_item")
        {
            let Some(name) = item
                .child_by_field_name("name")
                .filter(|name| !name.is_missing())
            else {
                continue;
            };
            let raw_name = node_text(name, text);
            let module = raw_name.strip_prefix("r#").unwrap_or(raw_name);
            let context = module_context(&directory, item, text);
            if let Some(body) = item.child_by_field_name("body") {
                scopes.push((
                    body,
                    context.and_then(|(dir, value)| dir.inline(root, module, value)),
                ));
            } else if !item.has_error() {
                match context.and_then(|(dir, value)| module_target(root, open, dir, module, value))
                {
                    Ok(target) => push(
                        path,
                        text,
                        &target,
                        name.start_byte(),
                        name.end_byte(),
                        RelationshipType::Imports,
                        &mut out.links,
                    ),
                    Err(reason) => out.rust_issues.push(ModuleIssue {
                        path: path.strip_prefix(root).unwrap_or(path).to_path_buf(),
                        line: name.start_position().row + 1,
                        name: raw_name.to_owned(),
                        reason,
                    }),
                }
            }
        }
    }
}

fn module_target(
    root: &Path,
    open: &OpenFiles<'_>,
    dir: &ModuleDirectory,
    name: &str,
    explicit: Option<&str>,
) -> Result<PathBuf, ModuleIssueReason> {
    let candidates = match explicit {
        Some(value) => vec![dir.explicit_path(root, value)?],
        None => vec![
            dir.default.join(format!("{name}.rs")),
            dir.default.join(name).join("mod.rs"),
        ],
    };
    match candidates_in(root, open, candidates) {
        RelationCandidates::One(target) => Ok(target),
        RelationCandidates::None => Err(ModuleIssueReason::Missing),
        RelationCandidates::Many => Err(ModuleIssueReason::Ambiguous),
    }
}

fn module_context<'d, 't>(
    directory: &'d ModuleContext,
    item: Node<'_>,
    text: &'t str,
) -> Result<(&'d ModuleDirectory, Option<&'t str>), ModuleIssueReason> {
    let dir = directory.as_ref().map_err(|&reason| reason)?;
    let mut explicit = None;
    let mut sibling = item.prev_named_sibling();
    while let Some(node) = sibling {
        sibling = node.prev_named_sibling();
        match node.kind() {
            "line_comment" | "block_comment" => continue,
            "attribute_item" => {}
            _ => break,
        }
        let Some(attr) = attribute_node(node).filter(|node| !node.has_error()) else {
            return Err(ModuleIssueReason::UnsupportedAttribute);
        };
        match attribute_name(attr, text) {
            "cfg" | "cfg_attr" => return Err(ModuleIssueReason::Conditional),
            "path" if explicit.is_none() => {
                explicit =
                    Some(path_value(attr, text).ok_or(ModuleIssueReason::UnsupportedAttribute)?);
            }
            "doc" | "macro_use" | "allow" | "warn" | "deny" | "forbid" | "deprecated" => {}
            _ => return Err(ModuleIssueReason::UnsupportedAttribute),
        }
    }
    Ok((dir, explicit))
}

fn attribute_name<'a>(attribute: Node<'_>, text: &'a str) -> &'a str {
    let mut cursor = attribute.walk();
    attribute
        .named_children(&mut cursor)
        .find(|node| !matches!(node.kind(), "line_comment" | "block_comment"))
        .map_or("", |name| node_text(name, text))
}

fn conditional_attribute(node: Node<'_>, text: &str) -> bool {
    attribute_node(node).is_some_and(|attr| match attribute_name(attr, text) {
        "cfg" => true,
        "cfg_attr" => !has_only_harmless_inner_attributes(attr, text),
        _ => false,
    })
}

fn attribute_node(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|n| n.kind() == "attribute")
}

fn has_only_harmless_inner_attributes(attribute: Node<'_>, text: &str) -> bool {
    let Some(args) = attribute.child_by_field_name("arguments") else {
        return false;
    };
    let mut cursor = args.walk();
    // Nested groups are single nodes: only a top-level comma ends the predicate.
    let mut tokens = args
        .children(&mut cursor)
        .filter(|n| !matches!(n.kind(), "line_comment" | "block_comment"));
    if tokens.next().map(|n| n.kind()) != Some("(") || !tokens.any(|n| n.kind() == ",") {
        return false;
    }
    // Crates commonly use this to select no_std. Neither branch changes
    // module presence or paths; do not interpret the predicate itself.
    match tokens.next().map(|n| node_text(n, text)) {
        Some("no_std") => match tokens.next().map(|n| n.kind()) {
            Some(")") => true,
            Some(",") => tokens.next().is_some_and(|n| n.kind() == ")"),
            _ => false,
        },
        _ => false,
    }
}

fn path_value<'a>(attribute: Node<'_>, text: &'a str) -> Option<&'a str> {
    let value = attribute.child_by_field_name("value")?;
    let raw = node_text(value, text);
    let inner = match value.kind() {
        "string_literal" => raw.strip_prefix('"')?.strip_suffix('"')?,
        "raw_string_literal" => {
            let (prefix, quoted) = raw.strip_prefix('r')?.split_once('"')?;
            quoted.strip_suffix(prefix)?.strip_suffix('"')?
        }
        _ => return None,
    };
    (!inner.is_empty() && !inner.contains(['\\', ':', '\0', '\r', '\n'])).then_some(inner)
}

fn node_text<'a>(node: Node<'_>, text: &'a str) -> &'a str {
    node.utf8_text(text.as_bytes()).unwrap_or_default()
}
