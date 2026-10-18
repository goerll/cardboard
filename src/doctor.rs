//! `cardboard doctor`: check what an agent needs to reach Cardboard, and say how to fix it.
use crate::{mcp, store::home_dir};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(PartialEq)]
enum Level {
    Ok,
    Warn,
    Fail,
}

struct Report {
    failed: bool,
}

impl Report {
    fn line(&mut self, level: Level, what: &str, detail: &str, fix: Option<&str>) {
        let tag = match level {
            Level::Ok => "ok  ",
            Level::Warn => "warn",
            Level::Fail => "FAIL",
        };
        self.failed |= level == Level::Fail;
        println!("  {tag}  {what}: {detail}");
        if let Some(fix) = fix {
            println!("        fix: {fix}");
        }
    }
}

/// The first executable called `name` on PATH.
pub fn which(name: &str) -> Option<PathBuf> {
    let names = if cfg!(windows) {
        vec![format!("{name}.exe"), name.to_string()]
    } else {
        vec![name.to_string()]
    };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .find(|p| p.is_file())
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Claude Code config files that can declare MCP servers.
fn claude_configs() -> Vec<PathBuf> {
    let mut files = vec![];
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        files.push(PathBuf::from(dir).join(".claude.json"));
    }
    files.push(home_dir().join(".claude.json"));
    if let Ok(cwd) = std::env::current_dir() {
        files.push(cwd.join(".mcp.json"));
    }
    files
}

/// `(where, command, args)` for every MCP server entry that runs Cardboard.
pub fn cardboard_servers(config: &Value) -> Vec<(String, String, Vec<String>)> {
    let mut found = vec![];
    let mut scan = |place: String, servers: Option<&Value>| {
        for (name, server) in servers.and_then(Value::as_object).into_iter().flatten() {
            let command = server["command"].as_str().unwrap_or_default().to_string();
            let args: Vec<String> = server["args"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect();
            let runs_cardboard = Path::new(&command)
                .file_stem()
                .is_some_and(|s| s.to_string_lossy().starts_with("cardboard"));
            if runs_cardboard || name == "cardboard" {
                found.push((format!("\"{name}\" ({place})"), command, args));
            }
        }
    };
    scan("all projects".into(), config.get("mcpServers"));
    for (project, settings) in config["projects"].as_object().into_iter().flatten() {
        scan(format!("project {project}"), settings.get("mcpServers"));
    }
    found
}

/// Print every check. Returns false if something blocks agents from using Cardboard.
pub fn run(socket: &Path, library: &Path) -> bool {
    let mut r = Report { failed: false };
    println!("Cardboard doctor ({})", env!("CARGO_PKG_VERSION"));

    let exe = std::env::current_exe().unwrap_or_default();
    match which("cardboard") {
        Some(found) if same_file(&found, &exe) => r.line(
            Level::Ok,
            "binary",
            &format!("{} is on PATH", found.display()),
            None,
        ),
        Some(found) => r.line(
            Level::Warn,
            "binary",
            &format!(
                "PATH runs {}, but this is {}",
                found.display(),
                exe.display()
            ),
            Some("agents launch the one on PATH; make sure it's the version you expect"),
        ),
        None => r.line(
            Level::Fail,
            "binary",
            &format!("`cardboard` is not on PATH (this is {})", exe.display()),
            Some(&format!(
                "ln -s {} ~/.local/bin/cardboard, or use the absolute path in your MCP config",
                exe.display()
            )),
        ),
    }

    r.line(Level::Ok, "socket", &socket.display().to_string(), None);
    let owner = mcp::control(socket, &serde_json::json!({"cardboard":"status"}));
    match &owner {
        // Older apps answer control lines with a JSON-RPC parse error.
        Ok(status) if status.get("jsonrpc").is_some() => r.line(
            Level::Warn,
            "running",
            "an older Cardboard app that can't report its status or hand over to agents",
            Some("update and restart the app"),
        ),
        Ok(status) => {
            let mode = status["mode"].as_str().unwrap_or("app");
            let detail = format!(
                "{mode} (pid {}, version {}, library {})",
                status["pid"],
                status["version"].as_str().unwrap_or("?"),
                status["library"].as_str().unwrap_or("?")
            );
            let (level, fix) = if mode == "headless" {
                (
                    Level::Ok,
                    Some(
                        "an agent session is hosting the library; start the app to watch, it takes over",
                    ),
                )
            } else {
                (Level::Ok, None)
            };
            r.line(level, "running", &detail, fix);
            if status["version"].as_str() != Some(env!("CARGO_PKG_VERSION")) {
                r.line(
                    Level::Warn,
                    "version",
                    "the running Cardboard is a different version than this binary",
                    Some("restart the app after upgrading"),
                );
            }
        }
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) =>
        {
            r.line(
                Level::Ok,
                "running",
                "nothing is listening; `cardboard mcp` will host the library headless",
                Some("open the Cardboard app (or `cardboard serve --open`) to watch agents live"),
            )
        }
        Err(e) => r.line(
            Level::Warn,
            "running",
            &format!("something answers on the socket but not as expected: {e}"),
            Some("restart the Cardboard app"),
        ),
    }
    if let Some(app) = which("cardboard-desktop") {
        r.line(Level::Ok, "desktop app", &app.display().to_string(), None);
    } else {
        r.line(
            Level::Warn,
            "desktop app",
            "`cardboard-desktop` is not on PATH",
            Some("install the desktop app, or use `cardboard serve --open` in a browser"),
        );
    }

    let library = owner
        .as_ref()
        .ok()
        .and_then(|s| s["library"].as_str())
        .map(PathBuf::from)
        .unwrap_or_else(|| library.to_path_buf());
    let probe = library.join(".cardboard-doctor");
    let writable = std::fs::create_dir_all(&library).is_ok()
        && std::fs::write(&probe, b"").is_ok()
        && std::fs::remove_file(&probe).is_ok();
    let files = std::fs::read_dir(&library)
        .map(|d| {
            d.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "cardboard"))
                .count()
        })
        .unwrap_or(0);
    if writable {
        r.line(
            Level::Ok,
            "library",
            &format!("{} ({files} files)", library.display()),
            None,
        );
    } else {
        r.line(
            Level::Fail,
            "library",
            &format!("{} is not writable", library.display()),
            Some("fix its permissions or set CARDBOARD_LIBRARY / --library to another directory"),
        );
    }

    let mut configured = false;
    for path in claude_configs() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(config) = serde_json::from_str::<Value>(&text) else {
            r.line(
                Level::Warn,
                "claude code",
                &format!("{} is not valid JSON", path.display()),
                None,
            );
            continue;
        };
        for (place, command, args) in cardboard_servers(&config) {
            configured = true;
            let resolves = Path::new(&command).is_file() || which(&command).is_some();
            let relay = args.first().map(String::as_str) == Some("mcp");
            let what = format!("{place} in {}", path.display());
            let detail = format!("{command} {}", args.join(" "));
            if !resolves {
                r.line(
                    Level::Fail,
                    "claude code",
                    &format!("{what}: `{command}` not found"),
                    Some("use the absolute path of the cardboard binary as the command"),
                );
            } else if !relay {
                r.line(
                    Level::Warn,
                    "claude code",
                    &format!("{what}: {detail}"),
                    Some("the args should start with `mcp`"),
                );
            } else {
                r.line(Level::Ok, "claude code", &format!("{what}: {detail}"), None);
            }
        }
    }
    if !configured {
        r.line(
            Level::Warn,
            "claude code",
            "no Cardboard MCP server found in Claude Code's config",
            Some("claude mcp add --scope user cardboard -- cardboard mcp"),
        );
    }

    let browsers = [
        "chromium",
        "chromium-browser",
        "google-chrome",
        "google-chrome-stable",
    ];
    let mac_chrome = Path::new("/Applications/Google Chrome.app");
    match browsers.iter().find_map(|b| which(b)) {
        Some(found) => r.line(Level::Ok, "chromium", &found.display().to_string(), None),
        None if cfg!(target_os = "macos") && mac_chrome.exists() => {
            r.line(Level::Ok, "chromium", &mac_chrome.display().to_string(), None)
        }
        None => r.line(
            Level::Warn,
            "chromium",
            "no Chromium-based browser on PATH",
            Some("install Chromium to open the editor with `cardboard serve --open` or a headless editor URL"),
        ),
    }
    !r.failed
}
