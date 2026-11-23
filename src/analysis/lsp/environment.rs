//! The environment language servers start with.
//!
//! An app started from a launcher gets a minimal `PATH`, so a toolchain
//! installed by rustup, mise, asdf or Nix is not found by rust-analyzer or
//! gopls. The project's environment is what the user's login shell has in the
//! project folder, plus what direnv exports for an `.envrc` found above it:
//!
//! 1. `$SHELL -l -i -c` prints `env -0` between two markers, because an
//!    interactive shell also prints banners and prompts (15 s limit);
//! 2. when an `.envrc` exists, `direnv export json` runs with that
//!    environment (60 s limit); a blocked `.envrc` becomes a notice telling
//!    the user to run `direnv allow`, and the server still starts.
//!
//! The result is cached per (folder, `.envrc` and its modification time), so
//! the servers of one folder share one shell run, and read, never computed,
//! by the UI ([`cached`]). The Language Servers dialog has a switch, and
//! `SPATIALCODING_LSP_SHELL_ENV=0` turns it off.

use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

/// How long the login shell may take.
pub const SHELL_TIMEOUT: Duration = Duration::from_secs(15);
/// How long direnv may take (it can build a Nix shell).
pub const DIRENV_TIMEOUT: Duration = Duration::from_secs(60);

/// Set to `0` to start servers with the app's own environment.
pub const KILL_SWITCH: &str = "SPATIALCODING_LSP_SHELL_ENV";

/// Shells whose `-l -i -c` and `env -0` behave as the loader expects.
const KNOWN_SHELLS: &[&str] = &["sh", "bash", "zsh", "dash", "ksh", "mksh", "fish"];

/// Set by the shell itself on every run, so they say nothing about the project.
const SKIPPED: &[&str] = &["_", "SHLVL", "PWD", "OLDPWD"];

/// What to add to the environment a server starts with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectEnv {
    pub set: BTreeMap<String, String>,
    /// Variables direnv removes.
    pub unset: Vec<String>,
    /// Why the environment is incomplete (shell failed, `.envrc` blocked),
    /// for the dialog.
    pub notice: Option<String>,
}

impl ProjectEnv {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.set.get(key).map(String::as_str)
    }

    /// The `PATH` servers are looked up in.
    pub fn path(&self) -> Option<&str> {
        self.get("PATH")
    }

    /// Applies it over the environment `command` inherits.
    pub fn apply(&self, command: &mut Command) {
        for (key, value) in &self.set {
            command.env(key, value);
        }
        for key in &self.unset {
            command.env_remove(key);
        }
    }
}

/// The programs and limits [`load`] uses, injectable for tests.
#[derive(Debug, Clone)]
pub struct Tools {
    pub shell: Option<PathBuf>,
    pub shell_timeout: Duration,
    pub direnv: String,
    pub direnv_timeout: Duration,
}

impl Tools {
    pub fn system() -> Self {
        Self {
            shell: std::env::var_os("SHELL").map(PathBuf::from).filter(|s| !s.as_os_str().is_empty()),
            shell_timeout: SHELL_TIMEOUT,
            direnv: "direnv".to_owned(),
            direnv_timeout: DIRENV_TIMEOUT,
        }
    }
}

static ENABLED: AtomicBool = AtomicBool::new(true);

/// The Language Servers dialog's switch, copied here each frame so the
/// server threads can read it.
pub fn sync_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// Whether servers start with the project's environment.
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed) && std::env::var(KILL_SWITCH).map_or(true, |v| v.trim() != "0")
}

/// `.envrc` of `root` or the nearest folder above it.
pub fn find_envrc(root: &Path) -> Option<PathBuf> {
    root.ancestors().map(|dir| dir.join(".envrc")).find(|p| p.is_file())
}

type Key = (PathBuf, Option<(PathBuf, Option<SystemTime>)>);
type Cell = Arc<OnceLock<Arc<ProjectEnv>>>;

static CACHE: LazyLock<Mutex<HashMap<Key, Cell>>> = LazyLock::new(Mutex::default);

fn key_for(root: &Path) -> Key {
    let envrc = find_envrc(root).map(|path| {
        let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        (path, modified)
    });
    (root.to_path_buf(), envrc)
}

/// The environment for servers started in `root`, loaded on first use (this
/// blocks, so call it from a worker thread) and shared after. `None` when it
/// is turned off.
pub fn env_for(root: &Path) -> Option<Arc<ProjectEnv>> {
    env_for_with(root, &Tools::system())
}

pub fn env_for_with(root: &Path, tools: &Tools) -> Option<Arc<ProjectEnv>> {
    if !enabled() {
        return None;
    }
    let cell = CACHE.lock().ok()?.entry(key_for(root)).or_default().clone();
    // Concurrent callers of the same key wait here for the one run.
    Some(cell.get_or_init(|| Arc::new(load(root, tools))).clone())
}

/// What [`env_for`] loaded, if it did; never starts a shell, so the UI may
/// call it every frame.
pub fn cached(root: &Path) -> Option<Arc<ProjectEnv>> {
    if !enabled() {
        return None;
    }
    let cell = CACHE.lock().ok()?.get(&key_for(root))?.clone();
    cell.get().cloned()
}

/// Runs the shell, then direnv. Never fails: what went wrong is the notice.
pub fn load(root: &Path, tools: &Tools) -> ProjectEnv {
    let mut env = ProjectEnv::default();
    match &tools.shell {
        None => env.notice = Some("SHELL is not set, so the project environment was not loaded.".to_owned()),
        Some(shell) => match load_shell_env(shell, root, tools.shell_timeout) {
            Ok(vars) => env.set = vars,
            Err(e) => env.notice = Some(format!("Could not load the shell environment: {e}")),
        },
    }
    let Some(envrc) = find_envrc(root) else { return env };
    let path = env.path().map(str::to_owned);
    let Some(direnv) = which_in(&tools.direnv, path.as_deref()) else {
        env.notice.get_or_insert_with(|| format!("{} exists but direnv was not found.", envrc.display()));
        return env;
    };
    match direnv_export(&direnv, root, &env.set, tools.direnv_timeout) {
        Ok(Export::Applied { set, unset }) => {
            env.set.extend(set);
            env.unset = unset;
        }
        Ok(Export::Blocked) => {
            let dir = envrc.parent().unwrap_or(root);
            env.notice = Some(format!("{} is blocked: run `direnv allow` in {}.", envrc.display(), dir.display()));
        }
        Err(e) => env.notice = Some(format!("direnv failed: {e}")),
    }
    env
}

/// The shell's environment in `root`.
pub fn load_shell_env(shell: &Path, root: &Path, timeout: Duration) -> Result<BTreeMap<String, String>, String> {
    let name = shell.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    if !KNOWN_SHELLS.contains(&name) {
        return Err(format!("{name} is not a supported shell"));
    }
    let marker = fresh_marker();
    let mut command = Command::new(shell);
    command.args(["-l", "-i", "-c", &env_script(&marker)]).current_dir(root);
    let out = run_with_timeout(command, timeout)?;
    parse_marked_env(&out.stdout, &marker).ok_or_else(|| {
        let tail = String::from_utf8_lossy(&out.stderr);
        format!("the shell printed no environment ({})", tail.lines().last().unwrap_or("no error output"))
    })
}

fn fresh_marker() -> String {
    let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    format!("__SPATIALCODING_ENV_{}_{nanos}__", std::process::id())
}

/// Prints the environment between two markers. `env -0` keeps values with
/// newlines whole; the plain `env` is for systems without `-0`.
fn env_script(marker: &str) -> String {
    format!("printf '%s' '{marker}BEGIN'; env -0 2>/dev/null || env; printf '%s' '{marker}END'")
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|i| i + from)
}

/// The variables printed by [`env_script`]: whatever the shell printed
/// outside the markers (banners, prompts, plugin output) is ignored.
pub fn parse_marked_env(stdout: &[u8], marker: &str) -> Option<BTreeMap<String, String>> {
    let begin = format!("{marker}BEGIN");
    let end = format!("{marker}END");
    let start = find(stdout, begin.as_bytes(), 0)? + begin.len();
    let stop = find(stdout, end.as_bytes(), start)?;
    let body = &stdout[start..stop];
    let separator = if body.contains(&0) { 0 } else { b'\n' };
    let mut vars = BTreeMap::new();
    for entry in body.split(|b| *b == separator) {
        let text = String::from_utf8_lossy(entry);
        let Some((key, value)) = text.split_once('=') else { continue };
        let valid = key.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if valid && !SKIPPED.contains(&key) {
            vars.insert(key.to_owned(), value.to_owned());
        }
    }
    (!vars.is_empty()).then_some(vars)
}

/// What `direnv export json` said.
#[derive(Debug, PartialEq, Eq)]
pub enum Export {
    Applied { set: BTreeMap<String, String>, unset: Vec<String> },
    Blocked,
}

/// Runs `direnv export json` in `root` on top of `base`.
pub fn direnv_export(
    direnv: &Path,
    root: &Path,
    base: &BTreeMap<String, String>,
    timeout: Duration,
) -> Result<Export, String> {
    let mut command = Command::new(direnv);
    command.args(["export", "json"]).current_dir(root).envs(base);
    let out = run_with_timeout(command, timeout)?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    if is_blocked(&stderr) {
        return Ok(Export::Blocked);
    }
    if !out.success {
        return Err(stderr.lines().last().unwrap_or("it failed").to_owned());
    }
    parse_direnv_json(&String::from_utf8_lossy(&out.stdout))
}

/// direnv refuses an `.envrc` the user has not allowed.
pub fn is_blocked(stderr: &str) -> bool {
    stderr.contains("is blocked") || stderr.contains("direnv allow")
}

/// `{"VAR": "value", "GONE": null}`; empty output means nothing changed.
pub fn parse_direnv_json(text: &str) -> Result<Export, String> {
    if text.trim().is_empty() {
        return Ok(Export::Applied { set: BTreeMap::new(), unset: Vec::new() });
    }
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("unreadable output: {e}"))?;
    let object = value.as_object().ok_or("the output is not an object")?;
    let (mut set, mut unset) = (BTreeMap::new(), Vec::new());
    for (key, value) in object {
        match value {
            serde_json::Value::String(v) => {
                set.insert(key.clone(), v.clone());
            }
            serde_json::Value::Null => unset.push(key.clone()),
            _ => {}
        }
    }
    Ok(Export::Applied { set, unset })
}

/// `program` as a path: itself when it has a directory part, else the first
/// executable of that name in `path` (the process's `PATH` when `None`).
pub fn which_in(program: &str, path: Option<&str>) -> Option<PathBuf> {
    if program.contains('/') {
        return Some(PathBuf::from(program));
    }
    let path = path.map(std::ffi::OsString::from).or_else(|| std::env::var_os("PATH"))?;
    std::env::split_paths(&path).map(|dir| dir.join(program)).find(|p| super::is_executable(p))
}

struct Finished {
    success: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

/// Runs `command` without input, in its own process group so a timeout can
/// kill the shell and everything it started.
fn run_with_timeout(mut command: Command, timeout: Duration) -> Result<Finished, String> {
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = spawn_retrying(&mut command).map_err(|e| e.to_string())?;
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(e) => return Err(e.to_string()),
        }
        if Instant::now() >= deadline {
            kill_group(&mut child);
            return Err(format!("it did not finish in {} s", timeout.as_secs_f32()));
        }
        thread::sleep(Duration::from_millis(10));
    };
    // The pipes close when everything the shell started has ended; a daemon
    // left behind must not hold the loader.
    let grace = Instant::now() + Duration::from_millis(500);
    let collect = |rx: Receiver<Vec<u8>>| rx.recv_timeout(grace.saturating_duration_since(Instant::now())).unwrap_or_default();
    Ok(Finished { success: status.success(), stdout: collect(stdout), stderr: collect(stderr) })
}

/// Starts `command`, trying again a few times while the program is "busy": a
/// file written a moment ago may still be open for writing in a child that
/// another thread forked, until that child execs. Rare, and over by itself.
fn spawn_retrying(command: &mut Command) -> std::io::Result<Child> {
    let mut attempts = 0;
    loop {
        match command.spawn() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy && attempts < 5 => {
                attempts += 1;
                thread::sleep(Duration::from_millis(20));
            }
            other => return other,
        }
    }
}

fn drain(pipe: Option<impl Read + Send + 'static>) -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    match pipe {
        Some(mut pipe) => {
            thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = pipe.read_to_end(&mut buf);
                let _ = tx.send(buf);
            });
        }
        None => {
            let _ = tx.send(Vec::new());
        }
    }
    rx
}

fn kill_group(child: &mut Child) {
    #[cfg(unix)]
    // SAFETY: `kill` only sends a signal; the group id is the child's, which
    // `process_group(0)` made equal to its pid.
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn printed(marker: &str, body: &str) -> Vec<u8> {
        format!("welcome to zsh\n$ {marker}BEGIN{body}{marker}END\nlogout\n").into_bytes()
    }

    #[test]
    fn the_markers_cut_the_environment_out_of_the_shells_noise() {
        let nul = printed("M", "PATH=/a:/b\0EDITOR=vi\0MULTI=one\ntwo\0_=/usr/bin/env\0SHLVL=3\0PWD=/x\0");
        let vars = parse_marked_env(&nul, "M").unwrap();
        assert_eq!(vars["PATH"], "/a:/b");
        assert_eq!(vars["MULTI"], "one\ntwo", "NUL keeps a value with a newline whole");
        for skipped in ["_", "SHLVL", "PWD"] {
            assert!(!vars.contains_key(skipped), "{skipped} says nothing about the project");
        }
        // Without `env -0` the entries are lines; junk lines are not variables.
        let plain = printed("M", "PATH=/a\nHOME=/home/u\nnot a variable\n9BAD=1\n");
        let vars = parse_marked_env(&plain, "M").unwrap();
        assert_eq!(vars.keys().collect::<Vec<_>>(), ["HOME", "PATH"]);
        assert_eq!(parse_marked_env(b"no markers at all", "M"), None);
        assert_eq!(parse_marked_env(b"MBEGIN only", "M"), None);
        assert_eq!(parse_marked_env(&printed("M", ""), "M"), None, "an empty environment is a failure");
    }

    #[test]
    fn direnv_output_sets_unsets_and_may_be_empty() {
        let export = parse_direnv_json(r#"{"PATH": "/nix/bin:/usr/bin", "GONE": null, "N": 3}"#).unwrap();
        let Export::Applied { set, unset } = export else { panic!("not applied") };
        assert_eq!(set["PATH"], "/nix/bin:/usr/bin");
        assert_eq!(unset, ["GONE"]);
        assert!(!set.contains_key("N"), "only strings are values");
        assert_eq!(parse_direnv_json("  \n").unwrap(), Export::Applied { set: BTreeMap::new(), unset: vec![] });
        assert!(parse_direnv_json("[1]").is_err() && parse_direnv_json("{oops").is_err());
        assert!(is_blocked("direnv: error /p/.envrc is blocked. Run `direnv allow` to approve its content"));
        assert!(!is_blocked("direnv: loading /p/.envrc"));
    }

    #[test]
    fn a_shell_that_never_finishes_is_killed_with_what_it_started() {
        let dir = tempfile::tempdir().unwrap();
        let slow = script(dir.path(), "sh", "sleep 30 &\nsleep 30");
        let started = Instant::now();
        let err = load_shell_env(&slow, dir.path(), Duration::from_millis(200)).unwrap_err();
        assert!(err.contains("did not finish"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
    }

    #[test]
    fn the_login_shell_environment_is_read_past_its_banner() {
        let dir = tempfile::tempdir().unwrap();
        // Runs the loader's script, like a login shell would, after a banner and
        // with a variable only the shell sets.
        let shell = script(dir.path(), "sh", "echo 'last login: today'\nFROM_SHELL=yes exec /bin/sh -c \"$4\"");
        let vars = load_shell_env(&shell, dir.path(), Duration::from_secs(10)).unwrap();
        assert_eq!(vars["FROM_SHELL"], "yes");
        assert!(vars.contains_key("PATH"));
        let unsupported = script(dir.path(), "nu", "true");
        assert!(load_shell_env(&unsupported, dir.path(), Duration::from_secs(1)).unwrap_err().contains("not a supported shell"));
    }

    fn tools(dir: &Path, direnv: &str) -> Tools {
        Tools {
            shell: Some(script(dir, "sh", "FROM_SHELL=yes exec /bin/sh -c \"$4\"")),
            shell_timeout: Duration::from_secs(10),
            direnv: direnv.to_owned(),
            direnv_timeout: Duration::from_secs(10),
        }
    }

    #[test]
    fn direnv_changes_are_merged_over_the_shell_environment() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::write(root.join(".envrc"), "use flake\n").unwrap();
        let direnv = script(&root, "direnv", r#"echo '{"FROM_DIRENV": "1", "FROM_SHELL": null, "PATH": "/nix/bin"}'"#);
        let env = load(&root, &tools(&root, direnv.to_str().unwrap()));
        assert_eq!(env.get("FROM_DIRENV"), Some("1"));
        assert_eq!(env.path(), Some("/nix/bin"), "direnv has the last word");
        assert_eq!(env.unset, ["FROM_SHELL"]);
        assert_eq!(env.notice, None);
        let mut command = Command::new("true");
        env.apply(&mut command);
        let envs: Vec<_> = command.get_envs().collect();
        assert!(envs.iter().any(|(k, v)| *k == "FROM_DIRENV" && v.is_some()));
        assert!(envs.iter().any(|(k, v)| *k == "FROM_SHELL" && v.is_none()), "removed from the child");
    }

    #[test]
    fn a_blocked_envrc_is_a_notice_and_the_shell_environment_still_applies() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::write(root.join(".envrc"), "use flake\n").unwrap();
        let direnv = script(&root, "direnv", "echo 'direnv: error .envrc is blocked. Run `direnv allow`' >&2\nexit 1");
        let env = load(&root, &tools(&root, direnv.to_str().unwrap()));
        assert_eq!(env.get("FROM_SHELL"), Some("yes"));
        let notice = env.notice.unwrap();
        assert!(notice.contains("direnv allow") && notice.contains(root.to_str().unwrap()), "{notice}");

        let missing = load(&root, &tools(&root, "no-such-direnv-binary"));
        assert!(missing.notice.unwrap().contains("direnv was not found"));
        let no_envrc = tempfile::tempdir().unwrap();
        assert_eq!(load(no_envrc.path(), &tools(no_envrc.path(), "direnv")).notice, None);
    }

    #[test]
    fn a_failing_shell_is_a_notice_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let broken = script(dir.path(), "bash", "echo oops >&2\nexit 1");
        let mut t = tools(dir.path(), "direnv");
        t.shell = Some(broken);
        let env = load(dir.path(), &t);
        assert!(env.set.is_empty() && env.notice.unwrap().contains("Could not load"));
        t.shell = None;
        assert!(load(dir.path(), &t).notice.unwrap().contains("SHELL is not set"));
    }

    /// A shell that counts how many times it ran, in `runs`.
    fn counting_tools(dir: &Path) -> (Tools, PathBuf) {
        let runs = dir.join("runs");
        let body = format!("echo run >> '{}'\nFROM_SHELL=yes exec /bin/sh -c \"$4\"", runs.display());
        let mut t = tools(dir, "direnv");
        t.shell = Some(script(dir, "sh", &body));
        (t, runs)
    }

    fn runs(file: &Path) -> usize {
        std::fs::read_to_string(file).map_or(0, |t| t.lines().count())
    }

    #[test]
    fn one_run_serves_every_caller_and_an_edited_envrc_runs_again() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let (tools, runs_file) = counting_tools(&root);
        assert!(cached(&root).is_none(), "nothing is started by looking");

        let handles: Vec<_> = (0..4)
            .map(|_| {
                let (root, tools) = (root.clone(), tools.clone());
                thread::spawn(move || env_for_with(&root, &tools).unwrap())
            })
            .collect();
        for handle in handles {
            assert_eq!(handle.join().unwrap().get("FROM_SHELL"), Some("yes"));
        }
        assert_eq!(runs(&runs_file), 1, "four workers, one shell");
        assert!(cached(&root).is_some());
        env_for_with(&root, &tools).unwrap();
        assert_eq!(runs(&runs_file), 1);

        // A new .envrc, then an edited one (a later modification time).
        let envrc = root.join(".envrc");
        std::fs::write(&envrc, "a\n").unwrap();
        let direnv = script(&root, "direnv", "echo '{}'");
        let mut with_direnv = tools.clone();
        with_direnv.direnv = direnv.to_str().unwrap().to_owned();
        env_for_with(&root, &with_direnv).unwrap();
        assert_eq!(runs(&runs_file), 2, "a new key");
        let file = std::fs::File::options().write(true).open(&envrc).unwrap();
        file.set_modified(SystemTime::now() + Duration::from_secs(5)).unwrap();
        env_for_with(&root, &with_direnv).unwrap();
        assert_eq!(runs(&runs_file), 3, "the .envrc changed");
    }

    #[test]
    fn programs_are_looked_up_in_the_given_path() {
        let dir = tempfile::tempdir().unwrap();
        script(dir.path(), "rust-analyzer", "true");
        let path = dir.path().to_str().unwrap();
        assert_eq!(which_in("rust-analyzer", Some(path)), Some(dir.path().join("rust-analyzer")));
        assert_eq!(which_in("rust-analyzer", Some("/nonexistent")), None);
        assert_eq!(which_in("/abs/tool", Some("/nonexistent")), Some(PathBuf::from("/abs/tool")));
    }

    /// Runs your own login shell: `cargo test --lib -- --ignored real_login_shell`.
    #[test]
    #[ignore = "starts the login shell of whoever runs it"]
    fn real_login_shell_environment_has_a_path() {
        let dir = tempfile::tempdir().unwrap();
        let started = Instant::now();
        let env = load(dir.path(), &Tools::system());
        println!("shell environment in {:?}: {} variables, notice {:?}", started.elapsed(), env.set.len(), env.notice);
        assert_eq!(env.notice, None);
        assert!(env.path().is_some_and(|p| !p.is_empty()), "PATH: {:?}", env.path());
    }
}
