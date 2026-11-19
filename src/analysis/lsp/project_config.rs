//! `.codecanvas/settings.json` of a repository: it can suggest the language
//! server command per language,
//!
//! ```json
//! { "languageServers": { "rust": { "command": ["direnv", "exec", ".", "rust-analyzer"] } } }
//! ```
//!
//! A repository may belong to someone else, so the app never runs a suggested
//! command on its own: it asks once for each exact command in each folder and
//! remembers the answer (`codecanvas/mcp-e-servidor-local.md`). Until the
//! answer is yes, the default command runs. A user override
//! (`SPATIALCODING_LSP_*`) always wins.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use serde::{Deserialize, Serialize};

use super::ServerId;

/// The key of a language in `languageServers`.
pub fn language_key(server: ServerId) -> &'static str {
    match server {
        ServerId::Rust => "rust",
        ServerId::TypeScript => "typescript",
        ServerId::Python => "python",
        ServerId::Go => "go",
    }
}

/// What a repository suggests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectConfig {
    pub language_servers: BTreeMap<String, Vec<String>>,
}

/// Reads `<root>/.codecanvas/settings.json`; `None` when absent or invalid.
pub fn load(root: &Path) -> Option<ProjectConfig> {
    let text = std::fs::read_to_string(root.join(".codecanvas").join("settings.json")).ok()?;
    parse(&text)
}

pub fn parse(text: &str) -> Option<ProjectConfig> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let servers = value.get("languageServers")?.as_object()?;
    let mut config = ProjectConfig::default();
    for (language, entry) in servers {
        let Some(words) = entry.get("command").and_then(|c| c.as_array()) else { continue };
        let command: Vec<String> = words.iter().filter_map(|w| w.as_str().map(str::to_owned)).collect();
        if !command.is_empty() {
            config.language_servers.insert(language.clone(), command);
        }
    }
    Some(config)
}

/// The answer given to one suggestion: a command in a folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answer {
    pub root: PathBuf,
    pub command: Vec<String>,
    pub allowed: bool,
}

/// A suggestion waiting for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    pub root: PathBuf,
    pub server: ServerId,
    pub command: Vec<String>,
}

#[derive(Default)]
struct Trust {
    answers: Vec<Answer>,
    pending: Vec<Proposal>,
    /// Proposals already shown, so a denied one is not asked again.
    asked: Vec<Proposal>,
}

static TRUST: LazyLock<Mutex<Trust>> = LazyLock::new(Mutex::default);

/// Replaces the remembered answers for the repository `root` (from the
/// settings file) each frame. The store is shared by every window of the
/// process, so only the answers of `root` are touched.
pub fn sync_answers(root: &Path, answers: &[Answer]) {
    let mine: Vec<&Answer> = answers.iter().filter(|a| a.root == root).collect();
    let Ok(mut trust) = TRUST.lock() else { return };
    let current: Vec<&Answer> = trust.answers.iter().filter(|a| a.root == root).collect();
    if current != mine {
        let mine: Vec<Answer> = mine.into_iter().cloned().collect();
        trust.answers.retain(|a| a.root != root);
        trust.answers.extend(mine);
    }
}

/// The command a repository suggests for `server`, when the user allowed it.
/// An unanswered suggestion is queued for the dialog and `None` returned, so
/// the caller falls back to the default command.
pub fn command_for(server: ServerId, root: &Path) -> Option<Vec<String>> {
    let config = load(root)?;
    let command = config.language_servers.get(language_key(server))?.clone();
    let mut trust = TRUST.lock().ok()?;
    match trust.answers.iter().find(|a| a.root == root && a.command == command) {
        Some(answer) => answer.allowed.then_some(command),
        None => {
            let proposal = Proposal { root: root.to_path_buf(), server, command };
            if !trust.asked.contains(&proposal) && !trust.pending.contains(&proposal) {
                trust.pending.push(proposal);
            }
            None
        }
    }
}

/// The suggestions for the repository `root` nobody answered yet, each
/// returned once (the queue is shared by every window of the process).
pub fn take_pending(root: &Path) -> Vec<Proposal> {
    let Ok(mut trust) = TRUST.lock() else { return Vec::new() };
    let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut trust.pending).into_iter().partition(|p| p.root == root);
    trust.pending = rest;
    trust.asked.extend(mine.iter().cloned());
    mine
}

/// Makes the dialog ask again about `proposal` (it was dismissed unanswered).
pub fn forget_asked(proposal: &Proposal) {
    if let Ok(mut trust) = TRUST.lock() {
        trust.asked.retain(|p| p != proposal);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"languageServers": {"rust": {"command": ["direnv", "exec", ".", "rust-analyzer"]}, "go": {"command": []}, "python": {}}}"#;

    #[test]
    fn the_suggested_commands_are_read_per_language() {
        let config = parse(SAMPLE).unwrap();
        assert_eq!(config.language_servers["rust"], ["direnv", "exec", ".", "rust-analyzer"]);
        assert!(!config.language_servers.contains_key("go"), "an empty command is no suggestion");
        assert!(parse("not json").is_none() && parse("{}").is_none());
        assert_eq!(language_key(ServerId::TypeScript), "typescript");
    }

    #[test]
    fn a_suggestion_is_asked_once_and_runs_only_after_a_yes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".codecanvas")).unwrap();
        std::fs::write(dir.path().join(".codecanvas/settings.json"), SAMPLE).unwrap();
        let root = dir.path().to_path_buf();
        let command: Vec<String> = ["direnv", "exec", ".", "rust-analyzer"].map(String::from).to_vec();

        assert_eq!(command_for(ServerId::Rust, &root), None, "unanswered: the default runs");
        let pending = take_pending(&root);
        assert_eq!(pending, vec![Proposal { root: root.clone(), server: ServerId::Rust, command: command.clone() }]);
        assert_eq!(command_for(ServerId::Rust, &root), None);
        assert!(take_pending(&root).is_empty(), "asked once");

        let mut answers = vec![Answer { root: root.clone(), command: command.clone(), allowed: true }];
        sync_answers(&root, &answers);
        assert_eq!(command_for(ServerId::Rust, &root), Some(command.clone()));
        // The answer is for this exact command in this folder.
        answers[0].command.push("--other".into());
        sync_answers(&root, &answers);
        assert_eq!(command_for(ServerId::Rust, &root), None, "another command is another question");
        answers[0] = Answer { root: root.clone(), command, allowed: false };
        sync_answers(&root, &answers);
        assert_eq!(command_for(ServerId::Rust, &root), None, "a no stays a no");
        sync_answers(&root, &[]);
        assert_eq!(command_for(ServerId::Go, &root), None, "no suggestion for Go");
    }
}
