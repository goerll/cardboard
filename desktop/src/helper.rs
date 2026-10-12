//! macOS helper executable: Chromium's GPU, renderer and utility processes run from the
//! "Cardboard Helper*.app" bundles inside Contents/Frameworks. Other platforms re-run the
//! main executable instead, so this binary is only packaged on macOS.
#[cfg(target_os = "macos")]
#[path = "mac.rs"]
mod mac;
#[path = "shell.rs"]
#[allow(dead_code)]
mod shell;

fn main() {
    #[cfg(target_os = "macos")]
    let _framework = mac::load_framework(true);
    std::process::exit(shell::subprocess().unwrap_or(0));
}
