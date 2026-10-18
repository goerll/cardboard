use cardboard::{hub::Hub, mcp::CardboardServer, relay, store::Store, tools};
use rmcp::ServiceExt;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("help");
    let option = |name: &str| args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone());
    let path_option = |name: &str| option(name).map(PathBuf::from);
    let socket = || path_option("--socket").unwrap_or_else(cardboard::mcp::default_socket);
    let library = || path_option("--library").unwrap_or_else(Store::default_library);
    match command {
        "mcp" if option("--document").is_none() => {
            let options = relay::Options {
                socket: socket(),
                library: library(),
                fallback: !args.iter().any(|a| a == "--no-fallback"),
            };
            relay::run(options, shutdown_signal()).await?;
        }
        "mcp" => {
            let path = path_option("--document").unwrap();
            let hub = Hub::new(Store::open_document(&path)?);
            let service = CardboardServer::new(hub)
                .serve(rmcp::transport::stdio())
                .await?;
            service.waiting().await?;
        }
        "--version" | "-V" | "version" => println!("cardboard {}", env!("CARGO_PKG_VERSION")),
        "update" => {
            if let Err(e) = update(
                &args,
                option("--root").map(PathBuf::from),
                option("--wait-pid"),
            ) {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        "doctor" => {
            if !cardboard::doctor::run(&socket(), &library()) {
                std::process::exit(1);
            }
        }
        "serve" => {
            let library = library();
            let port = option("--port")
                .map(|p| p.parse::<u16>())
                .transpose()?
                .unwrap_or(0);
            // Own the socket before reading the library; a headless agent host saves and hands over.
            let socket = socket();
            let listener = cardboard::mcp::bind_socket(&socket)?;
            let hub = Hub::new(Store::library(&library, Some(&Store::legacy_workspace()))?);
            hub.spawn_autosave();
            let server = cardboard::server::run_server(hub.clone(), port).await?;
            hub.set_host(serde_json::json!({"mode":"serve","editorUrl":server.url}));
            // Debug builds can fake a ready update to check the editor's update pill.
            #[cfg(debug_assertions)]
            if let Ok(version) = std::env::var("CARDBOARD_UPDATE_FAKE_READY") {
                hub.set_updater(cardboard::update::Updater::fake_ready(&version));
            }
            let shared = hub.clone();
            tokio::spawn(async move {
                if let Err(e) = cardboard::mcp::serve_socket(listener, shared).await {
                    eprintln!("MCP server ended: {e}");
                }
            });
            eprintln!("Cardboard MCP listening on {}", socket.display());
            eprintln!("Cardboard library: {}", library.display());
            println!("{}", server.url);
            if args.iter().any(|a| a == "--open") {
                open_browser(&server.url);
            }
            shutdown_signal().await;
            if let Err(e) = hub.flush() {
                eprintln!("{e}");
            }
            // Unix sockets leave a file behind; on Windows the pipe disappears by itself.
            #[cfg(unix)]
            let _ = std::fs::remove_file(&socket);
            #[cfg(windows)]
            let _ = socket;
        }
        "export" => {
            let path = path_option("--document").ok_or("export needs --document PATH")?;
            let mut store = Store::open_document(&path)?;
            let result = tools::call(&mut store, "export_html", &serde_json::json!({}))?;
            println!("{}", result["html"].as_str().unwrap());
        }
        _ => println!(
            "Cardboard {}\n\nDesktop app: cardboard-desktop\nEditor in a browser: cardboard serve [--library DIR] [--port N] [--open] [--socket PATH]\nAgent relay: cardboard mcp [--socket PATH] [--library DIR] [--no-fallback]\n  (hosts the library headless when no app is running; the app takes over when it starts)\nCheck setup: cardboard doctor [--socket PATH] [--library DIR]\nUpdate: cardboard update [--check]   (installs made by the installer; the app also updates itself)\nStandalone: cardboard mcp --document FILE.cardboard\nHTML export: cardboard export --document FILE.cardboard",
            env!("CARGO_PKG_VERSION")
        ),
    }
    Ok(())
}

/// `cardboard update [--check]`, and the internal `--apply` helper the desktop app starts.
fn update(args: &[String], root: Option<PathBuf>, wait_pid: Option<String>) -> Result<(), String> {
    use cardboard::update::{self, Found, Install};
    if args.iter().any(|a| a == "--apply") {
        let root = root.ok_or("--apply needs --root")?;
        let pid = wait_pid
            .map(|p| p.parse::<u32>().map_err(|e| e.to_string()))
            .transpose()?;
        let relaunch = args.iter().any(|a| a == "--relaunch").then(|| {
            let rest = args.iter().position(|a| a == "--");
            rest.map(|i| args[i + 1..].to_vec()).unwrap_or_default()
        });
        return update::run_helper(&root, pid, relaunch);
    }
    let check_only = args.iter().any(|a| a == "--check");
    println!("Cardboard {}", update::CURRENT);
    let install = Install::current();
    if let Some(reason) = update::disabled_by_env() {
        return Err(format!("Updates are turned off: {reason}."));
    }
    if let Err(reason) = &install
        && !check_only
    {
        return Err(format!("Not updating: {reason}."));
    }
    let found = update::check()?;
    let release = match found {
        Found::UpToDate(_) => {
            println!("Up to date.");
            return Ok(());
        }
        Found::NotYet(v) => {
            println!(
                "{v} is out but not signed for this platform yet; try again in a few minutes."
            );
            return Ok(());
        }
        Found::Available(release) => release,
    };
    println!("Latest: {} ({})", release.version, release.notes_url);
    let install = match install {
        Ok(install) if !check_only => install,
        Ok(_) => {
            println!("Run `cardboard update` to install it.");
            return Ok(());
        }
        Err(reason) => {
            println!("This copy can't update itself: {reason}.");
            return Ok(());
        }
    };
    println!("Downloading {}…", release.archive.name);
    update::stage(&install, &release)?;
    println!("Verified the signature.");
    // The open app swaps itself on its next restart; it holds files Windows won't replace.
    let socket = cardboard::mcp::default_socket();
    let running = cardboard::mcp::control(&socket, &serde_json::json!({"cardboard":"status"}))
        .is_ok_and(|s| s["mode"] == "app");
    if running {
        println!(
            "Restart Cardboard to finish updating to {}.",
            release.version
        );
    } else if cfg!(windows) {
        update::spawn_apply(&install, None)?;
        println!(
            "Finishing the update to {} in the background.",
            release.version
        );
    } else {
        let version = update::apply(&install, None)?;
        println!("Updated to {version}.");
    }
    Ok(())
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(windows)]
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    open_browser_linux(url);
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_browser_linux(url: &str) {
    let app = format!("--app={url}");
    let attempts: [(&str, &[&str]); 4] = [
        ("chromium", &[&app]),
        ("google-chrome", &[&app]),
        ("chromium-browser", &[&app]),
        ("xdg-open", &[url]),
    ];
    for (program, arguments) in attempts {
        if std::process::Command::new(program)
            .args(arguments)
            .spawn()
            .is_ok()
        {
            return;
        }
    }
    eprintln!("Open {url} in a Chromium-based browser");
}

/// Resolves on Ctrl-C or SIGTERM so either one flushes pending saves before exit.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}
