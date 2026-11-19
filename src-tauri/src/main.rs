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
                // quitting lives there as the predefined Quit item.
                let quit = PredefinedMenuItem::quit(handle, None::<&str>)?;
                let app_menu = Submenu::with_items(handle, "Everyday Cool", true, &[&quit])?;
                Menu::with_items(handle, &[&app_menu, &help])
            }
            #[cfg(not(target_os = "macos"))]
            {
                let exit = MenuItem::with_id(handle, "exit", "Exit", true, None::<&str>)?;
                let file = Submenu::with_items(handle, "File", true, &[&exit])?;
                Menu::with_items(handle, &[&file, &help])
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "exit" => app.exit(0),
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
