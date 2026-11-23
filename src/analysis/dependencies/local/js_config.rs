//! The project configuration JS and TS imports are resolved with, read from
//! the files themselves and never executed:
//!
//! - `tsconfig.json` and `jsconfig.json`: `compilerOptions.baseUrl` and
//!   `paths`, following `extends` to other local files (`./` and `../` only;
//!   a package name is not followed);
//! - `package.json` of the packages of the folder: `exports` and `main`, so
//!   an import of a workspace package reaches its source;
//! - `vite.config.*`: `resolve.alias`, read from the syntax tree for the
//!   forms people write (`path.resolve(__dirname, 'src')`,
//!   `fileURLToPath(new URL('./src', import.meta.url))`, plain strings).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use serde_json::Value;
use tree_sitter::Node;

use super::{normalize, ConfigSources, MAX_CONFIG_BYTES};
use crate::analysis::cst::parse_tree;

/// Names a compiler configuration may have, nearest `tsconfig.json` first.
pub(super) const TS_CONFIG_NAMES: [&str; 2] = ["tsconfig.json", "jsconfig.json"];
pub(super) const VITE_CONFIG_NAMES: [&str; 4] =
    ["vite.config.ts", "vite.config.js", "vite.config.mjs", "vite.config.cjs"];
/// How many `extends` hops are followed.
pub(super) const MAX_EXTENDS_DEPTH: usize = 5;
/// How many workspace `package.json` files are read.
pub(super) const MAX_PACKAGES: usize = 64;

/// Whether a file of this name changes how imports resolve.
pub(crate) fn is_config_name(name: &str) -> bool {
    TS_CONFIG_NAMES.contains(&name)
        || (name.starts_with("tsconfig.") || name.starts_with("jsconfig.")) && name.ends_with(".json")
        || VITE_CONFIG_NAMES.contains(&name)
        || matches!(name, "go.mod" | "package.json")
}

/// A config file that other configs can extend: `tsconfig.base.json` and
/// friends, not only the nearest `tsconfig.json`.
pub(super) fn is_ts_config_name(name: &str) -> bool {
    (name.starts_with("tsconfig") || name.starts_with("jsconfig")) && name.ends_with(".json")
}

// ---- tsconfig / jsconfig ----

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum Extends {
    One(String),
    Many(Vec<String>),
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompilerOptions {
    base_url: Option<PathBuf>,
    paths: Option<BTreeMap<String, Vec<String>>>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawConfig {
    extends: Option<Extends>,
    compiler_options: Option<CompilerOptions>,
}

/// The JSON of a tsconfig: comments and trailing commas are removed first.
fn parse_raw(text: &str) -> Option<RawConfig> {
    if text.len() > MAX_CONFIG_BYTES {
        return None;
    }
    // Preserve strings (including URL-like text) while removing JSONC
    // comments and trailing commas. The result is still validated as JSON.
    static COMMENTS: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"("(?:[^"\\]|\\.)*")|(?s:/\*.*?\*/)|//[^\r\n]*"#).unwrap()
    });
    static TRAILING: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r#"("(?:[^"\\]|\\.)*")|,\s*([}\]])"#).unwrap());
    let clean = COMMENTS.replace_all(text.trim_start_matches('\u{feff}'), "$1 ");
    let clean = TRAILING.replace_all(&clean, "${1}${2}");
    serde_json::from_str(&clean).ok()
}

/// The file an `extends` value names: only `./` and `../` values are local
/// files (anything else is a package), `.json` is added when missing, and the
/// file must be inside `root`.
fn extends_target(root: &Path, dir: &Path, value: &str) -> Option<PathBuf> {
    if !(value.starts_with("./") || value.starts_with("../")) {
        return None;
    }
    let value = if value.ends_with(".json") { value.to_owned() } else { format!("{value}.json") };
    normalize(&dir.join(value)).filter(|path| path.starts_with(root))
}

fn extends_values(extends: Option<Extends>) -> Vec<String> {
    match extends {
        Some(Extends::One(value)) => vec![value],
        Some(Extends::Many(values)) => values,
        None => Vec::new(),
    }
}

/// The local configs `text` (of the config at `path`) extends, resolved.
pub(super) fn extended_paths(root: &Path, path: &Path, text: &str) -> Vec<PathBuf> {
    let Some(raw) = parse_raw(text) else { return Vec::new() };
    let Some(dir) = path.parent() else { return Vec::new() };
    extends_values(raw.extends).iter().filter_map(|value| extends_target(root, dir, value)).collect()
}

/// What the nearest compiler configuration says about aliases, with its
/// `extends` chain merged in.
pub(super) struct TsPaths {
    base: PathBuf,
    paths: BTreeMap<String, Vec<String>>,
}

#[derive(Default)]
struct Merged {
    /// `baseUrl`, resolved from the config that declares it.
    base_url: Option<PathBuf>,
    /// `paths` and the folder of the config that declares them (their base
    /// when there is no `baseUrl`).
    paths: Option<(PathBuf, BTreeMap<String, Vec<String>>)>,
    /// An `extends` that could not be followed (a package, a missing file).
    partial: bool,
}

impl TsPaths {
    /// The aliases of the config at `path`. `None` when it cannot be read
    /// (not JSON, an absolute `baseUrl`) or when it extends something that
    /// could not be followed and has no `paths` of its own to go on.
    pub(super) fn parse(root: &Path, path: &Path, configs: &ConfigSources) -> Option<Self> {
        let merged = merge(root, path, configs, &mut Vec::new())?;
        if merged.partial && merged.paths.is_none() {
            return None;
        }
        let dir = path.parent()?;
        let (origin, paths) = merged.paths.unwrap_or_else(|| (dir.to_path_buf(), BTreeMap::new()));
        Some(Self { base: merged.base_url.unwrap_or(origin), paths })
    }

    /// Where `value` is mapped to, in the order the config lists them.
    pub(super) fn target(&self, value: &str) -> TsTarget {
        let (targets, capture) = if let Some(targets) = self.paths.get(value) {
            (targets, "")
        } else {
            let Some((_, targets, capture)) = self
                .paths
                .iter()
                .filter_map(|(key, targets)| {
                    let (prefix, suffix) = key.split_once('*')?;
                    if suffix.contains('*') {
                        return None;
                    }
                    let capture = value.strip_prefix(prefix)?.strip_suffix(suffix)?;
                    Some((prefix.len(), targets, capture))
                })
                .max_by_key(|(prefix, _, _)| *prefix)
            else {
                return TsTarget::NoMatch;
            };
            (targets, capture)
        };
        let bases: Vec<PathBuf> = targets
            .iter()
            .map(|target| target.replacen('*', capture, 1))
            .filter(|target| {
                let target = Path::new(target);
                target.is_relative() && !target.to_string_lossy().contains(['*', ':', '\\', '%'])
            })
            .map(|target| self.base.join(target))
            .collect();
        if bases.is_empty() { TsTarget::Unsupported } else { TsTarget::Paths(bases) }
    }
}

/// What the `paths` of a config say about an import.
pub(super) enum TsTarget {
    /// No key matches: a package, or a path the config does not know.
    NoMatch,
    /// The target is absolute or has a form that is not followed.
    Unsupported,
    /// The places to look, in order: the first that exists is the target.
    Paths(Vec<PathBuf>),
}

fn merge(root: &Path, path: &Path, configs: &ConfigSources, chain: &mut Vec<PathBuf>) -> Option<Merged> {
    if chain.len() >= MAX_EXTENDS_DEPTH || chain.iter().any(|seen| seen == path) {
        return Some(Merged { partial: true, ..Merged::default() });
    }
    let raw = parse_raw(configs.get(path)?)?;
    let dir = path.parent()?;
    chain.push(path.to_path_buf());
    let mut merged = Merged::default();
    for value in extends_values(raw.extends) {
        let base = extends_target(root, dir, &value);
        match base.and_then(|base| merge(root, &base, configs, chain)) {
            // Later `extends` override earlier ones.
            Some(inherited) => {
                merged.partial |= inherited.partial;
                if inherited.base_url.is_some() {
                    merged.base_url = inherited.base_url;
                }
                if inherited.paths.is_some() {
                    merged.paths = inherited.paths;
                }
            }
            None => merged.partial = true,
        }
    }
    chain.pop();
    if let Some(options) = raw.compiler_options {
        if let Some(base_url) = options.base_url {
            if !base_url.is_relative() {
                return None;
            }
            merged.base_url = normalize(&dir.join(base_url));
        }
        if let Some(paths) = options.paths {
            merged.paths = Some((dir.to_path_buf(), paths));
        }
    }
    Some(merged)
}

// ---- workspace packages ----

/// A package of the folder: its `package.json`, enough to find its source.
pub(super) struct Package {
    name: String,
    dir: PathBuf,
    exports: Option<Value>,
    main: Option<String>,
}

/// The workspace packages in `configs`, longest name first, so
/// `@acme/ui-kit` is tried before `@acme/ui`.
pub(super) fn packages(configs: &ConfigSources) -> Vec<Package> {
    let mut packages: Vec<Package> = configs
        .iter()
        .filter(|(path, _)| path.file_name().is_some_and(|name| name == "package.json"))
        .filter_map(|(path, text)| {
            let json: Value = serde_json::from_str(text).ok()?;
            Some(Package {
                name: json.get("name")?.as_str()?.to_owned(),
                dir: path.parent()?.to_path_buf(),
                exports: json.get("exports").cloned(),
                main: json.get("main").and_then(Value::as_str).map(str::to_owned),
            })
        })
        .collect();
    packages.sort_by(|a, b| b.name.len().cmp(&a.name.len()).then_with(|| a.name.cmp(&b.name)));
    packages
}

/// Where the workspace package `value` names starts: `None` when no package
/// of the folder has that name, `Some(Err(()))` when it does but exports
/// nothing for the subpath.
pub(super) fn package_base(packages: &[Package], value: &str) -> Option<Result<PathBuf, ()>> {
    let package = packages.iter().find(|p| value == p.name || value.strip_prefix(&p.name).is_some_and(|rest| rest.starts_with('/')))?;
    let subpath = match value.strip_prefix(&package.name) {
        Some(rest) if !rest.is_empty() => format!(".{rest}"),
        _ => ".".to_owned(),
    };
    let relative = match &package.exports {
        Some(exports) => export_target(exports, &subpath),
        None if subpath == "." => package.main.clone().or_else(|| Some("./".to_owned())),
        None => Some(subpath.clone()),
    };
    Some(match relative {
        Some(relative) => {
            let relative = relative.trim_start_matches("./");
            Ok(if relative.is_empty() { package.dir.clone() } else { package.dir.join(relative) })
        }
        None => Err(()),
    })
}

/// The `exports` target of `subpath` (`.` or `./x`): a string, an array, a
/// map of subpaths (with `*` patterns) or of conditions.
fn export_target(exports: &Value, subpath: &str) -> Option<String> {
    match exports {
        Value::Object(map) if map.keys().any(|key| key.starts_with('.')) => {
            if let Some(value) = map.get(subpath) {
                return condition_target(value);
            }
            let (_, value, capture) = map
                .iter()
                .filter_map(|(key, value)| {
                    let (prefix, suffix) = key.split_once('*')?;
                    let capture = subpath.strip_prefix(prefix)?.strip_suffix(suffix)?;
                    Some((prefix.len(), value, capture))
                })
                .max_by_key(|(prefix, _, _)| *prefix)?;
            condition_target(value).map(|target| target.replacen('*', capture, 1))
        }
        other => (subpath == ".").then(|| condition_target(other)).flatten(),
    }
}

/// The file a value of `exports` names, trying the conditions a bundler
/// resolving source would: `import`, `default`, `require`, `types`.
fn condition_target(value: &Value) -> Option<String> {
    match value {
        Value::String(target) => Some(target.clone()),
        Value::Array(items) => items.iter().find_map(condition_target),
        Value::Object(map) => ["import", "default", "require", "types", "node"]
            .iter()
            .find_map(|condition| map.get(*condition).and_then(condition_target)),
        _ => None,
    }
}

// ---- vite.config ----

/// `(alias, folder or file)` pairs of `resolve.alias`, longest alias first.
pub(super) fn vite_aliases(path: &Path, text: &str) -> Vec<(String, PathBuf)> {
    let Some(dir) = path.parent() else { return Vec::new() };
    let Some((_, tree)) = parse_tree(path, text) else { return Vec::new() };
    let mut found = Vec::new();
    let mut pending = vec![tree.root_node()];
    while let Some(node) = pending.pop() {
        if node.kind() == "pair"
            && node.child_by_field_name("key").is_some_and(|key| key_text(key, text) == "alias")
            && let Some(value) = node.child_by_field_name("value")
        {
            collect_aliases(value, text, dir, &mut found);
        }
        let mut cursor = node.walk();
        pending.extend(node.named_children(&mut cursor));
    }
    found.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
    found.dedup_by(|a, b| a.0 == b.0);
    found
}

fn key_text(node: Node, text: &str) -> String {
    let raw = node.utf8_text(text.as_bytes()).unwrap_or("");
    if node.kind() == "string" { string_value(raw).unwrap_or_default().to_owned() } else { raw.to_owned() }
}

fn string_value(raw: &str) -> Option<&str> {
    let quote = *raw.as_bytes().first()?;
    if !matches!(quote, b'\'' | b'"' | b'`') || raw.len() < 2 || raw.as_bytes().last()? != &quote {
        return None;
    }
    let value = &raw[1..raw.len() - 1];
    (!value.contains(['\\', '\n', '$'])).then_some(value)
}

fn collect_aliases(value: Node, text: &str, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let mut cursor = value.walk();
    match value.kind() {
        // { '@': path.resolve(__dirname, 'src') }
        "object" => {
            for pair in value.named_children(&mut cursor).filter(|n| n.kind() == "pair") {
                let (Some(key), Some(target)) = (pair.child_by_field_name("key"), pair.child_by_field_name("value")) else {
                    continue;
                };
                if let Some(target) = alias_target(target, text, dir) {
                    out.push((key_text(key, text), target));
                }
            }
        }
        // [{ find: '@', replacement: ... }]; a regex `find` is not followed.
        "array" => {
            for item in value.named_children(&mut cursor).filter(|n| n.kind() == "object") {
                let (mut find, mut replacement) = (None, None);
                let mut inner = item.walk();
                for pair in item.named_children(&mut inner).filter(|n| n.kind() == "pair") {
                    let (Some(key), Some(value)) = (pair.child_by_field_name("key"), pair.child_by_field_name("value")) else {
                        continue;
                    };
                    match key_text(key, text).as_str() {
                        "find" if value.kind() == "string" => {
                            find = string_value(value.utf8_text(text.as_bytes()).unwrap_or("")).map(str::to_owned)
                        }
                        "replacement" => replacement = alias_target(value, text, dir),
                        _ => {}
                    }
                }
                if let (Some(find), Some(replacement)) = (find, replacement) {
                    out.push((find, replacement));
                }
            }
        }
        _ => {}
    }
}

/// A path written the way Vite configs do, relative to the config's folder.
fn alias_target(node: Node, text: &str, dir: &Path) -> Option<PathBuf> {
    let raw = |n: Node| n.utf8_text(text.as_bytes()).unwrap_or("").to_owned();
    match node.kind() {
        "string" => {
            let text = raw(node);
            let value = string_value(&text)?;
            // `/src` is the project root's `src` for Vite, not the disk's.
            normalize(&dir.join(value.trim_start_matches('/')))
        }
        "call_expression" => {
            let function = raw(node.child_by_field_name("function")?);
            let args = node.child_by_field_name("arguments")?;
            let mut cursor = args.walk();
            let args: Vec<Node> = args.named_children(&mut cursor).collect();
            if function.ends_with("fileURLToPath") {
                // fileURLToPath(new URL('./src', import.meta.url))
                let url = args.first().filter(|n| n.kind() == "new_expression")?;
                let first = url.child_by_field_name("arguments")?.named_child(0)?;
                return normalize(&dir.join(string_value(&raw(first))?));
            }
            if !(function.ends_with("resolve") || function.ends_with("join")) {
                return None;
            }
            // path.resolve(__dirname, 'src') or path.join(__dirname, 'a', 'b')
            let mut path = dir.to_path_buf();
            for arg in args {
                match arg.kind() {
                    "identifier" if raw(arg) == "__dirname" => path = dir.to_path_buf(),
                    "string" => {
                        let text = raw(arg);
                        let part = string_value(&text)?;
                        if part.starts_with('/') {
                            return None;
                        }
                        path = path.join(part);
                    }
                    _ => return None,
                }
            }
            normalize(&path)
        }
        _ => None,
    }
}

/// `value` against the aliases: the target and the rest of the import.
pub(super) fn vite_base(aliases: &[(String, PathBuf)], value: &str) -> Option<PathBuf> {
    aliases.iter().find_map(|(alias, target)| {
        if value == alias {
            Some(target.clone())
        } else {
            let rest = value.strip_prefix(alias.as_str())?.strip_prefix('/')?;
            Some(target.join(rest))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configs(files: &[(&str, &str)]) -> ConfigSources {
        files.iter().map(|(p, t)| (PathBuf::from(p), (*t).to_owned())).collect()
    }

    fn paths_of(files: &[(&str, &str)], config: &str) -> Option<TsPaths> {
        TsPaths::parse(Path::new("/p"), Path::new(config), &configs(files))
    }

    fn bases(target: TsTarget) -> Vec<String> {
        match target {
            TsTarget::Paths(bases) => bases.iter().map(|p| p.to_str().unwrap().to_owned()).collect(),
            TsTarget::NoMatch => vec!["no match".into()],
            TsTarget::Unsupported => vec!["unsupported".into()],
        }
    }

    #[test]
    fn extends_merges_a_local_base_and_the_child_overrides_per_key() {
        let files = [
            ("/p/tsconfig.base.json", r#"{"compilerOptions": {"baseUrl": ".", "paths": {"@/*": ["src/*"], "lib/*": ["lib/*"]}}}"#),
            ("/p/apps/web/tsconfig.json", r#"{"extends": "../../tsconfig.base"}"#),
            ("/p/apps/api/tsconfig.json", r#"{"extends": ["../../tsconfig.base.json"], "compilerOptions": {"paths": {"@/*": ["app/*"]}}}"#),
        ];
        // An extending config with nothing of its own inherits baseUrl and paths.
        let web = paths_of(&files, "/p/apps/web/tsconfig.json").unwrap();
        assert_eq!(bases(web.target("@/a")), ["/p/src/a"], "baseUrl is relative to the config that declares it");
        // `paths` is replaced as a whole, not merged, but baseUrl is inherited.
        let api = paths_of(&files, "/p/apps/api/tsconfig.json").unwrap();
        assert_eq!(bases(api.target("@/a")), ["/p/app/a"]);
        assert_eq!(bases(api.target("lib/x")), ["no match"]);
    }

    #[test]
    fn paths_without_a_base_url_are_relative_to_the_config_that_declares_them() {
        let files = [
            ("/p/base.json", r#"{"compilerOptions": {"paths": {"~/*": ["src/*"]}}}"#),
            ("/p/apps/web/tsconfig.json", r#"{"extends": "../../base.json"}"#),
        ];
        let web = paths_of(&files, "/p/apps/web/tsconfig.json").unwrap();
        assert_eq!(bases(web.target("~/a")), ["/p/src/a"], "not /p/apps/web/src/a");
    }

    #[test]
    fn extends_that_cannot_be_followed_leaves_the_config_unreadable_unless_it_has_paths() {
        // A package, a missing file, an escape from the root and a cycle.
        for extends in ["@tsconfig/node20/tsconfig.json", "./missing.json", "../../../outside.json"] {
            let files = [("/p/tsconfig.json", format!(r#"{{"extends": "{extends}"}}"#))];
            let files: Vec<(&str, &str)> = files.iter().map(|(p, t)| (*p, t.as_str())).collect();
            assert!(paths_of(&files, "/p/tsconfig.json").is_none(), "{extends}");
        }
        let cycle = [("/p/a.json", r#"{"extends": "./b.json"}"#), ("/p/b.json", r#"{"extends": "./a.json"}"#)];
        assert!(paths_of(&cycle, "/p/a.json").is_none(), "a cycle ends instead of looping");
        let own = [("/p/tsconfig.json", r#"{"extends": "@x/y", "compilerOptions": {"paths": {"@/*": ["s/*"]}}}"#)];
        assert_eq!(bases(paths_of(&own, "/p/tsconfig.json").unwrap().target("@/a")), ["/p/s/a"]);
        // No compilerOptions at all is a readable config with no aliases.
        let plain = [("/p/tsconfig.json", r#"{"include": ["src"]}"#)];
        assert_eq!(bases(paths_of(&plain, "/p/tsconfig.json").unwrap().target("pkg")), ["no match"]);
        // A chain deeper than the limit is cut.
        let deep: Vec<(String, String)> = (0..8)
            .map(|i| (format!("/p/c{i}.json"), format!(r#"{{"extends": "./c{}.json"}}"#, i + 1)))
            .collect();
        let deep: Vec<(&str, &str)> = deep.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
        assert!(paths_of(&deep, "/p/c0.json").is_none());
    }

    #[test]
    fn a_key_lists_its_targets_in_order_and_unsupported_ones_are_skipped() {
        let files = [("/p/tsconfig.json", r#"{"compilerOptions": {"paths": {"m/*": ["a/*", "b/*"], "abs": ["/etc/x"], "mixed": ["/etc/x", "ok"]}}}"#)];
        let config = paths_of(&files, "/p/tsconfig.json").unwrap();
        assert_eq!(bases(config.target("m/x")), ["/p/a/x", "/p/b/x"]);
        assert_eq!(bases(config.target("abs")), ["unsupported"]);
        assert_eq!(bases(config.target("mixed")), ["/p/ok"]);
    }

    #[test]
    fn extended_paths_follows_only_local_files_inside_the_root() {
        let at = |text: &str| -> Vec<String> {
            extended_paths(Path::new("/p"), Path::new("/p/apps/web/tsconfig.json"), text)
                .iter()
                .map(|p| p.to_str().unwrap().to_owned())
                .collect()
        };
        assert_eq!(at(r#"{"extends": "../../base"}"#), ["/p/base.json"]);
        assert_eq!(at(r#"{"extends": ["./a.json", "pkg", "../b"]}"#), ["/p/apps/web/a.json", "/p/apps/b.json"]);
        assert!(at(r#"{"extends": "../../../out"}"#).is_empty(), "outside the root");
        assert!(at("not json").is_empty());
    }

    fn pkgs(files: &[(&str, &str)]) -> Vec<Package> {
        packages(&configs(files))
    }

    #[test]
    fn a_workspace_package_resolves_through_exports_main_and_subpaths() {
        let list = pkgs(&[
            ("/p/packages/ui/package.json", r#"{"name": "@acme/ui", "exports": {".": {"types": "./src/index.ts", "default": "./dist/index.js"}, "./*": "./src/*.ts", "./theme": "./src/theme/index.ts"}}"#),
            ("/p/packages/ui-kit/package.json", r#"{"name": "@acme/ui-kit", "main": "lib/main.ts"}"#),
            ("/p/packages/core/package.json", r#"{"name": "core"}"#),
            ("/p/packages/anon/package.json", r#"{"version": "1.0.0"}"#),
        ]);
        let base = |value: &str| package_base(&list, value).map(|r| r.map(|p| p.to_str().unwrap().to_owned()));
        assert_eq!(base("@acme/ui"), Some(Ok("/p/packages/ui/dist/index.js".into())), "default comes before types");
        assert_eq!(base("@acme/ui/button"), Some(Ok("/p/packages/ui/src/button.ts".into())), "a ./* pattern");
        assert_eq!(base("@acme/ui/theme"), Some(Ok("/p/packages/ui/src/theme/index.ts".into())), "an exact subpath wins over a pattern");
        assert_eq!(base("@acme/ui-kit"), Some(Ok("/p/packages/ui-kit/lib/main.ts".into())), "the longer name is tried first");
        assert_eq!(base("core"), Some(Ok("/p/packages/core".into())), "no exports, no main: the folder's index");
        assert_eq!(base("core/sub/file"), Some(Ok("/p/packages/core/sub/file".into())), "no exports: any subpath of the folder");
        assert_eq!(base("react"), None);
        assert_eq!(base("@acme/uix"), None, "a name is not a prefix");
        let closed = pkgs(&[("/p/p/package.json", r#"{"name": "closed", "exports": {".": "./index.ts"}}"#)]);
        assert_eq!(package_base(&closed, "closed/other"), Some(Err(())), "exports that do not list the subpath");
    }

    fn aliases(name: &str, text: &str) -> Vec<(String, String)> {
        vite_aliases(Path::new(name), text).into_iter().map(|(a, p)| (a, p.to_str().unwrap().to_owned())).collect()
    }

    #[test]
    fn vite_aliases_are_read_in_the_forms_people_write() {
        let object = r#"
            import { defineConfig } from 'vite';
            import path from 'node:path';
            import { fileURLToPath, URL } from 'node:url';
            export default defineConfig({
              resolve: { alias: {
                '@': path.resolve(__dirname, 'src'),
                '~': fileURLToPath(new URL('./web', import.meta.url)),
                '@lib': path.join(__dirname, 'packages', 'lib'),
                utils: './src/utils',
                '/abs': '/src/abs',
                dynamic: someFunction('x'),
              } },
            });"#;
        let found = aliases("/p/vite.config.ts", object);
        assert!(found.contains(&("@".into(), "/p/src".into())));
        assert!(found.contains(&("~".into(), "/p/web".into())));
        assert!(found.contains(&("@lib".into(), "/p/packages/lib".into())));
        assert!(found.contains(&("utils".into(), "/p/src/utils".into())));
        assert!(found.contains(&("/abs".into(), "/p/src/abs".into())), "/src is the project's, not the disk's");
        assert!(!found.iter().any(|(a, _)| a == "dynamic"), "what is not a path is skipped");
        assert!(found.windows(2).all(|w| w[0].0.len() >= w[1].0.len()), "longest alias first: {found:?}");

        let array = r#"export default { resolve: { alias: [
            { find: '@', replacement: path.resolve(__dirname, './src') },
            { find: /^~(.*)$/, replacement: 'x' },
        ] } };"#;
        assert_eq!(aliases("/p/web/vite.config.js", array), [("@".to_owned(), "/p/web/src".to_owned())], "a regex find is not followed");
        assert!(aliases("/p/vite.config.ts", "export default {};").is_empty());
    }

    #[test]
    fn vite_aliases_match_the_alias_alone_or_followed_by_a_slash() {
        let list = vec![("@lib".to_owned(), PathBuf::from("/p/lib")), ("@".to_owned(), PathBuf::from("/p/src"))];
        assert_eq!(vite_base(&list, "@lib/x"), Some(PathBuf::from("/p/lib/x")));
        assert_eq!(vite_base(&list, "@/a/b"), Some(PathBuf::from("/p/src/a/b")));
        assert_eq!(vite_base(&list, "@"), Some(PathBuf::from("/p/src")));
        assert_eq!(vite_base(&list, "@types/node"), None, "@types is not @ followed by a slash");
    }

    #[test]
    fn config_names() {
        for name in ["tsconfig.json", "tsconfig.base.json", "jsconfig.json", "vite.config.mjs", "go.mod", "package.json"] {
            assert!(is_config_name(name), "{name}");
        }
        assert!(!is_config_name("tsconfig.txt") && !is_config_name("vite.config.rs") && !is_config_name("main.ts"));
        assert!(is_ts_config_name("tsconfig.build.json") && !is_ts_config_name("package.json"));
    }
}
