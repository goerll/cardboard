//! Native window: one windowed (GPU-composited) Chromium view hosting the editor web UI.
//! Uses CEF's Views framework with the Alloy runtime style, so no Chrome toolbar or tabstrip.
use cef::args::Args;
use cef::*;
use std::sync::OnceLock;

/// Origin of the editor UI; navigation anywhere else opens in the system browser.
static ORIGIN: OnceLock<String> = OnceLock::new();
static DEVTOOLS: OnceLock<bool> = OnceLock::new();

fn same_origin(url: &str) -> bool {
    ORIGIN
        .get()
        .is_some_and(|o| url == o || url.starts_with(&format!("{o}/")))
        || url.starts_with("devtools://")
        || url.starts_with("blob:")
        || url.starts_with("data:")
        || url == "about:blank"
}
fn open_external(url: &str) {
    if !(url.starts_with("http://") || url.starts_with("https://") || url.starts_with("mailto:")) {
        return;
    }
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(windows)]
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
}

/// The native event type CEF passes to keyboard handlers differs per platform.
#[cfg(target_os = "linux")]
type OsEvent<'a> = Option<&'a mut sys::XEvent>;
#[cfg(windows)]
type OsEvent<'a> = Option<&'a mut sys::MSG>;
#[cfg(target_os = "macos")]
type OsEvent<'a> = *mut u8;
fn show_devtools(browser: &Browser) {
    if let Some(host) = browser.host() {
        host.show_dev_tools(Some(&WindowInfo::default()), None, None, None);
    }
}

wrap_app! { pub struct ShellApp {} impl App {
    fn on_before_command_line_processing(&self, process_type: Option<&CefString>, command_line: Option<&mut CommandLine>) {
        // Only the browser process (empty type) decides the display backend.
        if process_type.is_some_and(|t| !t.to_string().is_empty()) { return; }
        let Some(line) = command_line else { return };
        line.append_switch(Some(&"enable-gpu-rasterization".into()));
        if !cfg!(target_os = "linux") { return; }
        if line.has_switch(Some(&"ozone-platform".into())) == 0 && line.has_switch(Some(&"ozone-platform-hint".into())) == 0 {
            line.append_switch_with_value(Some(&"ozone-platform-hint".into()), Some(&"auto".into()));
        }
        // Wayland: smooth scroll/pinch and the app id come from these.
        line.append_switch_with_value(Some(&"enable-features".into()), Some(&"WaylandWindowDecorations,OverlayScrollbar".into()));
        line.append_switch_with_value(Some(&"class".into()), Some(&"cardboard".into()));
    }
} }

wrap_request_handler! { pub struct ShellRequests {} impl RequestHandler {
    fn on_before_browse(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, request: Option<&mut Request>, _user_gesture: i32, _is_redirect: i32) -> i32 {
        let Some(request) = request else { return 0 };
        // Subframes (the design iframe) are srcdoc/about:blank and stay in-app.
        if frame.is_some_and(|f| f.is_main() == 0) { return 0; }
        let url = CefString::from(&request.url()).to_string();
        if same_origin(&url) { 0 } else { open_external(&url); 1 }
    }
} }

wrap_life_span_handler! { pub struct ShellLifeSpan {} impl LifeSpanHandler {
    fn on_before_popup(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, _popup_id: i32, target_url: Option<&CefString>, _target_frame_name: Option<&CefString>, _target_disposition: WindowOpenDisposition, _user_gesture: i32, _popup_features: Option<&PopupFeatures>, _window_info: Option<&mut WindowInfo>, _client: Option<&mut Option<Client>>, _settings: Option<&mut BrowserSettings>, _extra_info: Option<&mut Option<DictionaryValue>>, _no_javascript_access: Option<&mut i32>) -> i32 {
        if let Some(url) = target_url { open_external(&url.to_string()); }
        1
    }
} }

wrap_context_menu_handler! { pub struct ShellMenu {} impl ContextMenuHandler {
    fn on_before_context_menu(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, params: Option<&mut ContextMenuParams>, model: Option<&mut MenuModel>) {
        // Text fields keep cut/copy/paste; everything else uses the editor's own menus.
        if params.is_some_and(|p| p.is_editable() == 1) { return; }
        if let Some(model) = model { model.clear(); }
    }
} }

wrap_download_handler! { pub struct ShellDownloads {} impl DownloadHandler {
    fn can_download(&self, _browser: Option<&mut Browser>, _url: Option<&CefString>, _method: Option<&CefString>) -> i32 { 1 }
    fn on_before_download(&self, _browser: Option<&mut Browser>, _item: Option<&mut DownloadItem>, suggested_name: Option<&CefString>, callback: Option<&mut BeforeDownloadCallback>) -> i32 {
        let Some(callback) = callback else { return 0 };
        let name = suggested_name.map(|n| n.to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| "cardboard-export.html".into());
        let dir = std::env::var_os("XDG_DOWNLOAD_DIR").map(std::path::PathBuf::from).unwrap_or_else(|| cardboard::store::home_dir().join("Downloads"));
        let _ = std::fs::create_dir_all(&dir);
        // Never overwrite: "name (1).html", "name (2).html", ...
        let path = std::path::Path::new(&name);
        let (stem, ext) = (path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(), path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default());
        let mut target = dir.join(&name);
        let mut n = 1;
        while target.exists() { target = dir.join(format!("{stem} ({n}){ext}")); n += 1; }
        callback.cont(Some(&target.to_string_lossy().as_ref().into()), 0);
        1
    }
} }

wrap_keyboard_handler! { pub struct ShellKeys {} impl KeyboardHandler {
    fn on_pre_key_event(&self, browser: Option<&mut Browser>, event: Option<&KeyEvent>, _os_event: OsEvent<'_>, _is_shortcut: Option<&mut i32>) -> i32 {
        // F12 opens DevTools; every other key goes to the page untouched.
        if let (Some(browser), Some(event)) = (browser, event)
            && event.type_ == KeyEventType::RAWKEYDOWN && event.windows_key_code == 123
        { show_devtools(browser); return 1; }
        0
    }
} }

wrap_client! { pub struct ShellClient {} impl Client {
    fn request_handler(&self) -> Option<RequestHandler> { Some(ShellRequests::new()) }
    fn life_span_handler(&self) -> Option<LifeSpanHandler> { Some(ShellLifeSpan::new()) }
    fn context_menu_handler(&self) -> Option<ContextMenuHandler> { Some(ShellMenu::new()) }
    fn download_handler(&self) -> Option<DownloadHandler> { Some(ShellDownloads::new()) }
    fn keyboard_handler(&self) -> Option<KeyboardHandler> { Some(ShellKeys::new()) }
} }

wrap_browser_view_delegate! { pub struct ShellBrowserView {} impl ViewDelegate {} impl BrowserViewDelegate {
    fn on_browser_created(&self, _view: Option<&mut BrowserView>, browser: Option<&mut Browser>) {
        if DEVTOOLS.get() == Some(&true) && let Some(browser) = browser { show_devtools(browser); }
    }
    fn browser_runtime_style(&self) -> RuntimeStyle { RuntimeStyle::ALLOY }
} }

wrap_window_delegate! { pub struct ShellWindow { browser_view: BrowserView } impl ViewDelegate {
    fn preferred_size(&self, _view: Option<&mut View>) -> Size { Size { width: 1440, height: 960 } }
    fn minimum_size(&self, _view: Option<&mut View>) -> Size { Size { width: 960, height: 640 } }
} impl PanelDelegate {} impl WindowDelegate {
    fn initial_bounds(&self, _window: Option<&mut Window>) -> Rect { Rect { x: 0, y: 0, width: 1440, height: 960 } }
    fn on_window_created(&self, window: Option<&mut Window>) {
        let Some(window) = window else { return };
        window.add_child_view(Some(&mut View::from(&self.browser_view)));
        window.set_title(Some(&"Cardboard".into()));
        window.show();
        self.browser_view.request_focus();
    }
    fn on_window_destroyed(&self, _window: Option<&mut Window>) { quit_message_loop(); }
    fn can_close(&self, _window: Option<&mut Window>) -> i32 {
        // Let the page run unload handlers (the UI flushes pending edits) before the window goes.
        self.browser_view.browser().and_then(|b| b.host()).map_or(1, |h| h.try_close_browser())
    }
    // The Rust bindings default these to 0 (C++ CEF defaults to true); a non-resizable window
    // pins min == max size, so tiling compositors can't size it.
    fn can_resize(&self, _window: Option<&mut Window>) -> i32 { 1 }
    fn can_maximize(&self, _window: Option<&mut Window>) -> i32 { 1 }
    fn can_minimize(&self, _window: Option<&mut Window>) -> i32 { 1 }
    fn window_runtime_style(&self) -> RuntimeStyle { RuntimeStyle::ALLOY }
    fn linux_window_properties(&self, _window: Option<&mut Window>, properties: Option<&mut LinuxWindowProperties>) -> i32 {
        let Some(p) = properties else { return 0 };
        // Owned CefStrings convert to empty raw strings in these bindings, so hand CEF a
        // borrowed view of a buffer that lives for the whole process (CEF copies it).
        p.wayland_app_id = app_id();
        p.wm_class_class = app_id();
        p.wm_class_name = app_id();
        1
    }
} }

/// Must run first: renderer/GPU subprocesses re-execute this binary and exit here.
pub fn subprocess() -> Option<i32> {
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let args = Args::new();
    let mut app = ShellApp::new();
    let code = execute_process(
        Some(args.as_main_args()),
        Some(&mut app),
        std::ptr::null_mut(),
    );
    (code >= 0).then_some(code)
}

/// Opens the editor window at `url` and runs Chromium's message loop until the window closes.
pub fn run(url: &str, devtools: bool) -> Result<(), String> {
    let origin = url.split('/').take(3).collect::<Vec<_>>().join("/");
    let _ = ORIGIN.set(origin);
    let _ = DEVTOOLS.set(devtools);
    let args = Args::new();
    let mut app = ShellApp::new();
    let cache = cache_dir().join("chromium");
    std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    let settings = Settings {
        root_cache_path: cache.to_string_lossy().as_ref().into(),
        // The Chromium sandbox needs a signed bootstrap on Windows and macOS; Linux keeps it.
        no_sandbox: (!cfg!(target_os = "linux")).into(),
        ..Default::default()
    };
    if initialize(
        Some(args.as_main_args()),
        Some(&settings),
        Some(&mut app),
        std::ptr::null_mut(),
    ) != 1
    {
        return Err("Chromium failed to initialize".into());
    }
    let browser_view = browser_view_create(
        Some(&mut ShellClient::new()),
        Some(&url.into()),
        Some(&BrowserSettings::default()),
        None,
        None,
        Some(&mut ShellBrowserView::new()),
    )
    .ok_or("Could not create the editor view")?;
    window_create_top_level(Some(&mut ShellWindow::new(browser_view)))
        .ok_or("Could not create the window")?;
    run_message_loop();
    shutdown();
    Ok(())
}

/// Per-user cache folder for Chromium's profile data.
fn cache_dir() -> std::path::PathBuf {
    let home = cardboard::store::home_dir();
    if cfg!(target_os = "macos") {
        home.join("Library/Caches/Cardboard")
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Local"))
            .join("Cardboard")
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| home.join(".cache"))
            .join("cardboard")
    }
}

/// "cardboard" as a borrowed CEF string over a leaked, process-lifetime UTF-16 buffer.
fn app_id() -> CefString {
    static BUF: OnceLock<&'static [u16]> = OnceLock::new();
    let buf = BUF.get_or_init(|| {
        Box::leak(
            "cardboard"
                .encode_utf16()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )
    });
    CefString::from(sys::_cef_string_utf16_t {
        str_: buf.as_ptr() as *mut _,
        length: buf.len(),
        dtor: None,
    })
}
