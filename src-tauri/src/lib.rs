mod engine;
#[cfg(target_os = "macos")]
mod mac;
mod settings;
mod windows;

use engine::{Control, Engine};
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use settings::{Appearance, Settings};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Theme, WebviewWindow, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_updater::UpdaterExt;

const PANEL: &str = "panel";

pub struct Shared {
    pub settings: RwLock<Settings>,
    pub engine: Mutex<Engine>,
    pub mouse_engine: Mutex<Engine>,
    pub windows: Mutex<windows::WindowGestures>,
    app: OnceLock<AppHandle>,
    pending_update: Mutex<Option<String>>,
}

impl Shared {
    pub fn app(&self) -> Option<AppHandle> {
        self.app.get().cloned()
    }

    /// Brink's level HUD with the percentage.
    #[cfg(target_os = "macos")]
    pub fn show_hud(&self, h: mac::levels::Hud) {
        if let Some(app) = self.app.get() {
            mac::hud::show(app, h);
        }
    }

    /// Runs `f` on the main thread and waits for it (briefly). For AppKit calls from the
    /// trackpad threads.
    pub fn on_main<R: Send + 'static>(&self, f: impl FnOnce() -> R + Send + 'static) -> Option<R> {
        let app = self.app.get()?;
        let (tx, rx) = std::sync::mpsc::channel();
        app.run_on_main_thread(move || {
            let _ = tx.send(f());
        })
        .ok()?;
        rx.recv_timeout(Duration::from_millis(500)).ok()
    }

    /// Live finger position on an edge for the panel's trackpad mirror (~30 fps).
    pub fn notify_edge(&self, c: Option<(Control, f32)>) {
        static LAST: AtomicU64 = AtomicU64::new(0);
        let now = ms();
        if c.is_some() && now.saturating_sub(LAST.load(Ordering::Relaxed)) < 33 {
            return;
        }
        LAST.store(now, Ordering::Relaxed);
        if let Some(app) = self.app.get() {
            let payload = c.map(|(control, y)| serde_json::json!({ "control": control, "y": y }));
            let _ = app.emit("brink://edge", payload);
        }
    }

    pub fn notify_corner(&self, k: engine::Corner) {
        if let Some(app) = self.app.get() {
            let _ = app.emit("brink://corner", k);
        }
    }

    pub fn notify_levels(&self) {
        // Throttled: a fast slide posts dozens of keys a second.
        static LAST: AtomicU64 = AtomicU64::new(0);
        let now = START.get_or_init(Instant::now).elapsed().as_millis() as u64;
        if now.saturating_sub(LAST.load(Ordering::Relaxed)) < 60 {
            return;
        }
        LAST.store(now, Ordering::Relaxed);
        if let Some(app) = self.app.get() {
            let _ = app.emit("brink://levels", levels());
        }
    }
}

static START: OnceLock<Instant> = OnceLock::new();

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Levels {
    volume: Option<f32>,
    brightness: Option<f32>,
    muted: bool,
}

fn levels() -> Levels {
    #[cfg(target_os = "macos")]
    {
        Levels { volume: mac::system::volume(), brightness: mac::system::brightness(), muted: mac::system::muted() }
    }
    #[cfg(not(target_os = "macos"))]
    Levels { volume: None, brightness: None, muted: false }
}

fn trusted() -> bool {
    #[cfg(target_os = "macos")]
    return mac::system::trusted();
    #[cfg(not(target_os = "macos"))]
    false
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppState {
    settings: Settings,
    levels: Levels,
    trusted: bool,
    version: String,
    update: Option<String>,
}

#[tauri::command]
fn get_state(app: AppHandle, shared: tauri::State<'_, Arc<Shared>>) -> AppState {
    AppState {
        settings: shared.settings.read().clone(),
        levels: levels(),
        trusted: trusted(),
        version: app.package_info().version.to_string(),
        update: shared.pending_update.lock().clone(),
    }
}

/// The desktop picture, for the panel's liquid-glass backdrop. Empty when unavailable.
#[tauri::command]
async fn wallpaper() -> tauri::ipc::Response {
    #[cfg(target_os = "macos")]
    let bytes = mac::system::wallpaper_jpeg().unwrap_or_default();
    #[cfg(not(target_os = "macos"))]
    let bytes = Vec::new();
    tauri::ipc::Response::new(bytes)
}

/// Where the panel sits on its screen, in points from the screen's top-left, so the page can
/// crop the wallpaper to exactly what is behind it.
#[derive(Serialize, Clone, Copy, Default)]
struct Placement {
    x: f64,
    y: f64,
    sw: f64,
    sh: f64,
}
static PLACEMENT: Mutex<Option<Placement>> = Mutex::new(None);

#[tauri::command]
fn placement() -> Option<Placement> {
    *PLACEMENT.lock()
}

#[tauri::command]
fn get_levels() -> Levels {
    levels()
}

#[tauri::command]
fn set_settings(app: AppHandle, shared: tauri::State<'_, Arc<Shared>>, patch: serde_json::Value) -> Result<Settings, String> {
    let before = shared.settings.read().clone();
    let mut merged = serde_json::to_value(&before).map_err(|e| e.to_string())?;
    if let (Some(dst), Some(src)) = (merged.as_object_mut(), patch.as_object()) {
        for (k, v) in src {
            dst.insert(k.clone(), v.clone());
        }
    }
    let next: Settings = serde_json::from_value::<Settings>(merged).map_err(|e| e.to_string())?.clamped();
    next.save();
    *shared.settings.write() = next.clone();

    if before.open_at_login != next.open_at_login {
        let al = app.autolaunch();
        let r = if next.open_at_login { al.enable() } else { al.disable() };
        if let Err(e) = r {
            log::warn!("autostart: {e}");
        }
    }
    if before.appearance != next.appearance {
        apply_appearance(&app, next.appearance);
    }
    if before.enabled && !next.enabled {
        #[cfg(target_os = "macos")]
        {
            let mut effects = shared.engine.lock().reset();
            effects.extend(shared.mouse_engine.lock().reset());
            mac::input::apply(&shared, effects);
        }
    }
    Ok(next)
}

fn apply_appearance(app: &AppHandle, a: Appearance) {
    if let Some(w) = app.get_webview_window(PANEL) {
        let _ = w.set_theme(Some(if a == Appearance::Dark { Theme::Dark } else { Theme::Light }));
    }
}

#[tauri::command]
fn request_access() -> bool {
    #[cfg(target_os = "macos")]
    {
        let ok = mac::system::request_trust();
        if !ok {
            let _ = std::process::Command::new("open")
                .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
                .spawn();
        }
        ok
    }
    #[cfg(not(target_os = "macos"))]
    false
}

#[tauri::command]
fn open_url(url: String) {
    if url.starts_with("https://") || url.starts_with("mailto:") {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
}

#[tauri::command]
fn set_panel_height(app: AppHandle, height: f64) {
    if let Some(w) = app.get_webview_window(PANEL) {
        let _ = w.set_size(tauri::LogicalSize::new(320.0, height.clamp(200.0, 760.0)));
    }
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

/// Checks for an update and, if there is one, downloads and installs it in place. The new
/// version runs after a restart (from the panel, the notification, or the next launch).
/// `notify` posts a macOS notification for updates found in the background.
async fn check_and_install(app: AppHandle, notify: bool) -> Result<Option<String>, String> {
    let shared = app.state::<Arc<Shared>>().inner().clone();
    if let Some(v) = shared.pending_update.lock().clone() {
        return Ok(Some(v));
    }
    let updater = app.updater().map_err(|e| e.to_string())?;
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let version = update.version.clone();
    update.download_and_install(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
    *shared.pending_update.lock() = Some(version.clone());
    let _ = app.emit("brink://update", &version);
    #[cfg(target_os = "macos")]
    if notify {
        mac::notify::update_ready(&version);
    }
    Ok(Some(version))
}

#[tauri::command]
async fn check_update(app: AppHandle) -> Result<Option<String>, String> {
    check_and_install(app, false).await
}

#[tauri::command]
fn restart(app: AppHandle) {
    app.restart();
}

/// True when the panel sits on NSGlassEffectView (macOS 26), false for the older blur.
static LAST_HIDE: AtomicU64 = AtomicU64::new(0);
static PANEL_OPEN: AtomicBool = AtomicBool::new(false);

fn ms() -> u64 {
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

fn hide_panel(w: &WebviewWindow) {
    if PANEL_OPEN.swap(false, Ordering::SeqCst) {
        LAST_HIDE.store(ms(), Ordering::SeqCst);
    }
    let _ = w.hide();
    let _ = w.emit("brink://visible", false);
}

fn toggle_panel(app: &AppHandle, anchor: Option<tauri::Rect>) {
    let Some(w) = app.get_webview_window(PANEL) else { return };
    if PANEL_OPEN.load(Ordering::SeqCst) {
        hide_panel(&w);
        return;
    }
    // A click on the icon first blurs the open panel; don't reopen it straight away.
    if ms().saturating_sub(LAST_HIDE.load(Ordering::SeqCst)) < 250 {
        return;
    }
    if let Some(rect) = anchor {
        let scale = w.scale_factor().unwrap_or(2.0);
        let pos = rect.position.to_physical::<f64>(scale);
        let size = rect.size.to_physical::<f64>(scale);
        let width = w.outer_size().map(|s| s.width as f64).unwrap_or(320.0 * scale);
        let mut x = pos.x + size.width / 2.0 - width / 2.0;
        if let Ok(Some(m)) = w.current_monitor().or_else(|_| w.primary_monitor()) {
            let left = m.position().x as f64 + 8.0 * scale;
            let right = m.position().x as f64 + m.size().width as f64 - width - 8.0 * scale;
            x = x.clamp(left, right.max(left));
        }
        let y = pos.y + size.height + 6.0 * scale;
        let _ = w.set_position(PhysicalPosition::new(x, y));
        if let Ok(Some(m)) = w.current_monitor().or_else(|_| w.primary_monitor()) {
            let ms = m.scale_factor();
            let p = Placement {
                x: (x - m.position().x as f64) / ms,
                y: (y - m.position().y as f64) / ms,
                sw: m.size().width as f64 / ms,
                sh: m.size().height as f64 / ms,
            };
            *PLACEMENT.lock() = Some(p);
            let _ = w.emit("brink://placement", p);
        }
    }
    PANEL_OPEN.store(true, Ordering::SeqCst);
    let _ = w.show();
    let _ = w.set_focus();
    let _ = w.emit("brink://visible", true);
}

pub fn run() {
    START.get_or_init(Instant::now);
    let shared = Arc::new(Shared {
        settings: RwLock::new(Settings::load()),
        engine: Mutex::new(Engine::new()),
        mouse_engine: Mutex::new(Engine::new()),
        windows: Mutex::new(windows::WindowGestures::new()),
        app: OnceLock::new(),
        pending_update: Mutex::new(None),
    });

    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .clear_targets()
                .level(log::LevelFilter::Info)
                .target(tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir { file_name: Some("brink".into()) }))
                .max_file_size(512 * 1024)
                .build(),
        )
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .manage(shared.clone())
        .invoke_handler(tauri::generate_handler![
            get_state,
            get_levels,
            wallpaper,
            placement,
            set_settings,
            request_access,
            open_url,
            set_panel_height,
            quit,
            check_update,
            restart
        ])
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let _ = shared.app.set(app.handle().clone());

            let appearance = shared.settings.read().appearance;
            apply_appearance(app.handle(), appearance);

            if let Some(w) = app.get_webview_window(PANEL) {
                #[cfg(target_os = "macos")]
                if let Ok(ns) = w.ns_window() {
                    mac::glass::apply(ns);
                }
                let w2 = w.clone();
                w.on_window_event(move |e| {
                    if let WindowEvent::Focused(false) = e {
                        hide_panel(&w2);
                    }
                });
            }

            #[cfg(target_os = "macos")]
            if let Some(w) = app.get_webview_window(mac::window::PREVIEW) {
                if let Ok(ns) = w.ns_window() {
                    mac::window::setup_preview_window(ns);
                }
            }
            #[cfg(target_os = "macos")]
            mac::betterdisplay::start(app.handle().clone());
            mac::media::start(app.handle().clone());

            #[cfg(target_os = "macos")]
            if let Some(w) = app.get_webview_window(mac::hud::HUD) {
                if let Ok(ns) = w.ns_window() {
                    mac::hud::setup(ns);
                }
            }

            let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;
            TrayIconBuilder::with_id("brink")
                .icon(icon)
                .icon_as_template(true)
                .tooltip("Brink")
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, rect, .. } = event {
                        toggle_panel(tray.app_handle(), Some(rect));
                    }
                })
                .build(app)?;

            #[cfg(target_os = "macos")]
            {
                if !mac::input::start(shared.clone()) {
                    log::error!("brink: could not start trackpad input");
                }
                mac::window::start_dividers(shared.clone());
                let hook = shared.clone();
                mac::output::set_hook(move |h| mac::window::on_event(&hook, h));
                // Posting keys and holding the pointer need Accessibility. Wait for it.
                let handle = app.handle().clone();
                std::thread::spawn(move || loop {
                    if mac::system::trusted() && mac::output::install_event_tap() {
                        log::info!("brink: accessibility granted, event tap installed");
                        let _ = handle.emit("brink://trusted", true);
                        break;
                    }
                    std::thread::sleep(Duration::from_secs(2));
                });
            }

            // First launch: open the panel so the setup guide is the first thing seen.
            if !shared.settings.read().onboarded {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(700));
                    let h2 = handle.clone();
                    let _ = handle.run_on_main_thread(move || {
                        let anchor = h2.tray_by_id("brink").and_then(|t| t.rect().ok().flatten());
                        toggle_panel(&h2, anchor);
                    });
                });
            }

            // Self-installing updates: at launch, then daily. A notification says when one is
            // ready; clicking it restarts into it.
            #[cfg(target_os = "macos")]
            {
                let handle = app.handle().clone();
                mac::notify::init(move || handle.restart());
            }
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(20)).await;
                    if let Err(e) = check_and_install(handle.clone(), true).await {
                        log::warn!("update check: {e}");
                    }
                    tokio::time::sleep(Duration::from_secs(60 * 60 * 24)).await;
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Brink")
        .run(|_app, event| {
            // Menu bar app: closing the panel never quits.
            if let tauri::RunEvent::ExitRequested { api, code: None, .. } = event {
                api.prevent_exit();
            }
        });
}
