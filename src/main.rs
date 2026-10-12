use cardboard::{hub::Hub, mcp::CardboardServer, store::Store, tools};
use rmcp::ServiceExt;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("help");
    let option = |name: &str| args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone());
    let path_option = |name: &str| option(name).map(PathBuf::from);
    if command == "mcp" && option("--document").is_none() {
        #[cfg(unix)]
        {
            let path = path_option("--socket").unwrap_or_else(cardboard::mcp::default_socket);
            let mut stream=tokio::net::UnixStream::connect(&path).await.map_err(|e|format!("Cannot connect to Cardboard at {}: {e}. Open the desktop app or `cardboard serve`, or use mcp --document PATH for standalone editing.",path.display()))?;
            let (mut reader, mut writer) = stream.split();
            let mut input = tokio::io::stdin();
            let mut output = tokio::io::stdout();
            tokio::select! { r=tokio::io::copy(&mut input,&mut writer)=>{r?;}, r=tokio::io::copy(&mut reader,&mut output)=>{r?;} }
            return Ok(());
        }
    }
    match command {
        "mcp" => {
            let path = path_option("--document").unwrap();
            let hub = Hub::new(Store::open_document(&path)?);
            let service = CardboardServer { store: hub }
                .serve(rmcp::transport::stdio())
                .await?;
            service.waiting().await?;
        }
        "serve" => {
            let library = path_option("--library").unwrap_or_else(Store::default_library);
            let port = option("--port")
                .map(|p| p.parse::<u16>())
                .transpose()?
                .unwrap_or(0);
            let hub = Hub::new(Store::library(&library, Some(&Store::legacy_workspace()))?);
            hub.spawn_autosave();
            let server = cardboard::server::run_server(hub.clone(), port).await?;
            #[cfg(unix)]
            let socket = {
                let socket = path_option("--socket").unwrap_or_else(cardboard::mcp::default_socket);
                let listener = cardboard::mcp::bind_socket(&socket)?;
                let shared = hub.clone();
                tokio::spawn(async move {
                    if let Err(e) = cardboard::mcp::serve_socket(listener, shared).await {
                        eprintln!("MCP server ended: {e}");
                    }
                });
                eprintln!("Cardboard MCP listening on {}", socket.display());
                socket
            };
            eprintln!("Cardboard library: {}", library.display());
            println!("{}", server.url);
            if args.iter().any(|a| a == "--open") {
                open_browser(&server.url);
            }
            shutdown_signal().await;
            if let Err(e) = hub.flush() {
                eprintln!("{e}");
            }
            #[cfg(unix)]
            let _ = std::fs::remove_file(socket);
        }
        "export" => {
            let path = path_option("--document").ok_or("export needs --document PATH")?;
            let mut store = Store::open_document(&path)?;
            let result = tools::call(&mut store, "export_html", &serde_json::json!({}))?;
            println!("{}", result["html"].as_str().unwrap());
        }
        _ => println!(
            "Cardboard 0.2\n\nDesktop: ./scripts/run-desktop.sh\nEditor in a browser: cardboard serve [--library DIR] [--port N] [--open] [--socket PATH]\nAgent relay: cardboard mcp [--socket PATH]\nStandalone: cardboard mcp --document FILE.cardboard\nHTML export: cardboard export --document FILE.cardboard"
        ),
    }
    Ok(())
}

fn open_browser(url: &str) {
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
