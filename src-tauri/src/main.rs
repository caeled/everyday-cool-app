// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_opener::OpenerExt;

/// Returns true when `url` belongs to the app itself and may load inside
/// the window: the Tauri production scheme (`tauri://localhost/...` on
/// macOS), the Windows production origin (`http://tauri.localhost/...`),
/// and inline schemes. Everything else is not the app's own content.
fn is_app_url(url: &tauri::Url) -> bool {
    match url.scheme() {
        "tauri" => true,
        "http" | "https" => url.host_str() == Some("tauri.localhost"),
        "about" | "data" | "blob" => true,
        _ => false,
    }
}

/// Splash timing state. Steven ran v1.1.1 and found the splash flashed
/// past too fast to see, so from v1.1.3 the splash holds for a minimum
/// of ten seconds from launch: it comes down at whichever completes
/// LATER — the main window's first page finishing loading, or the ten
/// seconds elapsing. A separate fifteen-second hard cap (see setup)
/// dismisses it unconditionally, so it can still never trap the user.
/// Clear Cache (File menu, new in v1.1.6). Field failure behind it:
/// after the v1.1.5 update, the app's persisted WebView2 profile kept
/// serving the pre-update Nerd Heaven room — the room's own service
/// worker answers cache-first out of Cache Storage, and neither
/// reinstalling the app nor restarting it clears that profile.
/// Renaming the EBWebView folder fixed it by hand; this menu item is
/// the standing cure, with no folder surgery.
///
/// The item cannot clear the cache in place: the running webview owns
/// those files. So the click writes a flag file and restarts the app,
/// and at the very top of setup — before any window (and therefore any
/// webview) exists — the flag is consumed: only the cache-bearing
/// subdirectories of the profile are deleted (HTTP cache, code cache,
/// GPU cache, and the Service Worker directory, which holds the
/// service worker registrations and their Cache Storage). Local
/// Storage and IndexedDB are never touched, so room progress and
/// preferences survive a Clear Cache.
static CACHE_JUST_CLEARED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

const CLEAR_CACHE_FLAG: &str = "clear-cache.flag";

fn clear_cache_flag_path(handle: &AppHandle) -> Option<std::path::PathBuf> {
    handle
        .path()
        .app_local_data_dir()
        .ok()
        .map(|dir| dir.join(CLEAR_CACHE_FLAG))
}

/// File → Clear Cache: leave the flag for the next launch, then
/// restart. The deletion happens in setup of the new process, before
/// its webview is created (see consume_clear_cache_flag).
fn request_clear_cache(handle: &AppHandle) {
    if let Some(flag) = clear_cache_flag_path(handle) {
        if let Some(parent) = flag.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&flag, b"clear");
    }
    handle.restart();
}

/// Consume a Clear Cache flag left by the previous run. Returns true
/// when a flag was present (the caller then arms the "cache cleared"
/// notice for the main window). Deletion is best-effort with a short
/// retry: the previous process may still be releasing file locks in
/// the first moments after a restart.
fn consume_clear_cache_flag(handle: &AppHandle) -> bool {
    let Some(flag) = clear_cache_flag_path(handle) else {
        return false;
    };
    if !flag.exists() {
        return false;
    }
    let _ = std::fs::remove_file(&flag);

    let mut targets: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(data_dir) = handle.path().app_local_data_dir() {
        // The WebView2 profile: <app local data>/EBWebView/Default.
        // Only cache directories are listed — Local Storage and
        // IndexedDB live in sibling directories and are preserved.
        let profile = data_dir.join("EBWebView").join("Default");
        for sub in ["Service Worker", "Cache", "Code Cache", "GPUCache"] {
            targets.push(profile.join(sub));
        }
    }
    #[cfg(target_os = "macos")]
    {
        // WKWebView has no EBWebView; the app's system cache container
        // (~/Library/Caches/<bundle id>) is pure cache and regenerates.
        if let Ok(home) = handle.path().home_dir() {
            targets.push(
                home.join("Library")
                    .join("Caches")
                    .join(handle.config().identifier.as_str()),
            );
        }
    }
    for target in targets {
        for _ in 0..5 {
            if !target.exists() || std::fs::remove_dir_all(&target).is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
    }
    true
}

/// In-page notice shown once on the main window after a Clear Cache
/// restart, so the click never feels dead. Injected from Rust; the
/// bundled pages are untouched.
const CACHE_CLEARED_NOTICE_JS: &str = "(() => { const d = document.createElement('div'); d.textContent = 'Cache cleared \\u2014 rooms now load their fresh copies. Your saved progress was not touched.'; d.style.cssText = 'position:fixed;top:14px;left:50%;transform:translateX(-50%);z-index:99999;background:#1d3b2a;color:#eaf5ec;padding:10px 18px;border-radius:10px;font:14px system-ui,sans-serif;box-shadow:0 4px 18px rgba(0,0,0,.35)'; document.body.appendChild(d); setTimeout(() => d.remove(), 8000); })();";

static MAIN_LOADED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static MIN_TIME_PASSED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static SPLASH_DISMISSED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Close the splash window (if it is still up) and show the main window.
/// Runs at most once; later calls simply find nothing to do.
fn dismiss_splash(handle: &AppHandle) {
    if SPLASH_DISMISSED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    if let Some(splash) = handle.get_webview_window("splashscreen") {
        let _ = splash.close();
    }
    if let Some(main) = handle.get_webview_window("main") {
        let _ = main.show();
        let _ = main.set_focus();
        // If this launch consumed a Clear Cache flag, say so on the
        // page now that it is visible (once only).
        if CACHE_JUST_CLEARED.swap(false, std::sync::atomic::Ordering::SeqCst) {
            let _ = main.eval(CACHE_CLEARED_NOTICE_JS);
        }
    }
}

/// Dismiss the splash only once both conditions hold: the main window
/// has finished its first load AND the ten-second minimum has elapsed.
fn maybe_dismiss_splash(handle: &AppHandle) {
    if MAIN_LOADED.load(std::sync::atomic::Ordering::SeqCst)
        && MIN_TIME_PASSED.load(std::sync::atomic::Ordering::SeqCst)
    {
        dismiss_splash(handle);
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .menu(|handle| {
            let clear_cache =
                MenuItem::with_id(handle, "clear-cache", "Clear Cache", true, None::<&str>)?;
            let docs = MenuItem::with_id(handle, "docs", "Documentation", true, None::<&str>)?;
            let products =
                MenuItem::with_id(handle, "products", "Product Index", true, None::<&str>)?;
            let about = MenuItem::with_id(
                handle,
                "about",
                "About Everyday Cool",
                true,
                None::<&str>,
            )?;
            let sep = PredefinedMenuItem::separator(handle)?;
            let help =
                Submenu::with_items(handle, "Help", true, &[&docs, &products, &sep, &about])?;
            #[cfg(target_os = "macos")]
            {
                // macOS convention: the first submenu is the app menu, and
                // quitting lives there as the predefined Quit item. Clear
                // Cache rides in the app menu, above Quit — the macOS
                // counterpart of its File-menu place on other platforms.
                let quit = PredefinedMenuItem::quit(handle, None::<&str>)?;
                let app_sep = PredefinedMenuItem::separator(handle)?;
                let app_menu = Submenu::with_items(
                    handle,
                    "Everyday Cool",
                    true,
                    &[&clear_cache, &app_sep, &quit],
                )?;
                Menu::with_items(handle, &[&app_menu, &help])
            }
            #[cfg(not(target_os = "macos"))]
            {
                let exit = MenuItem::with_id(handle, "exit", "Exit", true, None::<&str>)?;
                let file_sep = PredefinedMenuItem::separator(handle)?;
                let file = Submenu::with_items(
                    handle,
                    "File",
                    true,
                    &[&clear_cache, &file_sep, &exit],
                )?;
                Menu::with_items(handle, &[&file, &help])
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "exit" => app.exit(0),
            "clear-cache" => request_clear_cache(app),
            "docs" => {
                // The bundled documentation page, at the app root: resolve
                // it against the main window's current URL so the jump
                // works from any room. It is an app URL, so the navigation
                // handler below keeps it inside the window.
                if let Some(win) = app.get_webview_window("main") {
                    if let Ok(current) = win.url() {
                        if let Ok(target) = current.join("/docs.html") {
                            let _ = win.navigate(target);
                        }
                    }
                    let _ = win.set_focus();
                }
            }
            "products" => {
                // The product index lives on GitHub Pages; like every
                // external link in this app, it opens in the OS browser.
                let _ = app
                    .opener()
                    .open_url("https://caeled.github.io/everyday-cool/products/", None::<&str>);
            }
            "about" => {
                if let Some(win) = app.get_webview_window("about") {
                    let _ = win.set_focus();
                } else {
                    let version = app.package_info().version.to_string();
                    let _ = WebviewWindowBuilder::new(
                        app,
                        "about",
                        WebviewUrl::App("about.html".into()),
                    )
                    .title("About Everyday Cool")
                    .inner_size(430.0, 340.0)
                    .resizable(false)
                    .initialization_script(format!(
                        "window.__EC_VERSION = {version:?};"
                    ))
                    .build();
                }
            }
            _ => {}
        })
        .on_page_load(|webview, payload| {
            // The main window starts hidden behind the splash. Its first
            // finished load is one of the two conditions for the swap;
            // the other is the ten-second minimum (see setup), and the
            // splash comes down at whichever completes later.
            if webview.label() == "main"
                && matches!(payload.event(), PageLoadEvent::Finished)
            {
                MAIN_LOADED.store(true, std::sync::atomic::Ordering::SeqCst);
                maybe_dismiss_splash(&webview.app_handle());
            }
        })
        .setup(|app| {
            // Clear Cache lands here, before any window — and therefore
            // any webview — exists in this process: if the previous run
            // left the flag, delete the profile's cache directories now
            // (nothing holds their files yet) and arm the notice that
            // tells the user it happened once the main window shows.
            if consume_clear_cache_flag(app.handle()) {
                CACHE_JUST_CLEARED.store(true, std::sync::atomic::Ordering::SeqCst);
            }

            // The splash goes up first: artwork full-bleed, the title set
            // as real HTML type over it, the version in the corner (the
            // page reads it from the initialization script, so it can
            // never disagree with the build).
            let version = app.handle().package_info().version.to_string();
            WebviewWindowBuilder::new(app, "splashscreen", WebviewUrl::App("splash.html".into()))
                .title("Everyday Cool")
                .inner_size(768.0, 512.0)
                .decorations(false)
                .resizable(false)
                .center()
                .always_on_top(true)
                .skip_taskbar(true)
                .initialization_script(format!("window.__EC_VERSION = {version:?};"))
                .build()?;

            // Minimum display: ten seconds after launch the minimum is
            // met; if the main window has already loaded, the splash
            // comes down now, otherwise it comes down when the load
            // finishes (see on_page_load).
            let min_timer = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(10));
                MIN_TIME_PASSED.store(true, std::sync::atomic::Ordering::SeqCst);
                let h = min_timer.clone();
                let _ = min_timer.run_on_main_thread(move || maybe_dismiss_splash(&h));
            });

            // Hard cap: whatever happens to the load event, the splash
            // comes down unconditionally fifteen seconds after launch,
            // so it can never trap the user.
            let cap_timer = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(15));
                let h = cap_timer.clone();
                let _ = cap_timer.run_on_main_thread(move || dismiss_splash(&h));
            });

            let nav_handle = app.handle().clone();
            let new_window_handle = app.handle().clone();
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("Everyday Cool — The Whole Workshop")
                .inner_size(1280.0, 900.0)
                .visible(false)
                .on_navigation(move |url| {
                    if is_app_url(url) {
                        // The course itself: its pages (index, discovery,
                        // START-HERE) and in-page lesson anchors resolve to
                        // the app's own URLs and stay inside the window.
                        true
                    } else if url.scheme() == "http" || url.scheme() == "https" {
                        // An outside page: a documentation link, a
                        // Discovery Room recording, or any other external
                        // destination. Cancel the in-app navigation and
                        // hand the URL to the OS default browser instead.
                        // The course stays exactly where it was.
                        let _ = nav_handle.opener().open_url(url.as_str(), None::<&str>);
                        false
                    } else {
                        true
                    }
                })
                .on_new_window(move |url, _features| {
                    // An external link that asks for its own window gets
                    // the same treatment as an external navigation: never
                    // open a second in-app window; send the URL to the
                    // OS default browser.
                    if url.scheme() == "http" || url.scheme() == "https" {
                        let _ = new_window_handle
                            .opener()
                            .open_url(url.as_str(), None::<&str>);
                    }
                    tauri::webview::NewWindowResponse::Deny
                })
                .on_download(|_webview, _event| {
                    // Downloads are the course's own business: "Save
                    // progress to USB" exports a JSON file, and the
                    // workbook is a PDF. Allow every download with the
                    // platform's default behavior instead of blocking it.
                    true
                })
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Everyday Cool");
}
