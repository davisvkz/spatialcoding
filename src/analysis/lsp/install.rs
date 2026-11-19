//! Downloads language servers into a per-user directory (TASK-56).
//!
//! | Server | Source |
//! |---|---|
//! | Rust | `rust-analyzer-<target>.gz` of the latest GitHub release (sha256 checked against the release's `digest`) |
//! | TypeScript | the `typescript-language-server` and `typescript` npm tarballs (sha512 checked) |
//! | Python | the `pyright` npm tarball (sha512 checked) |
//! | Go | `go install golang.org/x/tools/gopls@latest` (gopls has no prebuilt binary) |
//!
//! Everything lands in `<install_dir>/<server>/`. An install is built in
//! `<install_dir>/.tmp-<server>/` and renamed into place, so a failed one
//! leaves nothing behind. [`managed_command`] turns an install into the
//! command to start it; `resolve_command` in `mod.rs` uses it after `PATH`.
//!
//! Network access goes through [`Downloader`], so tests install from a fake.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::ServerId;

/// Newest `typescript-language-server` and `typescript` known to work
/// together (typescript 7 is the native port and has no `tsserver.js`).
const TSLS_VERSION: &str = "6.1.0";
const TYPESCRIPT_VERSION: &str = "6.0.3";
/// Oldest Node.js each npm-based server runs on.
const TSLS_MIN_NODE: (u32, u32, u32) = (22, 22, 2);
const PYRIGHT_MIN_NODE: (u32, u32, u32) = (14, 0, 0);

const RA_RELEASE_API: &str = "https://api.github.com/repos/rust-lang/rust-analyzer/releases/latest";
const RA_LATEST_DOWNLOAD: &str = "https://github.com/rust-lang/rust-analyzer/releases/latest/download";
const NPM_REGISTRY: &str = "https://registry.npmjs.org";

/// Largest download or decompressed binary accepted.
const MAX_DOWNLOAD: u64 = 400 * 1024 * 1024;
const MAX_METADATA: u64 = 8 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Where things live
// ---------------------------------------------------------------------------

/// The directory servers are installed into: `SPATIALCODING_LSP_DIR`, else
/// `$XDG_DATA_HOME/spatialcoding/lsp`, else `~/.local/share/spatialcoding/lsp`.
pub fn install_dir() -> PathBuf {
    install_dir_from(|key| std::env::var_os(key).map(PathBuf::from))
}

fn install_dir_from(env: impl Fn(&str) -> Option<PathBuf>) -> PathBuf {
    if let Some(dir) = env("SPATIALCODING_LSP_DIR").filter(|d| !d.as_os_str().is_empty()) {
        return dir;
    }
    let data = env("XDG_DATA_HOME")
        .filter(|d| d.is_absolute())
        .or_else(|| env("HOME").map(|h| h.join(".local/share")))
        .unwrap_or_else(|| PathBuf::from(".local/share"));
    data.join("spatialcoding").join("lsp")
}

fn slug(server: ServerId) -> &'static str {
    match server {
        ServerId::Rust => "rust",
        ServerId::TypeScript => "typescript",
        ServerId::Python => "python",
        ServerId::Go => "go",
    }
}

/// The command that starts the copy of `server` installed in `dir`, if any.
pub fn managed_command(server: ServerId, dir: &Path) -> Option<Vec<String>> {
    let root = dir.join(slug(server));
    let node = |entry: PathBuf| {
        entry.is_file().then(|| {
            vec!["node".to_string(), entry.to_string_lossy().into_owned(), "--stdio".to_string()]
        })
    };
    match server {
        ServerId::Rust => single_binary(root.join(binary_name("rust-analyzer"))),
        ServerId::Go => single_binary(root.join(binary_name("gopls"))),
        ServerId::TypeScript => {
            node(root.join("node_modules/typescript-language-server/lib/cli.mjs"))
        }
        ServerId::Python => node(root.join("node_modules/pyright/langserver.index.js")),
    }
}

fn single_binary(path: PathBuf) -> Option<Vec<String>> {
    path.is_file().then(|| vec![path.to_string_lossy().into_owned()])
}

fn binary_name(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

// ---------------------------------------------------------------------------
// Platform and what blocks an install
// ---------------------------------------------------------------------------

/// What an install needs to know about the machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Platform {
    /// `std::env::consts::OS` / `ARCH` values.
    pub os: String,
    pub arch: String,
    /// `node --version`, if Node.js is on `PATH`.
    pub node: Option<(u32, u32, u32)>,
    pub has_go: bool,
    /// A musl system (Alpine): it has no glibc loader, so it needs the musl
    /// build of rust-analyzer.
    pub musl: bool,
}

impl Platform {
    pub fn detect() -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            node: detect_node(),
            has_go: super::on_path("go"),
            musl: Path::new("/lib/ld-musl-x86_64.so.1").exists()
                || Path::new("/lib/ld-musl-aarch64.so.1").exists(),
        }
    }
}

fn detect_node() -> Option<(u32, u32, u32)> {
    let out = std::process::Command::new("node").arg("--version").output().ok()?;
    out.status.success().then(|| parse_node_version(&String::from_utf8_lossy(&out.stdout)))?
}

/// `v22.22.2` → `(22, 22, 2)`.
pub fn parse_node_version(text: &str) -> Option<(u32, u32, u32)> {
    let mut parts = text.trim().trim_start_matches('v').split('.');
    let mut next = || parts.next()?.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok();
    Some((next()?, next()?, next()?))
}

fn fmt_version((a, b, c): (u32, u32, u32)) -> String {
    format!("{a}.{b}.{c}")
}

/// The release asset of rust-analyzer for a platform. The "musl" build is not
/// static: it needs the musl loader, so it is only for musl systems.
pub fn rust_asset_name(os: &str, arch: &str, musl: bool) -> Option<&'static str> {
    Some(match (os, arch, musl) {
        ("linux", "x86_64", true) => "rust-analyzer-x86_64-unknown-linux-musl.gz",
        ("linux", "x86_64", false) => "rust-analyzer-x86_64-unknown-linux-gnu.gz",
        ("linux", "aarch64", _) => "rust-analyzer-aarch64-unknown-linux-gnu.gz",
        ("macos", "x86_64", _) => "rust-analyzer-x86_64-apple-darwin.gz",
        ("macos", "aarch64", _) => "rust-analyzer-aarch64-apple-darwin.gz",
        _ => return None,
    })
}

/// Why `server` cannot be installed here, or `None` if it can.
pub fn blocker(server: ServerId, platform: &Platform) -> Option<String> {
    if platform.os == "windows" {
        return Some("Automatic install isn't available on this platform.".into());
    }
    let needs_node = |min: (u32, u32, u32)| match platform.node {
        None => Some(format!("Needs Node.js {} or newer on PATH.", fmt_version(min))),
        Some(found) if found < min => Some(format!(
            "Needs Node.js {} or newer (found {}).",
            fmt_version(min),
            fmt_version(found)
        )),
        Some(_) => None,
    };
    match server {
        ServerId::Rust => rust_asset_name(&platform.os, &platform.arch, platform.musl)
            .is_none()
            .then(|| format!("No prebuilt rust-analyzer for {}-{}.", platform.os, platform.arch)),
        ServerId::TypeScript => needs_node(TSLS_MIN_NODE),
        ServerId::Python => needs_node(PYRIGHT_MIN_NODE),
        ServerId::Go => (!platform.has_go)
            .then(|| "Needs the go toolchain on PATH (gopls is built with `go install`).".into()),
    }
}

// ---------------------------------------------------------------------------
// Network
// ---------------------------------------------------------------------------

/// A response body on its way.
pub struct Download {
    pub reader: Box<dyn Read + Send>,
    pub len: Option<u64>,
}

/// Fetches URLs. [`UreqDownloader`] is the real one.
pub trait Downloader: Send + Sync {
    fn get(&self, url: &str) -> Result<Download, String>;
}

pub struct UreqDownloader {
    agent: ureq::Agent,
}

impl UreqDownloader {
    pub fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .user_agent("spatialcoding-lsp-installer")
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .timeout_per_read(Some(Duration::from_secs(30)))
            .build();
        Self { agent: ureq::Agent::new_with_config(config) }
    }
}

impl Default for UreqDownloader {
    fn default() -> Self {
        Self::new()
    }
}

impl Downloader for UreqDownloader {
    fn get(&self, url: &str) -> Result<Download, String> {
        let response = self.agent.get(url).call().map_err(|e| format!("{url}: {e}"))?;
        let (_, body) = response.into_parts();
        let len = body.content_length();
        Ok(Download { reader: Box::new(body.into_reader()), len })
    }
}

/// Reads a whole download, reporting `(received, total)` as it goes.
fn read_all(
    download: Download,
    limit: u64,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<Vec<u8>, String> {
    if download.len.is_some_and(|n| n > limit) {
        return Err("The download is larger than allowed.".into());
    }
    let total = download.len;
    let mut reader = download.reader;
    let mut bytes = Vec::with_capacity(total.unwrap_or(0).min(limit) as usize);
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut chunk).map_err(|e| format!("Download failed: {e}"))?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..n]);
        if bytes.len() as u64 > limit {
            return Err("The download is larger than allowed.".into());
        }
        progress(bytes.len() as u64, total);
    }
    Ok(bytes)
}

// ---------------------------------------------------------------------------
// Integrity and archives (pure)
// ---------------------------------------------------------------------------

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Checks `bytes` against an npm Subresource Integrity string
/// (`sha512-<base64>`, several may be space separated; one match is enough).
pub fn verify_integrity(bytes: &[u8], sri: &str) -> Result<(), String> {
    use base64::Engine as _;
    use sha2::Digest as _;
    let mut known = false;
    for item in sri.split_whitespace() {
        let Some((algo, expected)) = item.split_once('-') else { continue };
        let expected = expected.split('?').next().unwrap_or(expected);
        let actual = match algo {
            "sha512" => sha2::Sha512::digest(bytes).to_vec(),
            "sha256" => sha2::Sha256::digest(bytes).to_vec(),
            _ => continue,
        };
        known = true;
        if base64::engine::general_purpose::STANDARD.encode(actual) == expected {
            return Ok(());
        }
    }
    Err(if known {
        "The downloaded package does not match its published checksum.".into()
    } else {
        "The registry gave no supported checksum for the package.".into()
    })
}

/// Checks `bytes` against a hex SHA-256 (`sha256:<hex>` or plain hex).
pub fn verify_sha256(bytes: &[u8], expected: &str) -> Result<(), String> {
    use sha2::Digest as _;
    let expected = expected.strip_prefix("sha256:").unwrap_or(expected).trim();
    if hex(&sha2::Sha256::digest(bytes)).eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err("The downloaded file does not match its published checksum.".into())
    }
}

/// Unpacks an npm tarball into `dest`, dropping its leading `package/`
/// directory. Entries that would land outside `dest` (`..`, absolute paths)
/// and links are refused.
pub fn extract_npm_tgz(bytes: &[u8], dest: &Path) -> Result<(), String> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
    std::fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let entries = archive.entries().map_err(|e| format!("Bad package archive: {e}"))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| format!("Bad package archive: {e}"))?;
        let kind = entry.header().entry_type();
        let path = entry.path().map_err(|e| format!("Bad package archive: {e}"))?.into_owned();
        let mut components = path.components();
        components.next(); // `package/`
        let relative: PathBuf = components.collect();
        if relative.as_os_str().is_empty() {
            continue;
        }
        if relative.components().any(|c| !matches!(c, Component::Normal(_))) {
            return Err(format!("The package has an unsafe path: {}", path.display()));
        }
        let target = dest.join(&relative);
        if kind.is_dir() {
            std::fs::create_dir_all(&target).map_err(|e| format!("{}: {e}", target.display()))?;
        } else if kind.is_file() {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            entry.unpack(&target).map_err(|e| format!("{}: {e}", target.display()))?;
        } else if kind.is_symlink() || kind.is_hard_link() {
            return Err(format!("The package has a link: {}", path.display()));
        }
    }
    Ok(())
}

/// Decompresses a `.gz` file into `dest`.
pub fn gunzip_to(bytes: &[u8], dest: &Path) -> Result<(), String> {
    let mut decoder = flate2::read::GzDecoder::new(bytes).take(MAX_DOWNLOAD + 1);
    let mut file = std::fs::File::create(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let written = std::io::copy(&mut decoder, &mut file).map_err(|e| format!("Bad archive: {e}"))?;
    if written > MAX_DOWNLOAD {
        return Err("The unpacked file is larger than allowed.".into());
    }
    Ok(())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> Result<(), String> {
    Ok(())
}

// ---------------------------------------------------------------------------
// One install
// ---------------------------------------------------------------------------

/// Progress of an install, as the dialog shows it.
#[derive(Debug, Clone, PartialEq)]
pub enum InstallState {
    Downloading { received: u64, total: Option<u64> },
    Extracting,
    Building,
    Failed(String),
}

type States = Arc<Mutex<HashMap<ServerId, InstallState>>>;
/// Installs that ended, waiting for the UI to take them.
type Finished = Arc<Mutex<Vec<(ServerId, Result<(), String>)>>>;

struct Job<'a> {
    downloader: &'a dyn Downloader,
    platform: &'a Platform,
    report: &'a dyn Fn(InstallState),
}

impl Job<'_> {
    fn fetch(&self, url: &str, limit: u64) -> Result<Vec<u8>, String> {
        (self.report)(InstallState::Downloading { received: 0, total: None });
        let download = self.downloader.get(url)?;
        let mut last = 0u64;
        read_all(download, limit, &mut |received, total| {
            // Every ~128 KiB is plenty for a progress bar.
            if received - last >= 128 * 1024 || Some(received) == total {
                last = received;
                (self.report)(InstallState::Downloading { received, total });
            }
        })
    }

    fn fetch_json(&self, url: &str) -> Result<serde_json::Value, String> {
        let bytes = self.fetch(url, MAX_METADATA)?;
        serde_json::from_slice(&bytes).map_err(|e| format!("{url}: unexpected answer ({e})"))
    }
}

fn install_rust(job: &Job, tmp: &Path) -> Result<String, String> {
    let asset = rust_asset_name(&job.platform.os, &job.platform.arch, job.platform.musl)
        .ok_or_else(|| format!("No prebuilt rust-analyzer for {}-{}.", job.platform.os, job.platform.arch))?;
    // The release API names the exact tag and the asset's checksum. Without
    // it (rate limit, offline mirror) the `latest` download still works,
    // just without a checksum.
    let (url, digest, version) = match release_asset(job, asset) {
        Ok(found) => found,
        Err(_) => (format!("{RA_LATEST_DOWNLOAD}/{asset}"), None, "latest".to_string()),
    };
    let bytes = job.fetch(&url, MAX_DOWNLOAD)?;
    if let Some(digest) = digest {
        verify_sha256(&bytes, &digest)?;
    }
    (job.report)(InstallState::Extracting);
    let binary = tmp.join(binary_name("rust-analyzer"));
    gunzip_to(&bytes, &binary)?;
    make_executable(&binary)?;
    Ok(version)
}

/// `(download url, sha256 digest, tag)` of `asset` in the latest release.
fn release_asset(job: &Job, asset: &str) -> Result<(String, Option<String>, String), String> {
    let release = job.fetch_json(RA_RELEASE_API)?;
    let found = release["assets"]
        .as_array()
        .and_then(|assets| assets.iter().find(|a| a["name"] == asset))
        .ok_or_else(|| format!("The latest release has no {asset}."))?;
    let url = found["browser_download_url"]
        .as_str()
        .ok_or("The release asset has no download url.")?
        .to_string();
    let digest = found["digest"].as_str().map(String::from);
    let tag = release["tag_name"].as_str().unwrap_or("latest").to_string();
    Ok((url, digest, tag))
}

/// Installs `name@version` from the npm registry into `<tmp>/node_modules/<name>`.
fn install_npm(job: &Job, tmp: &Path, name: &str, version: &str) -> Result<String, String> {
    let meta = job.fetch_json(&format!("{NPM_REGISTRY}/{name}/{version}"))?;
    let tarball = meta["dist"]["tarball"].as_str().ok_or_else(|| format!("{name}: the registry gave no tarball."))?;
    if !tarball.starts_with("https://") {
        return Err(format!("{name}: refusing a non-https tarball url."));
    }
    let integrity = meta["dist"]["integrity"]
        .as_str()
        .ok_or_else(|| format!("{name}: the registry gave no checksum."))?;
    let bytes = job.fetch(tarball, MAX_DOWNLOAD)?;
    verify_integrity(&bytes, integrity)?;
    (job.report)(InstallState::Extracting);
    extract_npm_tgz(&bytes, &tmp.join("node_modules").join(name))?;
    Ok(meta["version"].as_str().unwrap_or(version).to_string())
}

fn install_go(job: &Job, tmp: &Path) -> Result<String, String> {
    (job.report)(InstallState::Building);
    let output = std::process::Command::new("go")
        .args(["install", "golang.org/x/tools/gopls@latest"])
        .env("GOBIN", tmp)
        .current_dir(tmp)
        .output()
        .map_err(|e| format!("Could not run `go`: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: Vec<&str> = stderr.lines().rev().take(4).collect();
        let tail: Vec<&str> = tail.into_iter().rev().collect();
        return Err(format!("`go install gopls` failed: {}", tail.join(" ")));
    }
    Ok("latest".into())
}

/// Installs `server` into `dir`: builds it in a scratch directory and renames
/// it into place. Nothing is left behind on failure.
fn run_install(
    server: ServerId,
    dir: &Path,
    downloader: &dyn Downloader,
    platform: &Platform,
    report: &dyn Fn(InstallState),
) -> Result<(), String> {
    if let Some(why) = blocker(server, platform) {
        return Err(why);
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let dir = dir.canonicalize().map_err(|e| format!("{}: {e}", dir.display()))?;
    let tmp = dir.join(format!(".tmp-{}", slug(server)));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;

    let job = Job { downloader, platform, report };
    let built = match server {
        ServerId::Rust => install_rust(&job, &tmp),
        ServerId::TypeScript => install_npm(&job, &tmp, "typescript-language-server", TSLS_VERSION)
            .and_then(|version| install_npm(&job, &tmp, "typescript", TYPESCRIPT_VERSION).map(|_| version)),
        ServerId::Python => install_npm(&job, &tmp, "pyright", "latest"),
        ServerId::Go => install_go(&job, &tmp),
    };
    let committed = built.and_then(|version| {
        let meta = serde_json::json!({
            "server": slug(server),
            "version": version,
            "installed_at": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        });
        std::fs::write(tmp.join("install.json"), meta.to_string())
            .map_err(|e| format!("install.json: {e}"))?;
        let dest = dir.join(slug(server));
        let _ = std::fs::remove_dir_all(&dest);
        std::fs::rename(&tmp, &dest).map_err(|e| format!("{}: {e}", dest.display()))?;
        if managed_command(server, &dir).is_none() {
            let _ = std::fs::remove_dir_all(&dest);
            return Err(format!("The {} download did not contain the language server.", server.name()));
        }
        Ok(())
    });
    if committed.is_err() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    committed
}

// ---------------------------------------------------------------------------
// The installer the UI holds
// ---------------------------------------------------------------------------

/// How `PATH` and the environment are looked at; tests replace it.
#[derive(Clone, Copy)]
pub struct Probe {
    pub env: fn(&str) -> Option<String>,
    pub exists: fn(&str) -> bool,
}

impl Probe {
    pub fn system() -> Self {
        Self { env: |key| std::env::var(key).ok(), exists: super::on_path }
    }
}

/// Runs installs on worker threads and tracks their progress.
pub struct Installer {
    dir: PathBuf,
    downloader: Arc<dyn Downloader>,
    platform: Option<Platform>,
    probe: Probe,
    states: States,
    finished: Finished,
}

impl Default for Installer {
    fn default() -> Self {
        Self::with(install_dir(), Arc::new(UreqDownloader::new()), None, Probe::system())
    }
}

impl Installer {
    /// `platform` is detected on first use when `None`.
    pub fn with(
        dir: PathBuf,
        downloader: Arc<dyn Downloader>,
        platform: Option<Platform>,
        probe: Probe,
    ) -> Self {
        Self {
            dir,
            downloader,
            platform,
            probe,
            states: Default::default(),
            finished: Default::default(),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Where `server` comes from now: the environment, `PATH`, a managed
    /// install, or nowhere.
    pub fn availability(&self, server: ServerId) -> super::Availability {
        server.availability_with(&self.dir, self.probe.env, self.probe.exists)
    }

    /// The machine's facts, detected once (it runs `node --version`).
    pub fn platform(&mut self) -> &Platform {
        self.platform.get_or_insert_with(Platform::detect)
    }

    /// Looks again for Node.js and Go (after the user installed one).
    pub fn refresh_platform(&mut self) {
        self.platform = None;
    }

    pub fn blocker(&mut self, server: ServerId) -> Option<String> {
        let platform = self.platform().clone();
        blocker(server, &platform)
    }

    pub fn state(&self, server: ServerId) -> Option<InstallState> {
        self.states.lock().ok()?.get(&server).cloned()
    }

    pub fn is_busy(&self, server: ServerId) -> bool {
        matches!(
            self.state(server),
            Some(InstallState::Downloading { .. } | InstallState::Extracting | InstallState::Building)
        )
    }

    /// Starts installing `server` on a worker thread; a repaint is requested
    /// on `ctx` as it progresses. Does nothing if it is already running.
    pub fn install(&mut self, server: ServerId, ctx: Option<egui::Context>) {
        if self.is_busy(server) {
            return;
        }
        let platform = self.platform().clone();
        let (dir, downloader) = (self.dir.clone(), self.downloader.clone());
        let (states, finished) = (self.states.clone(), self.finished.clone());
        set_state(&states, server, Some(InstallState::Downloading { received: 0, total: None }));
        let spawned = std::thread::Builder::new()
            .name(format!("lsp-install-{}", slug(server)))
            .spawn({
                let (states, finished) = (states.clone(), finished.clone());
                move || {
                    let repaint = || {
                        if let Some(ctx) = &ctx {
                            ctx.request_repaint();
                        }
                    };
                    let report = |state: InstallState| {
                        set_state(&states, server, Some(state));
                        repaint();
                    };
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        run_install(server, &dir, downloader.as_ref(), &platform, &report)
                    }))
                    .unwrap_or_else(|_| Err("The installer crashed.".into()));
                    match &result {
                        Ok(()) => set_state(&states, server, None),
                        Err(e) => set_state(&states, server, Some(InstallState::Failed(e.clone()))),
                    }
                    if let Ok(mut done) = finished.lock() {
                        done.push((server, result));
                    }
                    repaint();
                }
            });
        if let Err(e) = spawned {
            let message = format!("Could not start the installer: {e}");
            set_state(&states, server, Some(InstallState::Failed(message.clone())));
            if let Ok(mut done) = finished.lock() {
                done.push((server, Err(message)));
            }
        }
    }

    /// Installs that ended since the last call, in order.
    pub fn take_finished(&mut self) -> Vec<(ServerId, Result<(), String>)> {
        self.finished.lock().map(|mut f| std::mem::take(&mut *f)).unwrap_or_default()
    }

    /// Deletes the managed copy of `server` (never touches `PATH`).
    pub fn remove(&mut self, server: ServerId) -> Result<(), String> {
        set_state(&self.states, server, None);
        let target = self.dir.join(slug(server));
        if !target.exists() {
            return Ok(());
        }
        std::fs::remove_dir_all(&target).map_err(|e| format!("{}: {e}", target.display()))
    }
}

fn set_state(states: &States, server: ServerId, state: Option<InstallState>) {
    if let Ok(mut map) = states.lock() {
        match state {
            Some(state) => map.insert(server, state),
            None => map.remove(&server),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Serves canned bodies by URL and records what was asked.
    struct Fake {
        bodies: HashMap<String, Vec<u8>>,
        asked: Mutex<Vec<String>>,
    }

    impl Fake {
        fn new(bodies: impl IntoIterator<Item = (String, Vec<u8>)>) -> Arc<Self> {
            Arc::new(Self { bodies: bodies.into_iter().collect(), asked: Default::default() })
        }
    }

    impl Downloader for Fake {
        fn get(&self, url: &str) -> Result<Download, String> {
            self.asked.lock().unwrap().push(url.to_string());
            let body = self.bodies.get(url).ok_or_else(|| format!("{url}: 404"))?.clone();
            Ok(Download { len: Some(body.len() as u64), reader: Box::new(Cursor::new(body)) })
        }
    }

    fn gz(bytes: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(bytes).unwrap();
        enc.finish().unwrap()
    }

    /// An npm-style tarball: entries under `package/`.
    fn npm_tgz(files: &[(&str, &str)]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (path, content) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, format!("package/{path}"), content.as_bytes()).unwrap();
        }
        gz(&builder.into_inner().unwrap())
    }

    fn sri(bytes: &[u8]) -> String {
        use base64::Engine as _;
        use sha2::Digest as _;
        format!("sha512-{}", base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(bytes)))
    }

    fn sha256(bytes: &[u8]) -> String {
        use sha2::Digest as _;
        format!("sha256:{}", hex(&sha2::Sha256::digest(bytes)))
    }

    fn npm_meta(name: &str, version: &str, tarball: &[u8]) -> (String, Vec<u8>) {
        npm_meta_at(name, version, version, tarball)
    }

    /// The registry's answer for `name@spec`, which resolves to `version`.
    fn npm_meta_at(name: &str, spec: &str, version: &str, tarball: &[u8]) -> (String, Vec<u8>) {
        let json = serde_json::json!({
            "version": version,
            "dist": {
                "tarball": format!("https://registry.npmjs.org/{name}/-/{name}-{version}.tgz"),
                "integrity": sri(tarball),
            },
        });
        (format!("{NPM_REGISTRY}/{name}/{spec}"), json.to_string().into_bytes())
    }

    fn linux() -> Platform {
        Platform { os: "linux".into(), arch: "x86_64".into(), node: Some((24, 0, 0)), has_go: false, musl: false }
    }

    fn quiet(_: InstallState) {}

    #[test]
    fn install_dir_prefers_the_override_then_xdg_then_home() {
        let env = |vars: &'static [(&'static str, &'static str)]| {
            move |key: &str| vars.iter().find(|(k, _)| *k == key).map(|(_, v)| PathBuf::from(v))
        };
        assert_eq!(install_dir_from(env(&[("SPATIALCODING_LSP_DIR", "/x"), ("HOME", "/h")])), PathBuf::from("/x"));
        assert_eq!(
            install_dir_from(env(&[("XDG_DATA_HOME", "/data"), ("HOME", "/h")])),
            PathBuf::from("/data/spatialcoding/lsp")
        );
        assert_eq!(
            install_dir_from(env(&[("HOME", "/h")])),
            PathBuf::from("/h/.local/share/spatialcoding/lsp")
        );
    }

    #[test]
    fn node_versions_parse() {
        assert_eq!(parse_node_version("v22.22.2\n"), Some((22, 22, 2)));
        assert_eq!(parse_node_version("v25.0.0-nightly2026"), Some((25, 0, 0)));
        assert_eq!(parse_node_version("nope"), None);
    }

    #[test]
    fn rust_assets_by_platform() {
        assert_eq!(rust_asset_name("linux", "x86_64", false), Some("rust-analyzer-x86_64-unknown-linux-gnu.gz"));
        assert_eq!(rust_asset_name("linux", "x86_64", true), Some("rust-analyzer-x86_64-unknown-linux-musl.gz"));
        assert_eq!(rust_asset_name("macos", "aarch64", false), Some("rust-analyzer-aarch64-apple-darwin.gz"));
        assert_eq!(rust_asset_name("windows", "x86_64", false), None);
        assert_eq!(rust_asset_name("freebsd", "x86_64", false), None);
    }

    #[test]
    fn blockers_explain_what_is_missing() {
        let mut p = linux();
        assert_eq!(blocker(ServerId::Rust, &p), None);
        assert_eq!(blocker(ServerId::Python, &p), None);
        assert!(blocker(ServerId::Go, &p).unwrap().contains("go toolchain"));
        p.node = Some((20, 1, 0));
        assert!(blocker(ServerId::TypeScript, &p).unwrap().contains("22.22.2 or newer (found 20.1.0)"));
        assert_eq!(blocker(ServerId::Python, &p), None);
        p.node = None;
        assert!(blocker(ServerId::Python, &p).unwrap().contains("Needs Node.js 14.0.0"));
        p.os = "windows".into();
        assert!(blocker(ServerId::Rust, &p).unwrap().contains("isn't available"));
    }

    #[test]
    fn integrity_accepts_a_match_and_rejects_a_mismatch() {
        let data = b"hello";
        assert!(verify_integrity(data, &sri(data)).is_ok());
        assert!(verify_integrity(data, &format!("sha1-abc {}", sri(data))).is_ok());
        assert!(verify_integrity(b"other", &sri(data)).unwrap_err().contains("does not match"));
        assert!(verify_integrity(data, "md5-abc").unwrap_err().contains("no supported checksum"));
        assert!(verify_sha256(data, &sha256(data)).is_ok());
        assert!(verify_sha256(b"other", &sha256(data)).is_err());
    }

    #[test]
    fn npm_archives_unpack_without_their_package_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let tgz = npm_tgz(&[("package.json", "{}"), ("lib/cli.mjs", "x")]);
        extract_npm_tgz(&tgz, &dir.path().join("out")).unwrap();
        assert!(dir.path().join("out/package.json").is_file());
        assert_eq!(std::fs::read_to_string(dir.path().join("out/lib/cli.mjs")).unwrap(), "x");
    }

    #[test]
    fn npm_archives_with_paths_outside_the_target_are_refused() {
        // `tar::Builder` refuses `..`, so write the header bytes by hand.
        let mut raw = vec![0u8; 512];
        let name = b"package/../evil.txt";
        raw[..name.len()].copy_from_slice(name);
        raw[100..107].copy_from_slice(b"0000644");
        raw[108..115].copy_from_slice(b"0000000");
        raw[116..123].copy_from_slice(b"0000000");
        raw[124..135].copy_from_slice(b"00000000001");
        raw[136..147].copy_from_slice(b"00000000000");
        raw[148..156].copy_from_slice(b"        ");
        raw[156] = b'0';
        raw[257..262].copy_from_slice(b"ustar");
        let sum: u32 = raw.iter().map(|b| *b as u32).sum();
        raw[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
        raw.extend_from_slice(b"x");
        raw.resize(1024, 0);
        raw.extend_from_slice(&[0u8; 1024]);
        let dir = tempfile::tempdir().unwrap();
        let err = extract_npm_tgz(&gz(&raw), &dir.path().join("out")).unwrap_err();
        assert!(err.contains("unsafe path"), "{err}");
        assert!(!dir.path().join("evil.txt").exists());
    }

    #[test]
    fn rust_analyzer_installs_from_the_release_and_checks_its_digest() {
        let binary = b"#!/bin/sh\necho rust-analyzer\n";
        let asset = "rust-analyzer-x86_64-unknown-linux-gnu.gz";
        let url = format!("https://github.com/rust-lang/rust-analyzer/releases/download/2026-01-01/{asset}");
        let release = serde_json::json!({
            "tag_name": "2026-01-01",
            "assets": [{ "name": asset, "browser_download_url": url, "digest": sha256(&gz(binary)) }],
        });
        let fake = Fake::new([
            (RA_RELEASE_API.to_string(), release.to_string().into_bytes()),
            (url, gz(binary)),
        ]);
        let dir = tempfile::tempdir().unwrap();
        run_install(ServerId::Rust, dir.path(), fake.as_ref(), &linux(), &quiet).unwrap();
        let cmd = managed_command(ServerId::Rust, dir.path()).unwrap();
        assert!(cmd[0].ends_with("rust/rust-analyzer"), "{cmd:?}");
        assert_eq!(std::fs::read(&cmd[0]).unwrap(), binary);
        let meta = std::fs::read_to_string(dir.path().join("rust/install.json")).unwrap();
        assert!(meta.contains("2026-01-01"));
        assert!(!dir.path().join(".tmp-rust").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&cmd[0]).unwrap().permissions().mode() & 0o111, 0o111);
        }
    }

    #[test]
    fn a_wrong_digest_leaves_nothing_installed() {
        let asset = "rust-analyzer-x86_64-unknown-linux-gnu.gz";
        let url = format!("https://example.test/{asset}");
        let release = serde_json::json!({
            "tag_name": "t",
            "assets": [{ "name": asset, "browser_download_url": url, "digest": sha256(b"something else") }],
        });
        let fake = Fake::new([
            (RA_RELEASE_API.to_string(), release.to_string().into_bytes()),
            (url, gz(b"binary")),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let err = run_install(ServerId::Rust, dir.path(), fake.as_ref(), &linux(), &quiet).unwrap_err();
        assert!(err.contains("does not match"), "{err}");
        assert!(managed_command(ServerId::Rust, dir.path()).is_none());
        assert!(!dir.path().join(".tmp-rust").exists());
        assert!(!dir.path().join("rust").exists());
    }

    #[test]
    fn rust_analyzer_falls_back_to_the_latest_download_when_the_api_is_down() {
        let asset = "rust-analyzer-x86_64-unknown-linux-gnu.gz";
        let fake = Fake::new([(format!("{RA_LATEST_DOWNLOAD}/{asset}"), gz(b"bin"))]);
        let dir = tempfile::tempdir().unwrap();
        run_install(ServerId::Rust, dir.path(), fake.as_ref(), &linux(), &quiet).unwrap();
        assert!(managed_command(ServerId::Rust, dir.path()).is_some());
    }

    #[test]
    fn pyright_installs_from_npm_and_starts_through_node() {
        let tgz = npm_tgz(&[("package.json", "{}"), ("langserver.index.js", "//")]);
        let (meta_url, meta) = npm_meta_at("pyright", "latest", "1.2.3", &tgz);
        let tarball_url = "https://registry.npmjs.org/pyright/-/pyright-1.2.3.tgz".to_string();
        let fake = Fake::new([(meta_url, meta), (tarball_url, tgz)]);
        let dir = tempfile::tempdir().unwrap();
        run_install(ServerId::Python, dir.path(), fake.as_ref(), &linux(), &quiet).unwrap();
        let cmd = managed_command(ServerId::Python, dir.path()).unwrap();
        assert_eq!(cmd[0], "node");
        assert!(cmd[1].ends_with("python/node_modules/pyright/langserver.index.js"), "{cmd:?}");
        assert_eq!(cmd[2], "--stdio");
        assert!(std::fs::read_to_string(dir.path().join("python/install.json")).unwrap().contains("1.2.3"));
    }

    #[test]
    fn a_tampered_package_is_not_installed_and_keeps_the_previous_copy() {
        let good = npm_tgz(&[("langserver.index.js", "//")]);
        let (meta_url, meta) = npm_meta_at("pyright", "latest", "1.0.0", &good);
        let tarball_url = "https://registry.npmjs.org/pyright/-/pyright-1.0.0.tgz".to_string();
        let fake = Fake::new([(meta_url, meta), (tarball_url, npm_tgz(&[("langserver.index.js", "evil")]))]);
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("python/node_modules/pyright");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("langserver.index.js"), "old").unwrap();
        let err = run_install(ServerId::Python, dir.path(), fake.as_ref(), &linux(), &quiet).unwrap_err();
        assert!(err.contains("does not match"), "{err}");
        assert_eq!(std::fs::read_to_string(old.join("langserver.index.js")).unwrap(), "old");
    }

    #[test]
    fn typescript_needs_both_packages() {
        let tsls = npm_tgz(&[("lib/cli.mjs", "//")]);
        let ts = npm_tgz(&[("lib/tsserver.js", "//")]);
        let (m1, b1) = npm_meta("typescript-language-server", TSLS_VERSION, &tsls);
        let (m2, b2) = npm_meta("typescript", TYPESCRIPT_VERSION, &ts);
        let fake = Fake::new([
            (m1, b1),
            (format!("https://registry.npmjs.org/typescript-language-server/-/typescript-language-server-{TSLS_VERSION}.tgz"), tsls),
            (m2, b2),
            (format!("https://registry.npmjs.org/typescript/-/typescript-{TYPESCRIPT_VERSION}.tgz"), ts),
        ]);
        let dir = tempfile::tempdir().unwrap();
        run_install(ServerId::TypeScript, dir.path(), fake.as_ref(), &linux(), &quiet).unwrap();
        let cmd = managed_command(ServerId::TypeScript, dir.path()).unwrap();
        assert!(cmd[1].ends_with("typescript/node_modules/typescript-language-server/lib/cli.mjs"));
        assert!(dir.path().join("typescript/node_modules/typescript/lib/tsserver.js").is_file());
    }

    #[test]
    fn an_install_that_is_blocked_does_not_touch_the_network() {
        let fake = Fake::new([]);
        let dir = tempfile::tempdir().unwrap();
        let mut platform = linux();
        platform.node = None;
        let err = run_install(ServerId::Python, dir.path(), fake.as_ref(), &platform, &quiet).unwrap_err();
        assert!(err.contains("Node.js"), "{err}");
        assert!(fake.asked.lock().unwrap().is_empty());
        assert!(!dir.path().join(".tmp-python").exists());
    }

    #[test]
    fn the_installer_reports_progress_and_the_end_of_each_install() {
        let tgz = npm_tgz(&[("langserver.index.js", "//")]);
        let (meta_url, meta) = npm_meta_at("pyright", "latest", "1.0.0", &tgz);
        let fake = Fake::new([
            (meta_url, meta),
            ("https://registry.npmjs.org/pyright/-/pyright-1.0.0.tgz".to_string(), tgz),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let mut installer = Installer::with(
            dir.path().to_path_buf(),
            fake,
            Some(linux()),
            Probe { env: |_| None, exists: |_| false },
        );
        assert_eq!(installer.availability(ServerId::Python), crate::analysis::lsp::Availability::Missing);
        installer.install(ServerId::Python, None);
        let finished = loop {
            let done = installer.take_finished();
            if !done.is_empty() {
                break done;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(finished, vec![(ServerId::Python, Ok(()))]);
        assert_eq!(installer.state(ServerId::Python), None);
        assert_eq!(installer.availability(ServerId::Python), crate::analysis::lsp::Availability::Managed);
        installer.remove(ServerId::Python).unwrap();
        assert_eq!(installer.availability(ServerId::Python), crate::analysis::lsp::Availability::Missing);
    }

    #[test]
    fn a_failed_install_stays_failed_until_retried() {
        let dir = tempfile::tempdir().unwrap();
        let mut installer = Installer::with(
            dir.path().to_path_buf(),
            Fake::new([]),
            Some(linux()),
            Probe { env: |_| None, exists: |_| false },
        );
        installer.install(ServerId::Python, None);
        let finished = loop {
            let done = installer.take_finished();
            if !done.is_empty() {
                break done;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(finished[0].1.is_err());
        assert!(matches!(installer.state(ServerId::Python), Some(InstallState::Failed(_))));
        assert!(!installer.is_busy(ServerId::Python));
    }

    /// Downloads the real servers. Needs network, `node`, `go` and a few hundred MB:
    /// `cargo test --lib -- --ignored real_download`.
    #[test]
    #[ignore]
    fn real_download() {
        let dir = tempfile::tempdir().unwrap();
        let downloader = UreqDownloader::new();
        let platform = Platform::detect();
        for server in [ServerId::Rust, ServerId::Python, ServerId::TypeScript, ServerId::Go] {
            if let Some(why) = blocker(server, &platform) {
                eprintln!("skipping {}: {why}", server.name());
                continue;
            }
            run_install(server, dir.path(), &downloader, &platform, &quiet).unwrap();
            let cmd = managed_command(server, dir.path()).unwrap();
            eprintln!("{}: {cmd:?}", server.name());
            let mut command = std::process::Command::new(&cmd[0]);
            match server {
                ServerId::Rust => {
                    command.arg("--version");
                }
                ServerId::Go => {
                    command.arg("version");
                }
                ServerId::TypeScript => {
                    command.arg(&cmd[1]).arg("--version");
                }
                _ => continue, // pyright's server has no --version; the entry file exists.
            }
            let status = command.status().unwrap();
            assert!(status.success(), "{} --version failed", server.name());
        }
    }
}
