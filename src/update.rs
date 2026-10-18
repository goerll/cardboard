//! Self-update for installs made by `site/install.sh` / `site/install.ps1`.
//!
//! The pattern VS Code and Chrome use: check quietly, download in the background, verify, and
//! swap the install on the next restart, never mid-session.
//!
//! 1. `check`: ask GitHub for the latest release and pick `cardboard-<os>-<arch>.<ext>` plus its
//!    `.minisig`. No signature yet (the release job signs after every platform is built) means
//!    "not available yet".
//! 2. `stage`: download, verify the minisign signature against the key compiled in from
//!    `packaging/update.pub`, check the trusted comment names this file and version, extract
//!    with the system `tar`/`ditto`, check the layout and that the new CLI reports the version,
//!    then move it next to the install (`.<name>.new`).
//! 3. `apply`: with the app closed, rename the install to `.<name>.old`, the staged copy into
//!    place, roll back if that fails, then delete the old copy. The desktop app hands this to a
//!    copy of the CLI (`cardboard update --apply --root R --wait-pid P [--relaunch]`) because a
//!    running program can't replace its own folder on Windows.
//!
//! Dev builds (anything not in an installer-managed folder) never update themselves.
mod os;

use minisign_verify::{PublicKey, Signature};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// The release signing key. Only this key is trusted by release builds.
pub const PUBLIC_KEY: &str = include_str!("../packaging/update.pub");
const LATEST: &str = "https://api.github.com/repos/goerll/cardboard/releases/latest";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
/// The app checks at launch unless it checked this recently, then once a day.
const MIN_GAP: Duration = Duration::from_secs(4 * 3600);
const CHECK_EVERY: Duration = Duration::from_secs(24 * 3600);
const RETRY_AFTER: Duration = Duration::from_secs(3600);
/// A failed apply (files locked on Windows) waits this long before the launch fast path retries.
const FAILED_COOLDOWN: u64 = 3600;
const MAX_ARCHIVE: u64 = 2 << 30;
/// Environment switch that turns updates off entirely (packagers, managed machines).
pub const DISABLE_ENV: &str = "CARDBOARD_NO_UPDATE";

pub type Result<T> = std::result::Result<T, String>;

// ---------------------------------------------------------------- versions

/// A semantic version (`1.2.3`, `1.2.3-beta.1`); build metadata is ignored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Vec<String>,
}

impl Version {
    /// Parses `1.2.3`, `v1.2.3`, `1.2.3-rc.1+build`.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().strip_prefix('v').unwrap_or(text.trim());
        let text = text.split('+').next()?;
        let (core, pre) = match text.split_once('-') {
            Some((core, pre)) => (core, pre.split('.').map(String::from).collect()),
            None => (text, vec![]),
        };
        let mut parts = core.split('.').map(|p| {
            (!p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
                .then(|| p.parse().ok())
                .flatten()
        });
        let version = Self {
            major: parts.next()??,
            minor: parts.next()??,
            patch: parts.next()??,
            pre,
        };
        let pre_ok = version
            .pre
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'));
        (parts.next().is_none() && pre_ok).then_some(version)
    }
    pub fn current() -> Self {
        Self::parse(CURRENT).expect("CARGO_PKG_VERSION is semver")
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() {
            write!(f, "-{}", self.pre.join("."))?;
        }
        Ok(())
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering::*;
        let core =
            (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch));
        if core != Equal {
            return core;
        }
        // A release outranks its pre-releases; otherwise compare identifiers (semver §11).
        match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => return Equal,
            (true, false) => return Greater,
            (false, true) => return Less,
            _ => {}
        }
        for (a, b) in self.pre.iter().zip(&other.pre) {
            let order = match (a.parse::<u64>(), b.parse::<u64>()) {
                (Ok(a), Ok(b)) => a.cmp(&b),
                (Ok(_), Err(_)) => Less,
                (Err(_), Ok(_)) => Greater,
                _ => a.cmp(b),
            };
            if order != Equal {
                return order;
            }
        }
        self.pre.len().cmp(&other.pre.len())
    }
}
impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

// ---------------------------------------------------------------- releases

/// The release archive for the platform this binary was built for.
pub fn asset_name() -> Option<&'static str> {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("cardboard-linux-x64.tar.gz")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("cardboard-macos-arm64.zip")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("cardboard-macos-x64.zip")
    } else if cfg!(all(windows, target_arch = "x86_64")) {
        Some("cardboard-windows-x64.zip")
    } else {
        None
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: Version,
    pub notes_url: String,
    pub archive: Asset,
    pub signature: Asset,
}

#[derive(Debug, PartialEq)]
pub enum Found {
    UpToDate(Version),
    /// Newer, but this platform's archive or its signature isn't uploaded yet.
    NotYet(Version),
    Available(Release),
}

/// Reads a GitHub "latest release" response.
pub fn select(release: &Value, current: &Version, asset: &str) -> Result<Found> {
    let tag = release["tag_name"]
        .as_str()
        .ok_or("the release has no tag")?;
    let version = Version::parse(tag).ok_or_else(|| format!("unexpected release tag {tag}"))?;
    if release["draft"].as_bool() == Some(true) || release["prerelease"].as_bool() == Some(true) {
        return Ok(Found::UpToDate(current.clone()));
    }
    if version <= *current {
        return Ok(Found::UpToDate(version));
    }
    let find = |name: &str| {
        release["assets"].as_array()?.iter().find_map(|a| {
            (a["name"].as_str() == Some(name)).then(|| Asset {
                name: name.to_string(),
                url: a["browser_download_url"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                size: a["size"].as_u64().unwrap_or(0),
            })
        })
    };
    let (Some(archive), Some(signature)) = (find(asset), find(&format!("{asset}.minisig"))) else {
        return Ok(Found::NotYet(version));
    };
    if archive.url.is_empty() || signature.url.is_empty() {
        return Ok(Found::NotYet(version));
    }
    Ok(Found::Available(Release {
        notes_url: release["html_url"].as_str().unwrap_or_default().to_string(),
        version,
        archive,
        signature,
    }))
}

// ---------------------------------------------------------------- signatures

/// The key release builds trust. Debug builds may point `CARDBOARD_UPDATE_TEST_KEY` at a
/// throwaway public key file for local end-to-end tests; release builds ignore it.
pub fn trusted_key() -> Result<PublicKey> {
    #[cfg(debug_assertions)]
    if let Some(path) = std::env::var_os("CARDBOARD_UPDATE_TEST_KEY") {
        return PublicKey::from_file(&path).map_err(|e| format!("test key: {e}"));
    }
    PublicKey::decode(PUBLIC_KEY).map_err(|e| format!("embedded update key: {e}"))
}

/// The trusted comment the release job writes: binds a signature to one file and one version,
/// so a valid signature can't be replayed on another platform's archive or an older release.
pub fn trusted_comment(file: &str, version: &Version) -> String {
    format!("cardboard file:{file} version:{version}")
}

fn comment_matches(comment: &str, file: &str, version: &Version) -> Result<()> {
    let field = |key: &str| {
        comment
            .split_whitespace()
            .find_map(|w| w.strip_prefix(key)?.strip_prefix(':'))
    };
    if field("file") != Some(file) {
        return Err(format!(
            "the signature is for a different file (\"{comment}\"), expected {file}"
        ));
    }
    if field("version") != Some(&version.to_string()) {
        return Err(format!(
            "the signature is for a different version (\"{comment}\"), expected {version}"
        ));
    }
    Ok(())
}

/// Parse a `.minisig` and check, before trusting anything, that it claims the right file and
/// version. Cheap enough to run before downloading the archive.
pub fn parse_signature(text: &str, file: &str, version: &Version) -> Result<Signature> {
    let signature = Signature::decode(text).map_err(|e| format!("bad signature file: {e}"))?;
    comment_matches(signature.trusted_comment(), file, version)?;
    Ok(signature)
}

/// Verify `data` (streamed) against `signature`, including the trusted comment.
pub fn verify(
    key: &PublicKey,
    signature: &Signature,
    data: &mut dyn Read,
    file: &str,
    version: &Version,
) -> Result<()> {
    let mut verifier = key
        .verify_stream(signature)
        .map_err(|e| format!("signature rejected: {e}"))?;
    let mut buf = vec![0; 1 << 16];
    loop {
        let n = data.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        verifier.update(&buf[..n]);
    }
    verifier
        .finalize()
        .map_err(|e| format!("signature rejected: {e}"))?;
    comment_matches(signature.trusted_comment(), file, version)
}

// ---------------------------------------------------------------- installs

/// An installer-managed copy of Cardboard.
#[derive(Clone, Debug)]
pub struct Install {
    /// The folder replaced as a whole: `.../cardboard/app`, `Cardboard.app`, `Programs\Cardboard`.
    pub root: PathBuf,
    /// Per-user folder for the update state, downloads and the apply helper.
    pub staging: PathBuf,
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Where the installers put Cardboard on this OS, and the matching staging folder.
fn managed_locations() -> Vec<Install> {
    let home = crate::store::home_dir();
    if cfg!(target_os = "macos") {
        let staging = home.join("Library/Application Support/Cardboard/update");
        [PathBuf::from("/Applications"), home.join("Applications")]
            .into_iter()
            .map(|dir| Install {
                root: dir.join("Cardboard.app"),
                staging: staging.clone(),
            })
            .collect()
    } else if cfg!(windows) {
        let local = env_path("LOCALAPPDATA").unwrap_or_else(|| home.join("AppData/Local"));
        vec![Install {
            root: local.join("Programs").join("Cardboard"),
            staging: local.join("Cardboard").join("update"),
        }]
    } else {
        let data = env_path("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share"));
        vec![Install {
            root: data.join("cardboard").join("app"),
            staging: data.join("cardboard").join("update"),
        }]
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

impl Install {
    /// The install this executable belongs to, or why it isn't one that can update itself.
    pub fn current() -> Result<Self> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe = exe.canonicalize().unwrap_or(exe);
        if asset_name().is_none() {
            return Err("there are no release builds for this platform".into());
        }
        for install in managed_locations() {
            if let Ok(root) = install.root.canonicalize()
                && exe.starts_with(&root)
            {
                return Ok(Install { root, ..install });
            }
        }
        let dir = exe.parent().unwrap_or(&exe).display().to_string();
        Err(format!(
            "this Cardboard runs from {dir}, not from a folder the installer manages, so it never updates itself. Update it the way you installed it (for a source build: git pull, then rebuild)"
        ))
    }
    /// The managed install at `root` (used by the apply helper, which runs from a temp copy).
    pub fn at(root: &Path) -> Result<Self> {
        managed_locations()
            .into_iter()
            .find(|i| same_path(&i.root, root))
            .map(|i| Install {
                root: root.canonicalize().unwrap_or_else(|_| root.to_path_buf()),
                ..i
            })
            .ok_or_else(|| format!("{} is not a Cardboard install folder", root.display()))
    }
    fn sibling(&self, suffix: &str) -> PathBuf {
        let name = self.root.file_name().unwrap_or_default().to_string_lossy();
        self.root.with_file_name(format!(".{name}.{suffix}"))
    }
    /// A verified update waiting to be swapped in. Next to the install, so the swap is a rename.
    pub fn staged(&self) -> PathBuf {
        self.sibling("new")
    }
    fn old(&self) -> PathBuf {
        self.sibling("old")
    }
    fn extracting(&self) -> PathBuf {
        self.sibling("extract")
    }
    /// The CLI inside an install-shaped folder (`root` or a staged copy).
    pub fn cli_in(root: &Path) -> PathBuf {
        if cfg!(target_os = "macos") {
            root.join("Contents/MacOS/cardboard-cli")
        } else if cfg!(windows) {
            root.join("cardboard.exe")
        } else {
            root.join("cardboard")
        }
    }
    pub fn desktop_in(root: &Path) -> PathBuf {
        if cfg!(target_os = "macos") {
            root.join("Contents/MacOS/Cardboard")
        } else if cfg!(windows) {
            root.join("cardboard-desktop.exe")
        } else {
            root.join("cardboard-desktop")
        }
    }
    fn state_file(&self) -> PathBuf {
        self.staging.join("state.json")
    }
}

/// Top-level folder inside a release archive (see scripts/package.sh).
fn archive_top(asset: &str) -> String {
    if cfg!(target_os = "macos") {
        "Cardboard.app".into()
    } else {
        asset
            .trim_end_matches(".tar.gz")
            .trim_end_matches(".zip")
            .to_string()
    }
}

// ---------------------------------------------------------------- state

/// What the updater remembers between runs (`<staging>/state.json`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct State {
    /// Unix seconds of the last successful check.
    pub last_check: u64,
    pub latest: Option<String>,
    pub notes_url: Option<String>,
    /// Version sitting verified in `Install::staged()`.
    pub staged: Option<String>,
    /// Settings toggle; None means on.
    pub auto_update: Option<bool>,
    /// Unix seconds of the last apply that couldn't swap (files in use).
    pub failed_at: Option<u64>,
}

impl State {
    pub fn load(install: &Install) -> Self {
        std::fs::read_to_string(install.state_file())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }
    pub fn save(&self, install: &Install) -> Result<()> {
        std::fs::create_dir_all(&install.staging).map_err(|e| e.to_string())?;
        let tmp = install.staging.join("state.json.tmp");
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, install.state_file()).map_err(|e| e.to_string())
    }
    pub fn auto_update(&self) -> bool {
        self.auto_update.unwrap_or(true)
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Why updates are off by environment, if they are.
pub fn disabled_by_env() -> Option<String> {
    std::env::var_os(DISABLE_ENV)
        .filter(|v| !v.is_empty() && v != "0")
        .map(|_| format!("{DISABLE_ENV} is set"))
}

/// The staged version, if it is newer than this binary and still on disk.
pub fn pending(install: &Install, state: &State) -> Option<Version> {
    let version = Version::parse(state.staged.as_deref()?)?;
    (version > Version::current() && Install::cli_in(&install.staged()).is_file())
        .then_some(version)
}

// ---------------------------------------------------------------- network

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_global(Some(timeout))
        .user_agent(concat!(
            "Cardboard/",
            env!("CARGO_PKG_VERSION"),
            " (updater)"
        ))
        .http_status_as_error(false)
        .build()
        .into()
}

fn latest_url() -> String {
    #[cfg(debug_assertions)]
    if let Ok(url) = std::env::var("CARDBOARD_UPDATE_API") {
        return url;
    }
    LATEST.into()
}

fn get(agent: &ureq::Agent, url: &str) -> Result<ureq::http::Response<ureq::Body>> {
    let response = agent
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .call()
        .map_err(|e| {
            format!(
                "could not reach {}: {e}",
                url.split('/').nth(2).unwrap_or(url)
            )
        })?;
    match response.status().as_u16() {
        200..=299 => Ok(response),
        403 | 429 => Err("GitHub's rate limit was reached; will try again later".into()),
        404 => Err("no published release found".into()),
        code => Err(format!("{url} answered HTTP {code}")),
    }
}

/// Ask GitHub what the latest release is.
pub fn check() -> Result<Found> {
    let asset = asset_name().ok_or("there are no release builds for this platform")?;
    let response = get(&agent(Duration::from_secs(20)), &latest_url())?;
    let text = response
        .into_body()
        .read_to_string()
        .map_err(|e| format!("unexpected answer from GitHub: {e}"))?;
    let release: Value =
        serde_json::from_str(&text).map_err(|e| format!("unexpected answer from GitHub: {e}"))?;
    select(&release, &Version::current(), asset)
}

/// Download `release`, verify it, extract it and leave it ready in `install.staged()`.
pub fn stage(install: &Install, release: &Release) -> Result<()> {
    let key = trusted_key()?;
    let agent = agent(Duration::from_secs(30 * 60));
    let file = &release.archive.name;
    let version = &release.version;
    let mut text = String::new();
    get(&agent, &release.signature.url)?
        .into_body()
        .into_reader()
        .take(4096)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    let signature = parse_signature(&text, file, version)?;

    let downloads = install.staging.join("download");
    let _ = std::fs::remove_dir_all(&downloads);
    std::fs::create_dir_all(&downloads).map_err(|e| e.to_string())?;
    let archive = downloads.join(file);
    let mut body = get(&agent, &release.archive.url)?.into_body().into_reader();
    let mut out = std::fs::File::create(&archive).map_err(|e| e.to_string())?;
    let written = std::io::copy(&mut (&mut body).take(MAX_ARCHIVE), &mut out)
        .map_err(|e| format!("download interrupted: {e}"))?;
    out.flush().map_err(|e| e.to_string())?;
    drop(out);
    if release.archive.size != 0 && written != release.archive.size {
        return Err(format!(
            "download incomplete ({written} of {} bytes)",
            release.archive.size
        ));
    }
    stage_archive(install, &archive, &signature, &key, file, version)?;
    let _ = std::fs::remove_dir_all(&downloads);
    let mut state = State::load(install);
    state.staged = Some(version.to_string());
    state.latest = Some(version.to_string());
    state.notes_url = Some(release.notes_url.clone());
    state.failed_at = None;
    state.save(install)
}

/// Verify a downloaded archive and unpack it into `install.staged()`.
pub fn stage_archive(
    install: &Install,
    archive: &Path,
    signature: &Signature,
    key: &PublicKey,
    file: &str,
    version: &Version,
) -> Result<()> {
    let mut reader = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    verify(key, signature, &mut reader, file, version)?;
    let tmp = install.extracting();
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let result = extract(archive, &tmp).and_then(|()| {
        let top = tmp.join(archive_top(file));
        check_layout(&top, version)?;
        let staged = install.staged();
        let _ = std::fs::remove_dir_all(&staged);
        std::fs::rename(&top, &staged).map_err(|e| format!("cannot stage the update: {e}"))
    });
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

fn extract(archive: &Path, into: &Path) -> Result<()> {
    let mut command = if cfg!(target_os = "macos") {
        // The inverse of how package.sh builds the zip; keeps the bundle's signature intact.
        let mut c = Command::new("ditto");
        c.args(["-x", "-k"]).arg(archive).arg(into);
        c
    } else if cfg!(windows) {
        // Windows' own bsdtar reads zip; a Git Bash GNU tar earlier on PATH would not.
        let root = env_path("SystemRoot").unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let mut c = Command::new(root.join("System32").join("tar.exe"));
        c.arg("-xf").arg(archive).arg("-C").arg(into);
        c
    } else {
        let mut c = Command::new("tar");
        c.arg("-xzf").arg(archive).arg("-C").arg(into);
        c
    };
    let output = command
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run the archive tool: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "the update archive is damaged: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

/// The unpacked folder must look like an install and its CLI must report `version`.
fn check_layout(top: &Path, version: &Version) -> Result<()> {
    let cli = Install::cli_in(top);
    for path in [&cli, &Install::desktop_in(top)] {
        if !path.is_file() {
            return Err(format!(
                "the update is missing {}",
                path.strip_prefix(top).unwrap_or(path).display()
            ));
        }
    }
    let output = Command::new(&cli)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("the new version does not start: {e}"))?;
    let reported = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if reported != format!("cardboard {version}") {
        return Err(format!(
            "the update reports \"{reported}\" instead of cardboard {version}"
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------- applying

/// Rename `root` aside, move `staged` into place, delete the old copy. Rolls back if the second
/// rename fails, so `root` is never left missing.
pub fn swap(root: &Path, staged: &Path, old: &Path) -> std::io::Result<()> {
    if old.exists() {
        let _ = std::fs::remove_dir_all(old);
    }
    std::fs::rename(root, old)?;
    if let Err(e) = std::fs::rename(staged, root) {
        std::fs::rename(old, root)?;
        return Err(e);
    }
    // Windows may still hold a file open for a moment; a leftover is removed next time.
    let _ = std::fs::remove_dir_all(old);
    Ok(())
}

/// Swap in the staged update. `wait_pid` is the app that has to exit first.
pub fn apply(install: &Install, wait_pid: Option<u32>) -> Result<Version> {
    if let Some(pid) = wait_pid
        && !os::wait_for_exit(pid, Duration::from_secs(120))
    {
        return Err(format!("Cardboard (pid {pid}) is still running"));
    }
    let mut state = State::load(install);
    let staged = state
        .staged
        .as_deref()
        .and_then(Version::parse)
        .filter(|_| Install::cli_in(&install.staged()).is_file())
        .ok_or("no update is staged")?;
    // Running `cardboard mcp` relays lock their files on Windows; give them a moment.
    let attempts = if cfg!(windows) { 20 } else { 1 };
    let mut result = Ok(());
    for attempt in 0..attempts {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(500));
        }
        result = swap(&install.root, &install.staged(), &install.old());
        if result.is_ok() {
            break;
        }
    }
    if let Err(e) = result {
        state.failed_at = Some(now());
        let _ = state.save(install);
        return Err(format!(
            "could not replace {} ({e}); the update stays staged and is applied next time",
            install.root.display()
        ));
    }
    state.staged = None;
    state.failed_at = None;
    let _ = state.save(install);
    refresh_icons(install);
    Ok(staged)
}

/// Linux: the installer copied icons out of the app folder; refresh them if they're there.
fn refresh_icons(install: &Install) {
    if !cfg!(target_os = "linux") {
        return;
    }
    let Some(data) = install.root.parent().and_then(Path::parent) else {
        return;
    };
    let icons = data.join("icons/hicolor");
    let copy = |from: &str, to: PathBuf| {
        if to.is_file() {
            let _ = std::fs::copy(install.root.join(from), to);
        }
    };
    copy("cardboard.svg", icons.join("scalable/apps/cardboard.svg"));
    for size in [64, 128, 256, 512] {
        copy(
            &format!("cardboard-{size}.png"),
            icons.join(format!("{size}x{size}/apps/cardboard.png")),
        );
    }
}

/// Start the desktop app from `install`, detached, with `args`.
pub fn launch_desktop(install: &Install, args: &[String]) -> Result<()> {
    let mut command = if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg("-n").arg(&install.root);
        if !args.is_empty() {
            c.arg("--args").args(args);
        }
        c
    } else {
        let mut c = Command::new(Install::desktop_in(&install.root));
        c.args(args);
        c
    };
    os::detach(&mut command)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(drop)
        .map_err(|e| format!("cannot start Cardboard: {e}"))
}

/// Hand the swap to a copy of the CLI that waits for this process to exit. With `relaunch`,
/// the helper starts the app again (with those arguments) afterwards.
pub fn spawn_apply(install: &Install, relaunch: Option<&[String]>) -> Result<()> {
    let helpers = install.staging.join("helper");
    // Old helpers delete themselves on Unix; Windows can't, so clear them here.
    let _ = std::fs::remove_dir_all(&helpers);
    std::fs::create_dir_all(&helpers).map_err(|e| e.to_string())?;
    let helper = helpers.join(format!(
        "cardboard-update-{}{}",
        std::process::id(),
        std::env::consts::EXE_SUFFIX
    ));
    std::fs::copy(Install::cli_in(&install.root), &helper)
        .map_err(|e| format!("cannot prepare the update helper: {e}"))?;
    let log =
        std::fs::File::create(install.staging.join("apply.log")).map_err(|e| e.to_string())?;
    let mut command = Command::new(&helper);
    command
        .args(["update", "--apply", "--root"])
        .arg(&install.root)
        .args(["--wait-pid", &std::process::id().to_string()]);
    if let Some(args) = relaunch {
        command.arg("--relaunch").arg("--").args(args);
    }
    let err = log.try_clone().map_err(|e| e.to_string())?;
    os::detach(&mut command)
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(err)
        .spawn()
        .map(drop)
        .map_err(|e| format!("cannot start the update helper: {e}"))
}

/// `cardboard update --apply` (the helper): swap, optionally relaunch, then remove itself.
pub fn run_helper(root: &Path, wait_pid: Option<u32>, relaunch: Option<Vec<String>>) -> Result<()> {
    let install = Install::at(root)?;
    let result = apply(&install, wait_pid);
    match &result {
        Ok(v) => eprintln!("Updated Cardboard to {v}"),
        Err(e) => eprintln!("Update not applied: {e}"),
    }
    // Relaunch either way, so a failed swap never leaves the user without their app.
    if let Some(args) = relaunch
        && let Err(e) = launch_desktop(&install, &args)
    {
        eprintln!("{e}");
    }
    if cfg!(unix)
        && let Ok(me) = std::env::current_exe()
        && me.starts_with(&install.staging)
    {
        let _ = std::fs::remove_file(me);
    }
    result.map(drop)
}

/// Desktop launch fast path: a verified update is staged, so hand off to the helper and exit
/// before opening a window. Skipped for an hour after an apply failed (files in use).
pub fn apply_at_launch(args: &[String]) -> bool {
    if disabled_by_env().is_some() {
        return false;
    }
    let Ok(install) = Install::current() else {
        return false;
    };
    let state = State::load(&install);
    let recently_failed = state
        .failed_at
        .is_some_and(|t| now().saturating_sub(t) < FAILED_COOLDOWN);
    if !state.auto_update() || recently_failed || pending(&install, &state).is_none() {
        return false;
    }
    spawn_apply(&install, Some(args)).is_ok()
}

// ---------------------------------------------------------------- background updater (desktop)

type Hook = Box<dyn Fn() + Send + Sync>;

/// The desktop app's background updater. Its status is pushed to the editor UI by the server
/// (`{"type":"update"}`); the UI calls `set_auto_update`, `check_for_updates` and
/// `restart_to_update` (see `Updater::call`). Not exposed over MCP.
pub struct Updater {
    install: Result<Install>,
    status: Mutex<Value>,
    wake: (Mutex<bool>, Condvar),
    notify: OnceLock<Hook>,
    restart: OnceLock<Hook>,
    restart_requested: AtomicBool,
}

/// Tools the editor UI may send to the updater.
pub const UI_TOOLS: &[&str] = &["set_auto_update", "check_for_updates", "restart_to_update"];

impl Updater {
    pub fn new() -> Arc<Self> {
        Self::with_install(Install::current())
    }
    fn with_install(install: Result<Install>) -> Arc<Self> {
        let updater = Arc::new(Self {
            install,
            status: Mutex::new(Value::Null),
            wake: (Mutex::new(false), Condvar::new()),
            notify: OnceLock::new(),
            restart: OnceLock::new(),
            restart_requested: AtomicBool::new(false),
        });
        updater.publish("idle", None);
        updater
    }
    /// A fake updater that reports a ready update, for UI checks with `cardboard serve`.
    #[cfg(debug_assertions)]
    pub fn fake_ready(version: &str) -> Arc<Self> {
        let updater = Self::with_install(Err("fake".into()));
        let status = json!({"state":"ready","current":CURRENT,"version":version,
            "notesUrl":format!("https://github.com/goerll/cardboard/releases/tag/v{version}"),
            "autoUpdate":true,"managed":true});
        *updater.status.lock().unwrap_or_else(|e| e.into_inner()) = status;
        updater
    }
    /// Called whenever the status changes (the hub wakes its WebSocket clients).
    pub fn on_change(&self, f: impl Fn() + Send + Sync + 'static) {
        let _ = self.notify.set(Box::new(f));
    }
    /// How the host quits for a restart (the desktop closes its window).
    pub fn on_restart(&self, f: impl Fn() + Send + Sync + 'static) {
        let _ = self.restart.set(Box::new(f));
    }
    pub fn status(&self) -> Value {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    fn publish(&self, state: &str, error: Option<String>) {
        let mut status = json!({"state":state,"current":CURRENT});
        match &self.install {
            Err(reason) => {
                status["state"] = json!("unmanaged");
                status["reason"] = json!(reason);
                status["managed"] = json!(false);
            }
            Ok(install) => {
                let saved = State::load(install);
                status["managed"] = json!(true);
                status["autoUpdate"] = json!(saved.auto_update() && disabled_by_env().is_none());
                status["lastCheck"] = json!(saved.last_check);
                if let Some(reason) = disabled_by_env() {
                    status["state"] = json!("disabled");
                    status["reason"] = json!(reason);
                } else if let Some(version) = pending(install, &saved) {
                    status["state"] = json!("ready");
                    status["version"] = json!(version.to_string());
                    status["notesUrl"] = json!(saved.notes_url);
                } else if !saved.auto_update() {
                    status["state"] = json!("disabled");
                }
            }
        }
        if let Some(e) = error {
            status["error"] = json!(e);
        }
        let changed = {
            let mut current = self.status.lock().unwrap_or_else(|e| e.into_inner());
            let changed = *current != status;
            *current = status;
            changed
        };
        if changed && let Some(notify) = self.notify.get() {
            notify();
        }
    }
    fn sleep(&self, timeout: Option<Duration>) {
        let (lock, condvar) = &self.wake;
        let mut woken = lock.lock().unwrap_or_else(|e| e.into_inner());
        match timeout {
            Some(t) => {
                woken = condvar
                    .wait_timeout_while(woken, t, |w| !*w)
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
            }
            None => {
                woken = condvar
                    .wait_while(woken, |w| !*w)
                    .unwrap_or_else(|e| e.into_inner());
            }
        }
        *woken = false;
    }
    fn wake(&self) {
        let (lock, condvar) = &self.wake;
        *lock.lock().unwrap_or_else(|e| e.into_inner()) = true;
        condvar.notify_all();
    }
    /// Start checking in the background (managed installs only).
    pub fn spawn(self: &Arc<Self>) {
        if self.install.is_err() {
            return;
        }
        let updater = self.clone();
        let _ = std::thread::Builder::new()
            .name("cardboard-update".into())
            .spawn(move || updater.run());
    }
    fn run(&self) {
        let Ok(install) = &self.install else { return };
        let mut gap = MIN_GAP;
        loop {
            let state = State::load(install);
            if disabled_by_env().is_some()
                || !state.auto_update()
                || pending(install, &state).is_some()
            {
                self.publish("idle", None);
                self.sleep(None);
                continue;
            }
            let since = Duration::from_secs(now().saturating_sub(state.last_check));
            if since < gap {
                self.publish("idle", None);
                self.sleep(Some(gap - since));
                continue;
            }
            self.publish("checking", None);
            let result = check().and_then(|found| match found {
                Found::Available(release) => {
                    self.publish("downloading", None);
                    stage(install, &release)
                }
                Found::NotYet(_) | Found::UpToDate(_) => Ok(()),
            });
            match result {
                Ok(()) => {
                    let mut state = State::load(install);
                    state.last_check = now();
                    let _ = state.save(install);
                    gap = CHECK_EVERY;
                    self.publish("idle", None);
                }
                Err(e) => {
                    eprintln!("Update check failed: {e}");
                    self.publish("idle", Some(e));
                    self.sleep(Some(RETRY_AFTER));
                }
            }
        }
    }
    /// A request from the editor UI.
    pub fn call(&self, tool: &str, args: &Value) -> Result<Value> {
        match tool {
            "set_auto_update" => {
                let enabled = args["enabled"]
                    .as_bool()
                    .ok_or("expected {enabled: bool}")?;
                if let Some(reason) = disabled_by_env() {
                    return Err(format!("Updates are turned off because {reason}"));
                }
                let install = self.install.as_ref().map_err(Clone::clone)?;
                let mut state = State::load(install);
                state.auto_update = Some(enabled);
                state.save(install)?;
            }
            "check_for_updates" => {
                let install = self.install.as_ref().map_err(Clone::clone)?;
                let mut state = State::load(install);
                state.last_check = 0;
                state.save(install)?;
            }
            "restart_to_update" => {
                if self.status()["state"] != "ready" {
                    return Err("No update is ready yet".into());
                }
                let hook = self
                    .restart
                    .get()
                    .ok_or("Restart Cardboard to finish updating")?;
                self.restart_requested.store(true, Ordering::SeqCst);
                hook();
                return Ok(json!({}));
            }
            _ => return Err(format!("unknown update request {tool}")),
        }
        self.wake();
        self.publish("idle", None);
        Ok(self.status())
    }
    /// Call after the window closed: apply a staged update (and relaunch if the user asked for
    /// the restart). Ignored updates are applied on quit, so the next launch is already new.
    pub fn finish(&self, relaunch_args: &[String]) {
        let Ok(install) = &self.install else { return };
        let state = State::load(install);
        let requested = self.restart_requested.load(Ordering::SeqCst);
        if pending(install, &state).is_none()
            || !(requested || (state.auto_update() && disabled_by_env().is_none()))
        {
            return;
        }
        if let Err(e) = spawn_apply(install, requested.then_some(relaunch_args)) {
            eprintln!("Update not applied: {e}");
        }
    }
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn versions_parse_and_order() {
        assert_eq!(v("v0.2.1"), v("0.2.1"));
        assert_eq!(v("1.2.3+build.5"), v("1.2.3"));
        assert_eq!(v("1.2.3-rc.1").to_string(), "1.2.3-rc.1");
        for bad in [
            "",
            "1.2",
            "1.2.3.4",
            "01a.2.3",
            "1.2.x",
            "1.2.3-",
            "1.2.3-a..b",
        ] {
            assert!(Version::parse(bad).is_none(), "{bad}");
        }
        let ordered = [
            "0.2.0",
            "0.2.1",
            "0.10.0",
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "2.0.0",
        ];
        for pair in ordered.windows(2) {
            assert!(v(pair[0]) < v(pair[1]), "{} < {}", pair[0], pair[1]);
        }
        assert!(Version::current() >= v("0.2.0"));
    }

    fn release(tag: &str, assets: &[&str]) -> Value {
        json!({
            "tag_name": tag,
            "html_url": format!("https://github.com/goerll/cardboard/releases/tag/{tag}"),
            "assets": assets.iter().map(|n| json!({
                "name": n, "size": 10, "browser_download_url": format!("https://example.test/{n}")
            })).collect::<Vec<_>>(),
        })
    }

    #[test]
    fn selects_this_platforms_asset_and_signature() {
        let current = v("0.2.0");
        let asset = "cardboard-linux-x64.tar.gz";
        let all = [
            "cardboard-linux-x64.tar.gz",
            "cardboard-linux-x64.tar.gz.minisig",
            "cardboard-windows-x64.zip",
            "cardboard-windows-x64.zip.minisig",
        ];
        let Found::Available(r) = select(&release("v0.2.1", &all), &current, asset).unwrap() else {
            panic!("expected an update");
        };
        assert_eq!(r.version, v("0.2.1"));
        assert_eq!(r.archive.name, asset);
        assert_eq!(r.signature.name, "cardboard-linux-x64.tar.gz.minisig");
        assert!(r.notes_url.ends_with("/v0.2.1"));
        // Signed later than built: no .minisig yet means not available yet.
        let unsigned = select(&release("v0.2.1", &all[..1]), &current, asset).unwrap();
        assert_eq!(unsigned, Found::NotYet(v("0.2.1")));
        let other = select(&release("v0.2.1", &all[2..]), &current, asset).unwrap();
        assert_eq!(other, Found::NotYet(v("0.2.1")));
        assert_eq!(
            select(&release("v0.2.0", &all), &current, asset).unwrap(),
            Found::UpToDate(v("0.2.0"))
        );
        assert_eq!(
            select(&release("v0.1.9", &all), &current, asset).unwrap(),
            Found::UpToDate(v("0.1.9"))
        );
        let mut pre = release("v0.3.0-rc.1", &all);
        pre["prerelease"] = json!(true);
        assert!(matches!(
            select(&pre, &current, asset),
            Ok(Found::UpToDate(_))
        ));
        assert!(select(&release("nightly", &all), &current, asset).is_err());
    }

    // Fixtures signed with a throwaway key (tests/update/test.pub); never the release key.
    const TEST_KEY: &str = include_str!("../tests/update/test.pub");
    const PAYLOAD: &[u8] = include_bytes!("../tests/update/payload.bin");
    const GOOD: &str = include_str!("../tests/update/good.minisig");
    const OTHER_FILE: &str = include_str!("../tests/update/other-file.minisig");
    const OTHER_VERSION: &str = include_str!("../tests/update/other-version.minisig");
    const OTHER_KEY: &str = include_str!("../tests/update/other-key.minisig");
    const FILE: &str = "cardboard-linux-x64.tar.gz";

    fn check_sig(sig: &str, data: &[u8]) -> Result<()> {
        let key = PublicKey::decode(TEST_KEY).unwrap();
        let version = v("0.2.1");
        let signature = parse_signature(sig, FILE, &version)?;
        verify(&key, &signature, &mut &data[..], FILE, &version)
    }

    #[test]
    fn signatures_are_checked() {
        check_sig(GOOD, PAYLOAD).unwrap();
        let mut tampered = PAYLOAD.to_vec();
        tampered[0] ^= 1;
        assert!(check_sig(GOOD, &tampered).unwrap_err().contains("rejected"));
        assert!(
            check_sig(OTHER_FILE, PAYLOAD)
                .unwrap_err()
                .contains("different file")
        );
        assert!(
            check_sig(OTHER_VERSION, PAYLOAD)
                .unwrap_err()
                .contains("different version")
        );
        assert!(
            check_sig(OTHER_KEY, PAYLOAD)
                .unwrap_err()
                .contains("rejected")
        );
        assert!(
            check_sig("", PAYLOAD)
                .unwrap_err()
                .contains("bad signature")
        );
        // A forged trusted comment breaks the global signature.
        let forged = GOOD.replace("version:0.2.1", "version:0.2.1 ");
        assert!(check_sig(&forged, PAYLOAD).is_err());
    }

    #[test]
    fn embedded_release_key_parses() {
        let key = PublicKey::decode(PUBLIC_KEY).unwrap();
        assert!(
            key.untrusted_comment()
                .unwrap()
                .contains("C617FFC25514E420")
        );
    }

    #[test]
    fn comments_need_exact_fields() {
        let version = v("0.2.1");
        assert!(comment_matches(&trusted_comment(FILE, &version), FILE, &version).is_ok());
        assert!(comment_matches("cardboard file:x.tar.gz version:0.2.1", FILE, &version).is_err());
        assert!(
            comment_matches(
                "cardboard file:cardboard-linux-x64.tar.gz version:0.2.10",
                FILE,
                &version
            )
            .is_err()
        );
        assert!(
            comment_matches("cardboard file:cardboard-linux-x64.tar.gz", FILE, &version).is_err()
        );
    }

    #[test]
    fn swap_replaces_and_rolls_back() {
        let dir = std::env::temp_dir().join(format!("cardboard-swap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (root, staged, old) = (dir.join("app"), dir.join(".app.new"), dir.join(".app.old"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&staged).unwrap();
        std::fs::write(root.join("v"), "old").unwrap();
        std::fs::write(staged.join("v"), "new").unwrap();
        swap(&root, &staged, &old).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("v")).unwrap(), "new");
        assert!(!old.exists() && !staged.exists());
        // Nothing staged: the second rename fails and the install is put back.
        assert!(swap(&root, &staged, &old).is_err());
        assert_eq!(std::fs::read_to_string(root.join("v")).unwrap(), "new");
        assert!(!old.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
