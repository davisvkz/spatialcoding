use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Dependency {
    pub source: PathBuf,
    pub target: PathBuf,
    pub kind: DependencyKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyKind {
    Import,
    Export,
    ReExport,
}

pub fn analyze_dependencies(root: &Path, files: &[PathBuf]) -> Vec<Dependency> {
    let file_set: HashSet<&PathBuf> = files.iter().collect();
    let mut deps = Vec::new();

    for file in files {
        let content = match std::fs::read_to_string(file) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext == "rs" {
            for target in parse_rust_imports(root, file, &content) {
                if file_set.contains(&target) && target != *file {
                    deps.push(Dependency {
                        source: file.clone(),
                        target,
                        kind: DependencyKind::Import,
                    });
                }
            }
        }
    }

    deps
}

fn parse_rust_imports(root: &Path, source_file: &Path, content: &str) -> Vec<PathBuf> {
    let mut imports = Vec::new();
    let src_dir = root.join("src");

    for line in content.lines() {
        let trimmed = line.trim();

        if let Some(rest) = trimmed.strip_prefix("use crate::") {
            let path_part = rest.split('{').next().unwrap_or("");
            let path_part = path_part.split("::*").next().unwrap_or(path_part);
            let path_part = path_part.trim_end_matches(';').trim_end_matches("::");
            if path_part.is_empty() {
                continue;
            }
            let segments: Vec<&str> = path_part.split("::").collect();
            resolve_module_path(&src_dir, &segments, &mut imports);
        }

        if trimmed.starts_with("mod ") && trimmed.ends_with(';') {
            let mod_name = trimmed
                .strip_prefix("mod ")
                .unwrap_or("")
                .trim_end_matches(';')
                .trim();
            if !mod_name.is_empty() && !mod_name.contains(' ') {
                if let Some(parent) = source_file.parent() {
                    let as_file = parent.join(format!("{mod_name}.rs"));
                    let as_mod = parent.join(mod_name).join("mod.rs");
                    if as_file.exists() {
                        imports.push(as_file);
                    } else if as_mod.exists() {
                        imports.push(as_mod);
                    }
                }
            }
        }
    }

    imports
}

fn resolve_module_path(src_dir: &Path, segments: &[&str], imports: &mut Vec<PathBuf>) {
    let mut dir = src_dir.to_path_buf();
    for (i, seg) in segments.iter().enumerate() {
        if i == segments.len() - 1 {
            let as_file = dir.join(format!("{seg}.rs"));
            let as_mod = dir.join(seg).join("mod.rs");
            if as_file.exists() {
                imports.push(as_file);
            } else if as_mod.exists() {
                imports.push(as_mod);
            }
        } else {
            dir = dir.join(seg);
        }
    }
}
