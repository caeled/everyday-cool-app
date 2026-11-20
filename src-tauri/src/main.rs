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

/// ------------------------------------------------------------------
/// User profiles (new in v1.2.0). Steven's order: "the whole thing
/// needs a way to save a 'user' so multiple people can access from
/// one machine" — a family or classroom machine where each person
/// keeps their own place in every room.
///
/// Profiles live entirely at the app level. The app keeps a small
/// registry (users.json in the app local data directory, OUTSIDE any
/// webview data folder): a display name and an id per user. No
/// passwords or PINs — a name is just a label, like picking a seat
/// at the bench. Each user gets a separate webview data store, so
/// every room's local storage, IndexedDB, and cache are per-user
/// with zero changes to any room's code:
///
/// - The first user, "Default", is the data the app already had: on
///   Windows that is the existing EBWebView profile under the app
///   local data directory, adopted unchanged and in place. The main
///   window for Default gets NO data-store override, so its storage
///   path is exactly the pre-profiles one and nothing moves.
/// - Every later user gets a fresh store: a `profiles/<id>` data
///   directory on Windows/Linux. WKWebView has no data directories,
///   so on macOS each later user instead gets a per-user website
///   data store identifier (available on macOS 14+; on older macOS
///   the store falls back to the shared default, so profiles there
///   are labels without storage isolation).
///
/// Startup: with exactly one user the workshop opens directly, as it
/// always has. With two or more — or after File → Switch User — a
/// picker window (dist/users.html, an app-only page) comes first:
/// name tiles, an add-user form, and per-user rename/delete. The
/// page never calls a Tauri API; its tiles and forms point at a fake
/// "ec-action" path, the navigation is intercepted, and Rust acts.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct UserProfile {
    id: String,
    name: String,
    created: u64,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct UserRegistry {
    version: u32,
    last_used: String,
    users: Vec<UserProfile>,
}

const DEFAULT_USER_ID: &str = "default";
const USERS_FILE: &str = "users.json";
const SHOW_PICKER_FLAG: &str = "show-picker.flag";
const PICKED_FLAG: &str = "picked.flag";

fn users_file_path(handle: &AppHandle) -> Option<std::path::PathBuf> {
    handle
        .path()
        .app_local_data_dir()
        .ok()
        .map(|dir| dir.join(USERS_FILE))
}

fn fresh_registry() -> UserRegistry {
    UserRegistry {
        version: 1,
        last_used: DEFAULT_USER_ID.to_string(),
        users: vec![UserProfile {
            id: DEFAULT_USER_ID.to_string(),
            name: "Default".to_string(),
            created: 0,
        }],
    }
}

/// Load the user registry, creating (and saving) a fresh one — a
/// single "Default" user adopting the app's existing webview data —
/// when the file is missing or unreadable. A registry that lost its
/// Default user, or whose last_used points at a user who no longer
/// exists, is repaired rather than trusted.
fn load_registry(handle: &AppHandle) -> UserRegistry {
    let mut registry = users_file_path(handle)
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<UserRegistry>(&text).ok())
        .unwrap_or_else(fresh_registry);
    let mut repaired = false;
    if registry.users.is_empty() {
        registry.users = fresh_registry().users;
        repaired = true;
    }
    if !registry.users.iter().any(|u| u.id == DEFAULT_USER_ID) {
        registry.users.insert(
            0,
            UserProfile {
                id: DEFAULT_USER_ID.to_string(),
                name: "Default".to_string(),
                created: 0,
            },
        );
        repaired = true;
    }
    if !registry.users.iter().any(|u| u.id == registry.last_used) {
        registry.last_used = registry.users[0].id.clone();
        repaired = true;
    }
    if repaired {
        save_registry(handle, &registry);
    }
    registry
}

fn save_registry(handle: &AppHandle, registry: &UserRegistry) {
    if let Some(path) = users_file_path(handle) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(registry) {
            let _ = std::fs::write(path, text);
        }
    }
}

/// The user whose store the main window opens with: the last one
/// used, as recorded in the registry.
fn active_user(registry: &UserRegistry) -> UserProfile {
    registry
        .users
        .iter()
        .find(|u| u.id == registry.last_used)
        .cloned()
        .unwrap_or_else(|| registry.users[0].clone())
}

/// The webview data directory for a user id, or None for Default —
/// Default keeps the platform's own location (the existing EBWebView
/// profile on Windows), so no override is applied and nothing moves.
/// Only meaningful on platforms with real data directories; macOS
/// uses store identifiers instead (see user_store_uuid).
fn user_data_dir(handle: &AppHandle, user_id: &str) -> Option<std::path::PathBuf> {
    if user_id == DEFAULT_USER_ID {
        return None;
    }
    handle
        .path()
        .app_local_data_dir()
        .ok()
        .map(|dir| dir.join("profiles").join(user_id))
}

/// A stable 16-byte website data store identifier for a user on
/// macOS, derived from the user id with two FNV-1a passes and shaped
/// like a version-4 UUID. Deterministic across launches, distinct
/// per user.
#[cfg(target_os = "macos")]
fn user_store_uuid(id: &str) -> [u8; 16] {
    fn fnv1a(seed: u64, bytes: &[u8]) -> u64 {
        let mut hash = seed;
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    }
    let bytes = id.as_bytes();
    let hi = fnv1a(0xcbf2_9ce4_8422_2325, bytes);
    let lo = fnv1a(0x8422_2325_cbf2_9ce4, bytes);
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&hi.to_be_bytes());
    out[8..].copy_from_slice(&lo.to_be_bytes());
    out[6] = (out[6] & 0x0f) | 0x40; // version 4
    out[8] = (out[8] & 0x3f) | 0x80; // variant 1
    out
}

/// Tidy a user-supplied profile name: trimmed, control characters
/// dropped, capped at forty characters. Names are display labels
/// only — folders and stores key on the id — so this is about not
/// letting a label wreck the picker page, not about path safety.
fn sanitize_user_name(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(40)
        .collect();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

/// A fresh user id ("u-" plus eight hex digits), unique in the
/// registry. Seeded from the clock; a counter walks until unique.
fn new_user_id(registry: &UserRegistry) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut counter = 0u64;
    loop {
        let mixed = nanos ^ counter.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        let id = format!("u-{:08x}", (mixed >> 16) as u32);
        if !registry.users.iter().any(|u| u.id == id) {
            return id;
        }
        counter += 1;
    }
}

/// The state handed to dist/users.html: the user list with the flags
/// the page needs to render its tiles and controls. In "pick" mode
/// the page is the startup picker; in "add" mode it is the
/// File → Add User window and only the form is shown.
#[derive(serde::Serialize)]
struct PickerUser {
    id: String,
    name: String,
    is_default: bool,
    is_last_used: bool,
    can_delete: bool,
}

#[derive(serde::Serialize)]
struct PickerState {
    mode: String,
    users: Vec<PickerUser>,
}

fn picker_state(registry: &UserRegistry, mode: &str) -> PickerState {
    let many = registry.users.len() > 1;
    PickerState {
        mode: mode.to_string(),
        users: registry
            .users
            .iter()
            .map(|u| PickerUser {
                id: u.id.clone(),
                name: u.name.clone(),
                is_default: u.id == DEFAULT_USER_ID,
                is_last_used: u.id == registry.last_used,
                // Deleting is offered only in the picker, and never
                // for Default (its store is the app's original
                // profile), never for the last-used user (switch to
                // someone else first), and never for the only user.
                can_delete: many && u.id != DEFAULT_USER_ID && u.id != registry.last_used,
            })
            .collect(),
    }
}

fn picker_state_json(registry: &UserRegistry, mode: &str) -> String {
    serde_json::to_string(&picker_state(registry, mode)).unwrap_or_else(|_| "{}".to_string())
}

/// Push fresh state into an open users.html window after the
/// registry changed underneath it (add / rename / delete).
fn refresh_users_window(handle: &AppHandle, label: &str, registry: &UserRegistry, mode: &str) {
    if let Some(win) = handle.get_webview_window(label) {
        let state = picker_state_json(registry, mode);
        let _ = win.eval(format!("if (window.__ecState) window.__ecState({state});"));
    }
}

/// A small in-page notice on a window, same dress as the Clear Cache
/// notice below, with the message safely quoted as a JSON string.
fn notice_js(message: &str) -> String {
    let text = serde_json::to_string(message).unwrap_or_else(|_| "\"\"".to_string());
    let mut js = String::from(
        "(() => { const d = document.createElement('div'); d.textContent = ",
    );
    js.push_str(&text);
    js.push_str("; d.style.cssText = 'position:fixed;top:14px;left:50%;transform:translateX(-50%);z-index:99999;background:#1d3b2a;color:#eaf5ec;padding:10px 18px;border-radius:10px;font:14px system-ui,sans-serif;box-shadow:0 4px 18px rgba(0,0,0,.35)'; document.body.appendChild(d); setTimeout(() => d.remove(), 8000); })();");
    js
}

/// One action from a users.html window, parsed out of the fake
/// "ec-action" navigation the page's tiles and forms point at.
enum UserAction {
    Pick(String),
    Add(String),
    Delete(String),
    Rename(String, String),
}

fn parse_user_action(url: &tauri::Url) -> Option<UserAction> {
    if url.path() != "/ec-action" {
        return None;
    }
    let mut pick = None;
    let mut add = None;
    let mut delete = None;
    let mut rename = None;
    let mut name = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "pick" => pick = Some(value.into_owned()),
            "add" => add = Some(value.into_owned()),
            "del" => delete = Some(value.into_owned()),
            "rename" => rename = Some(value.into_owned()),
            "name" => name = Some(value.into_owned()),
            _ => {}
        }
    }
    if let Some(id) = pick {
        Some(UserAction::Pick(id))
    } else if let Some(raw) = add {
        Some(UserAction::Add(raw))
    } else if let Some(id) = delete {
        Some(UserAction::Delete(id))
    } else if let (Some(id), Some(raw)) = (rename, name) {
        Some(UserAction::Rename(id, raw))
    } else {
        None
    }
}

/// Act on a startup-picker action. Runs on the main thread, deferred
/// out of the navigation callback that intercepted it.
fn handle_picker_action(handle: &AppHandle, action: UserAction) {
    match action {
        UserAction::Pick(id) => {
            // Bookkeeping only — then restart, and let setup build
            // the windows. Field failure (v1.3.0, Steven's machine):
            // this branch used to close the picker and call
            // launch_main right here, synchronously inside the
            // closure the navigation intercept dispatched — closing
            // the very webview whose navigation the main thread had
            // just handled, then building the splash and main
            // WebView2 windows on that same thread, wedged it for
            // good: frame and menus painted, workshop blank white,
            // "(Not Responding)", and even the splash's fifteen-
            // second hard cap could never fire, because the cap is
            // itself a post to the blocked thread. Windows are only
            // ever built from setup — the one context that has
            // worked in every version — so a pick now takes the
            // flag-and-restart road Switch User and Clear Cache
            // already use: record the pick as last_used, leave the
            // picked flag, restart, and setup opens the workshop
            // directly for that user (see consume_picked_flag).
            let mut registry = load_registry(handle);
            if !registry.users.iter().any(|u| u.id == id) {
                return;
            }
            registry.last_used = id;
            save_registry(handle, &registry);
            write_flag(handle, PICKED_FLAG);
            handle.restart();
        }
        UserAction::Add(raw) => {
            if let Some(name) = sanitize_user_name(&raw) {
                let mut registry = load_registry(handle);
                let created = registry.users.iter().map(|u| u.created).max().unwrap_or(0) + 1;
                registry.users.push(UserProfile {
                    id: new_user_id(&registry),
                    name,
                    created,
                });
                save_registry(handle, &registry);
                refresh_users_window(handle, "picker", &registry, "pick");
            }
        }
        UserAction::Delete(id) => {
            let mut registry = load_registry(handle);
            let deletable = registry.users.len() > 1
                && id != DEFAULT_USER_ID
                && id != registry.last_used
                && registry.users.iter().any(|u| u.id == id);
            if deletable {
                registry.users.retain(|u| u.id != id);
                save_registry(handle, &registry);
                // Remove the user's data folder (the Windows/Linux
                // data directory). On macOS the per-user store is a
                // WKWebView website data store with no folder to
                // delete; the registry entry is gone, which is what
                // the picker shows — the orphaned store is the
                // system's to keep.
                #[cfg(not(target_os = "macos"))]
                {
                    if let Some(dir) = user_data_dir(handle, &id) {
                        let _ = std::fs::remove_dir_all(dir);
                    }
                }
                refresh_users_window(handle, "picker", &registry, "pick");
            }
        }
        UserAction::Rename(id, raw) => {
            if let Some(name) = sanitize_user_name(&raw) {
                let mut registry = load_registry(handle);
                if let Some(user) = registry.users.iter_mut().find(|u| u.id == id) {
                    user.name = name;
                }
                save_registry(handle, &registry);
                // No menu update here: the File menu's "Current
                // user" label is read from the registry when the
                // menu is built at startup, so a rename shows there
                // from the next launch. Mutating a menu item from
                // inside a picker callback was part of the v1.3.0
                // hang class and is not done anymore.
                refresh_users_window(handle, "picker", &registry, "pick");
            }
        }
    }
}

/// Act on the File → Add User window's one action. Adding a user
/// never restarts anything: the registry gains an entry, the window
/// closes, and the workshop (or the picker behind it) hears about it.
fn handle_add_user_action(handle: &AppHandle, action: UserAction) {
    if let UserAction::Add(raw) = action {
        if let Some(name) = sanitize_user_name(&raw) {
            let mut registry = load_registry(handle);
            let created = registry.users.iter().map(|u| u.created).max().unwrap_or(0) + 1;
            registry.users.push(UserProfile {
                id: new_user_id(&registry),
                name: name.clone(),
                created,
            });
            save_registry(handle, &registry);
            if let Some(win) = handle.get_webview_window("adduser") {
                let _ = win.close();
            }
            refresh_users_window(handle, "picker", &registry, "pick");
            if let Some(main) = handle.get_webview_window("main") {
                let _ = main.eval(notice_js(&format!(
                    "User '{name}' added. Switch User in the File menu takes you to them."
                )));
            }
        }
    }
}

/// Build the startup picker window (dist/users.html in "pick" mode).
fn build_picker(handle: &AppHandle, registry: &UserRegistry) -> tauri::Result<()> {
    let state = picker_state_json(registry, "pick");
    let nav = handle.clone();
    WebviewWindowBuilder::new(handle, "picker", WebviewUrl::App("users.html".into()))
        .title("Everyday Cool \u{2014} Who's using it?")
        .inner_size(480.0, 620.0)
        .resizable(false)
        .center()
        .initialization_script(format!("window.__EC_PICKER = {state};"))
        .on_navigation(move |url| match parse_user_action(url) {
            Some(action) => {
                let h = nav.clone();
                let _ = nav.run_on_main_thread(move || handle_picker_action(&h, action));
                false
            }
            None => true,
        })
        .build()?;
    Ok(())
}

/// Build (or focus) the File → Add User window: the same app-only
/// page in "add" mode — just the form.
fn build_add_user_window(handle: &AppHandle) {
    if let Some(win) = handle.get_webview_window("adduser") {
        let _ = win.set_focus();
        return;
    }
    let registry = load_registry(handle);
    let state = picker_state_json(&registry, "add");
    let nav = handle.clone();
    let _ = WebviewWindowBuilder::new(handle, "adduser", WebviewUrl::App("users.html".into()))
        .title("Add a user \u{2014} Everyday Cool")
        .inner_size(480.0, 430.0)
        .resizable(false)
        .center()
        .initialization_script(format!("window.__EC_PICKER = {state};"))
        .on_navigation(move |url| match parse_user_action(url) {
            Some(action) => {
                let h = nav.clone();
                let _ = nav.run_on_main_thread(move || handle_add_user_action(&h, action));
                false
            }
            None => true,
        })
        .build();
}

/// Path of a small flag file in the app local data directory (the
/// Clear Cache flag, the show-picker flag, and the picked flag live
/// side by side).
fn flag_path(handle: &AppHandle, name: &str) -> Option<std::path::PathBuf> {
    handle
        .path()
        .app_local_data_dir()
        .ok()
        .map(|dir| dir.join(name))
}

fn write_flag(handle: &AppHandle, name: &str) {
    if let Some(flag) = flag_path(handle, name) {
        if let Some(parent) = flag.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&flag, b"1");
    }
}

/// Consume the picked flag left by a pick in the startup picker
/// (see the Pick branch of handle_picker_action). Setup checks it
/// before the picker decision: the pick restarted the app, and this
/// process opens the workshop directly for the picked user — who is
/// the registry's last_used — instead of showing the picker again.
fn consume_picked_flag(handle: &AppHandle) -> bool {
    match flag_path(handle, PICKED_FLAG) {
        Some(flag) if flag.exists() => {
            let _ = std::fs::remove_file(&flag);
            true
        }
        _ => false,
    }
}

/// Consume the show-picker flag left by File → Switch User. The
/// picker also appears whenever two or more users exist; the flag
/// forces it even for a single user, so Switch User always lands
/// somewhere meaningful.
fn consume_show_picker_flag(handle: &AppHandle) -> bool {
    match flag_path(handle, SHOW_PICKER_FLAG) {
        Some(flag) if flag.exists() => {
            let _ = std::fs::remove_file(&flag);
            true
        }
        _ => false,
    }
}

/// File → Switch User: leave the flag, then restart. Setup of the
/// new process sees it and opens the picker before the workshop.
fn request_switch_user(handle: &AppHandle) {
    write_flag(handle, SHOW_PICKER_FLAG);
    handle.restart();
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

/// File → Clear Cache: leave the flag for the next launch, then
/// restart. The deletion happens in setup of the new process, before
/// its webview is created (see consume_clear_cache_flag).
fn request_clear_cache(handle: &AppHandle) {
    write_flag(handle, CLEAR_CACHE_FLAG);
    handle.restart();
}

/// Consume a Clear Cache flag left by the previous run. Returns true
/// when a flag was present (the caller then arms the "cache cleared"
/// notice for the main window). Deletion is best-effort with a short
/// retry: the previous process may still be releasing file locks in
/// the first moments after a restart.
///
/// Since v1.2.0 the flag clears the ACTIVE user's caches only (the
/// user who clicked the item — the restart keeps them last_used):
/// Default's profile is the original EBWebView one; every other
/// user's is their own `profiles/<id>` data directory.
fn consume_clear_cache_flag(handle: &AppHandle, user: &UserProfile) -> bool {
    let Some(flag) = flag_path(handle, CLEAR_CACHE_FLAG) else {
        return false;
    };
    if !flag.exists() {
        return false;
    }
    let _ = std::fs::remove_file(&flag);

    let mut targets: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(data_dir) = handle.path().app_local_data_dir() {
        // The WebView2 profile's Default folder: for the Default user
        // it lives under <app local data>/EBWebView; for any other
        // user, under their own data directory. Only cache
        // directories are listed — Local Storage and IndexedDB live
        // in sibling directories and are preserved.
        let profile = match user_data_dir(handle, &user.id) {
            Some(dir) => dir.join("Default"),
            None => data_dir.join("EBWebView").join("Default"),
        };
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

/// Build the splash and the main window for `user`, with the main
/// window's webview pointed at the user's own data store (see the
/// user-profiles notes at the top of this file). Called only from
/// setup: directly when no picker is needed, and on the restart
/// that follows a pick (the picked flag). Never called from a
/// picker callback — see the Pick branch of handle_picker_action
/// for the hang that taught that. Default gets no store override
/// at all — its window is built exactly as it was before profiles
/// existed.
fn launch_main(handle: &AppHandle, user: &UserProfile) -> tauri::Result<()> {
    // The splash goes up first: artwork full-bleed, the title set
    // as real HTML type over it, the version in the corner (the
    // page reads it from the initialization script, so it can
    // never disagree with the build).
    let version = handle.package_info().version.to_string();
    WebviewWindowBuilder::new(handle, "splashscreen", WebviewUrl::App("splash.html".into()))
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
    let min_timer = handle.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(10));
        MIN_TIME_PASSED.store(true, std::sync::atomic::Ordering::SeqCst);
        let h = min_timer.clone();
        let _ = min_timer.run_on_main_thread(move || maybe_dismiss_splash(&h));
    });

    // Hard cap: whatever happens to the load event, the splash
    // comes down unconditionally fifteen seconds after launch,
    // so it can never trap the user.
    let cap_timer = handle.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(15));
        let h = cap_timer.clone();
        let _ = cap_timer.run_on_main_thread(move || dismiss_splash(&h));
    });

    let nav_handle = handle.clone();
    let new_window_handle = handle.clone();
    let mut builder = WebviewWindowBuilder::new(handle, "main", WebviewUrl::App("index.html".into()))
        .title("Everyday Cool \u{2014} The Whole Workshop")
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
        });
    // The user's own data store (profiles): Default keeps the
    // platform default; everyone else gets their own directory —
    // or, on macOS, their own website data store identifier.
    #[cfg(not(target_os = "macos"))]
    {
        if let Some(dir) = user_data_dir(handle, &user.id) {
            let _ = std::fs::create_dir_all(&dir);
            builder = builder.data_directory(dir);
        }
    }
    #[cfg(target_os = "macos")]
    {
        if user.id != DEFAULT_USER_ID {
            builder = builder.data_store_identifier(user_store_uuid(&user.id));
        }
    }
    builder.build()?;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .menu(|handle| {
            // User profiles (v1.2.0): a disabled label naming the
            // active user, plus Switch User and Add User. The label
            // is read from the registry as the menu is built and is
            // never mutated afterwards — a pick or rename lands in
            // the registry and shows here from the next launch. The
            // registry is read here — before setup — purely for the
            // label; setup loads it again for real.
            let current_name = active_user(&load_registry(handle)).name;
            let current_user = MenuItem::with_id(
                handle,
                "current-user",
                format!("Current user: {current_name}"),
                false,
                None::<&str>,
            )?;
            let switch_user =
                MenuItem::with_id(handle, "switch-user", "Switch User", true, None::<&str>)?;
            let add_user =
                MenuItem::with_id(handle, "add-user", "Add User", true, None::<&str>)?;
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
                // quitting lives there as the predefined Quit item. The
                // user items and Clear Cache ride in the app menu, above
                // Quit — the macOS counterpart of their File-menu places
                // on other platforms.
                let quit = PredefinedMenuItem::quit(handle, None::<&str>)?;
                let app_sep = PredefinedMenuItem::separator(handle)?;
                let app_sep2 = PredefinedMenuItem::separator(handle)?;
                let app_sep3 = PredefinedMenuItem::separator(handle)?;
                let app_menu = Submenu::with_items(
                    handle,
                    "Everyday Cool",
                    true,
                    &[
                        &current_user,
                        &app_sep,
                        &switch_user,
                        &add_user,
                        &app_sep2,
                        &clear_cache,
                        &app_sep3,
                        &quit,
                    ],
                )?;
                Menu::with_items(handle, &[&app_menu, &help])
            }
            #[cfg(not(target_os = "macos"))]
            {
                let exit = MenuItem::with_id(handle, "exit", "Exit", true, None::<&str>)?;
                let file_sep = PredefinedMenuItem::separator(handle)?;
                let file_sep2 = PredefinedMenuItem::separator(handle)?;
                let file_sep3 = PredefinedMenuItem::separator(handle)?;
                let file = Submenu::with_items(
                    handle,
                    "File",
                    true,
                    &[
                        &current_user,
                        &file_sep,
                        &switch_user,
                        &add_user,
                        &file_sep2,
                        &clear_cache,
                        &file_sep3,
                        &exit,
                    ],
                )?;
                Menu::with_items(handle, &[&file, &help])
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "exit" => app.exit(0),
            "switch-user" => request_switch_user(app),
            "add-user" => build_add_user_window(app),
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
            let handle = app.handle();
            // The user registry first: everything in this launch —
            // the Clear Cache target below, and the data store the
            // main window will open with — hangs off who is active.
            let registry = load_registry(handle);
            let active = active_user(&registry);

            // Clear Cache lands here, before any window — and therefore
            // any webview — exists in this process: if the previous run
            // left the flag, delete the active user's cache directories
            // now (nothing holds their files yet) and arm the notice
            // that tells the user it happened once the main window shows.
            if consume_clear_cache_flag(handle, &active) {
                CACHE_JUST_CLEARED.store(true, std::sync::atomic::Ordering::SeqCst);
            }

            // A pick in the picker restarts the app with the picked
            // flag set (see handle_picker_action): honour it before
            // the picker decision and open the workshop directly for
            // the picked user — who is `active`, since the pick
            // recorded them as last_used — even when two or more
            // users exist. Windows are built only here, in setup,
            // never from a picker callback.
            //
            // Otherwise: two or more users — or a Switch User
            // restart — means the picker comes first. With a single
            // user there is nothing to pick: open directly, as the
            // app always has.
            if consume_picked_flag(handle) {
                launch_main(handle, &active)?;
            } else if consume_show_picker_flag(handle) || registry.users.len() > 1 {
                build_picker(handle, &registry)?;
            } else {
                launch_main(handle, &active)?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Everyday Cool");
}
