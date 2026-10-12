// No console window behind the app on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]
mod backend;
#[cfg(target_os = "macos")]
mod mac;
mod shell;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // macOS loads Chromium from the bundle at runtime; subprocesses use the helper apps.
    #[cfg(target_os = "macos")]
    let _framework = mac::load_framework(false);
    if let Some(code) = shell::subprocess() {
        std::process::exit(code);
    }
    let args: Vec<String> = std::env::args().collect();
    let value = |name: &str| {
        args.iter().enumerate().find_map(|(i, a)| {
            a.strip_prefix(&format!("{name}=")).map(String::from).or_else(|| (a == name).then(|| args.get(i + 1).cloned()).flatten())
        })
    };
    let library = value("--library").map(PathBuf::from);
    let devtools = args.iter().any(|a| a == "--devtools");
    #[cfg(target_os = "macos")]
    mac::setup_application();
    let runtime = tokio::runtime::Runtime::new()?;
    let backend = backend::Backend::start(&runtime, library)?;
    eprintln!("Cardboard editor at {}", backend.url);
    let result = shell::run(&backend.url, devtools);
    backend.shutdown(runtime);
    result?;
    Ok(())
}
