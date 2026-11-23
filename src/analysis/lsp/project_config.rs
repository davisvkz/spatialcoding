//! `.codecanvas/settings.json` of a repository: it can suggest the language
//! server command per language,
//!
//! ```json
//! { "languageServers": { "rust": { "command": ["direnv", "exec", ".", "rust-analyzer"] } } }
//! ```
//!
//! A repository may belong to someone else, so the app never runs a suggested
//! command on its own: it asks once for each exact (folder, language, command)
//! and remembers the answer (`codecanvas/mcp-e-servidor-local.md`). Until the
//! answer is yes, the default command runs. A user override
//! (`SPATIALCODING_LSP_*`) always wins.
//!
//! The file is untrusted input, so it is bounded: at most [`MAX_FILE_BYTES`]
//! and a regular file, and a command of at most [`MAX_ARGS`] arguments of at
//! most [`MAX_ARG_CHARS`] characters, every one a string.

use std::collections::BTreeMap;
use std::io::Read;
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

/// The largest `.codecanvas/settings.json` that is read.
pub const MAX_FILE_BYTES: u64 = 64 * 1024;
/// The most arguments a suggested command may have.
pub const MAX_ARGS: usize = 64;
/// The most characters one argument may have.
pub const MAX_ARG_CHARS: usize = 4096;

/// What a repository suggests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectConfig {
    pub language_servers: BTreeMap<String, Vec<String>>,
}

/// Reads `<root>/.codecanvas/settings.json`; `None` when absent, invalid,
/// not a regular file or larger than [`MAX_FILE_BYTES`].
pub fn load(root: &Path) -> Option<ProjectConfig> {
    parse(&read_bounded(&root.join(".codecanvas").join("settings.json"))?)
}

/// The text of a regular file of at most [`MAX_FILE_BYTES`]. The size is
/// checked on the opened file and again while reading, so a file that grows
/// (or a special file that never ends) cannot be read whole.
fn read_bounded(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let mut text = String::new();
    file.take(MAX_FILE_BYTES + 1).read_to_string(&mut text).ok()?;
    (text.len() as u64 <= MAX_FILE_BYTES).then_some(text)
}

pub fn parse(text: &str) -> Option<ProjectConfig> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let servers = value.get("languageServers")?.as_object()?;
    let mut config = ProjectConfig::default();
    for (language, entry) in servers {
        let Some(words) = entry.get("command").and_then(|c| c.as_array()) else { continue };
        if let Some(command) = bounded_command(words) {
            config.language_servers.insert(language.clone(), command);
        }
    }
    Some(config)
}

/// The command when it is non-empty, within the limits and made only of
/// strings. A suggestion that breaks a rule is dropped whole: running the
/// other words of a changed command would be running a command nobody wrote.
fn bounded_command(words: &[serde_json::Value]) -> Option<Vec<String>> {
    if words.is_empty() || words.len() > MAX_ARGS {
        return None;
    }
    words
        .iter()
        .map(|w| w.as_str().filter(|s| s.chars().count() <= MAX_ARG_CHARS).map(str::to_owned))
        .collect()
}

/// The answer given to one suggestion: a command for a language in a folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answer {
    pub root: PathBuf,
    /// The [`language_key`]. Answers saved before it existed have none and
    /// match nothing, so they are asked again.
    #[serde(default)]
    pub language: String,
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
    let language = language_key(server);
    match trust.answers.iter().find(|a| a.root == root && a.language == language && a.command == command) {
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

        let mut answers = vec![Answer { root: root.clone(), language: "rust".into(), command: command.clone(), allowed: true }];
        sync_answers(&root, &answers);
        assert_eq!(command_for(ServerId::Rust, &root), Some(command.clone()));
        // The answer is for this exact command in this folder.
        answers[0].command.push("--other".into());
        sync_answers(&root, &answers);
        assert_eq!(command_for(ServerId::Rust, &root), None, "another command is another question");
        answers[0] = Answer { root: root.clone(), language: "rust".into(), command, allowed: false };
        sync_answers(&root, &answers);
        assert_eq!(command_for(ServerId::Rust, &root), None, "a no stays a no");
        sync_answers(&root, &[]);
        assert_eq!(command_for(ServerId::Go, &root), None, "no suggestion for Go");
    }

    fn project_with(settings: &[u8]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".codecanvas")).unwrap();
        std::fs::write(dir.path().join(".codecanvas/settings.json"), settings).unwrap();
        dir
    }

    #[test]
    fn a_settings_file_over_the_limit_or_not_regular_is_not_read() {
        let ok = project_with(SAMPLE.as_bytes());
        assert!(load(ok.path()).is_some());

        let padding = " ".repeat(MAX_FILE_BYTES as usize);
        let big = project_with(format!("{SAMPLE}{padding}").as_bytes());
        assert!(load(big.path()).is_none(), "larger than 64 KiB");
        let edge = project_with(format!("{SAMPLE}{}", " ".repeat(MAX_FILE_BYTES as usize - SAMPLE.len())).as_bytes());
        assert!(load(edge.path()).is_some(), "exactly 64 KiB is fine");

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".codecanvas/settings.json")).unwrap();
        assert!(load(dir.path()).is_none(), "a folder named settings.json");
        #[cfg(unix)]
        {
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(dir.path().join(".codecanvas")).unwrap();
            std::os::unix::fs::symlink("/dev/zero", dir.path().join(".codecanvas/settings.json")).unwrap();
            assert!(load(dir.path()).is_none(), "a device never ends and is not read");
        }
    }

    #[test]
    fn a_command_over_the_argument_limits_is_dropped_whole() {
        let command = |words: Vec<serde_json::Value>| {
            let doc = serde_json::json!({"languageServers": {"rust": {"command": words}, "go": {"command": ["gopls"]}}});
            parse(&doc.to_string()).unwrap().language_servers
        };
        let words = |n: usize| vec![serde_json::json!("x"); n];
        assert_eq!(command(words(MAX_ARGS))["rust"].len(), MAX_ARGS);
        assert!(!command(words(MAX_ARGS + 1)).contains_key("rust"), "65 arguments");
        let long = |n: usize| vec![serde_json::json!("a".repeat(n))];
        assert!(command(long(MAX_ARG_CHARS)).contains_key("rust"));
        assert!(!command(long(MAX_ARG_CHARS + 1)).contains_key("rust"), "an argument of 4097 characters");
        assert!(
            !command(vec![serde_json::json!("rust-analyzer"), serde_json::json!(5)]).contains_key("rust"),
            "a non-string word would otherwise run a command nobody wrote"
        );
        assert!(command(words(MAX_ARGS + 1)).contains_key("go"), "the other languages are kept");
    }

    #[test]
    fn an_answer_is_for_one_language() {
        let doc = r#"{"languageServers": {"rust": {"command": ["tool", "--serve"]}, "go": {"command": ["tool", "--serve"]}}}"#;
        let dir = project_with(doc.as_bytes());
        let root = dir.path().to_path_buf();
        let command: Vec<String> = ["tool", "--serve"].map(String::from).to_vec();
        let answer = |language: &str| Answer { root: root.clone(), language: language.into(), command: command.clone(), allowed: true };

        sync_answers(&root, &[answer("rust")]);
        assert_eq!(command_for(ServerId::Rust, &root), Some(command.clone()));
        assert_eq!(command_for(ServerId::Go, &root), None, "the same argv for another language asks again");
        assert!(take_pending(&root).iter().any(|p| p.server == ServerId::Go));

        // An answer saved before `language` existed has none and matches nothing.
        let legacy: Answer = serde_json::from_str(&format!(
            r#"{{"root": {:?}, "command": ["tool", "--serve"], "allowed": true}}"#,
            root.to_string_lossy()
        ))
        .unwrap();
        assert_eq!(legacy.language, "");
        sync_answers(&root, &[legacy]);
        assert_eq!(command_for(ServerId::Rust, &root), None);
        sync_answers(&root, &[]);
    }
}
