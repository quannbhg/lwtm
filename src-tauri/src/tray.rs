//! System tray, mini-widget (small floating window) and "close window = hide to tray" behaviour.
//!
//! Every window owns a WebView2/WebKit renderer process, which is where most of the app's memory goes.
//! To stay light, windows are created on demand and destroyed after sitting hidden for a while:
//! - the mini-widget is created on the first tray click and destroyed [`MINI_IDLE_SECS`] after it hides;
//! - the main window is destroyed [`MAIN_IDLE_SECS`] after being sent to the tray, and re-created on demand.

use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Window, WindowEvent, Wry,
};

use crate::i18n::tr;

const MAIN_LABEL: &str = "main";
const MINI_LABEL: &str = "mini";

/// How long a hidden window is kept alive before its renderer is released.
const MAIN_IDLE_SECS: u64 = 60;
const MINI_IDLE_SECS: u64 = 30;

/// How long to wait for the mini-widget page to report it is ready before showing it anyway.
const MINI_READY_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Default)]
pub struct MiniState {
    /// When the mini-widget last hid itself after losing focus. Clicking the tray icon takes focus away
    /// from the widget before the click event arrives; without this guard it would hide and then
    /// immediately reappear.
    blurred_at: Mutex<Option<Instant>>,
    /// When the mini-widget was last shown; focus-loss events right after showing are ignored.
    shown_at: Mutex<Option<Instant>>,
    /// A freshly created mini-widget waits here (with the tray click position) until its page is ready.
    pending: Mutex<Option<Option<PhysicalPosition<f64>>>>,
    /// Bumped on every show/hide so stale "destroy later" timers can tell they were superseded.
    main_gen: AtomicU64,
    mini_gen: AtomicU64,
}

/// User preferences the Rust side needs to know (sent by the frontend at startup and on change).
pub struct Prefs {
    pub close_to_tray: AtomicBool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { close_to_tray: AtomicBool::new(true) }
    }
}

fn bump(app: &AppHandle, label: &str) -> u64 {
    let st = app.state::<MiniState>();
    let counter = if label == MAIN_LABEL { &st.main_gen } else { &st.mini_gen };
    counter.fetch_add(1, Ordering::SeqCst) + 1
}

fn current_gen(app: &AppHandle, label: &str) -> u64 {
    let st = app.state::<MiniState>();
    let counter = if label == MAIN_LABEL { &st.main_gen } else { &st.mini_gen };
    counter.load(Ordering::SeqCst)
}

/// Destroy the window after `secs` unless it was shown or hidden again in the meantime.
fn destroy_later(app: &AppHandle, label: &'static str, secs: u64) {
    let gen = bump(app, label);
    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(secs));
        if current_gen(&app, label) != gen {
            return;
        }
        if let Some(w) = app.get_webview_window(label) {
            if !w.is_visible().unwrap_or(true) {
                let _ = w.destroy();
            }
        }
    });
}

fn create_main(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let cfg = app
        .config()
        .app
        .windows
        .first()
        .cloned()
        .ok_or_else(|| tauri::Error::WindowNotFound)?;
    WebviewWindowBuilder::from_config(app, &cfg)?.build()
}

fn create_mini(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    WebviewWindowBuilder::new(app, MINI_LABEL, WebviewUrl::App("index.html".into()))
        .title("Mini")
        .inner_size(280.0, 168.0)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false)
        .build()
}

pub fn show_main(app: &AppHandle) {
    bump(app, MAIN_LABEL); // cancel any pending destroy
    if let Some(w) = app.get_webview_window(MAIN_LABEL) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        return;
    }
    // Building a window must not happen on the event-loop thread (it can deadlock on Windows)
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Ok(w) = create_main(&app) {
            let _ = w.set_focus();
        }
    });
}

pub fn hide_mini(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MINI_LABEL) {
        let _ = w.hide();
        destroy_later(app, MINI_LABEL, MINI_IDLE_SECS);
    }
}

/// Place the widget just above the click position (or below it if the taskbar is at the top), kept on screen, then show it.
fn place_and_show(app: &AppHandle, w: &WebviewWindow, anchor: Option<PhysicalPosition<f64>>) {
    bump(app, MINI_LABEL); // cancel any pending destroy
    if let (Some(p), Ok(size)) = (anchor, w.outer_size()) {
        let (sw, sh) = (size.width as f64, size.height as f64);
        let mut x = p.x - sw / 2.0;
        let mut y = p.y - sh - 16.0;
        if let Ok(Some(mon)) = app.monitor_from_point(p.x, p.y) {
            let (mx, my) = (mon.position().x as f64, mon.position().y as f64);
            let (mw, mh) = (mon.size().width as f64, mon.size().height as f64);
            if y < my {
                y = p.y + 16.0;
            }
            x = x.clamp(mx, (mx + mw - sw).max(mx));
            y = y.clamp(my, (my + mh - sh).max(my));
        }
        let _ = w.set_position(PhysicalPosition::new(x as i32, y as i32));
    }
    *app.state::<MiniState>().shown_at.lock().unwrap() = Some(Instant::now());
    let _ = w.show();
    let _ = w.set_focus();
}

/// Called by the mini-widget page once it has its first data (or by the timeout fallback).
pub fn mini_ready(app: &AppHandle) {
    let pending = app.state::<MiniState>().pending.lock().unwrap().take();
    if let (Some(anchor), Some(w)) = (pending, app.get_webview_window(MINI_LABEL)) {
        place_and_show(app, &w, anchor);
    }
}

fn recently_blurred(app: &AppHandle) -> bool {
    app.state::<MiniState>()
        .blurred_at
        .lock()
        .unwrap()
        .is_some_and(|t| t.elapsed() < Duration::from_millis(300))
}

fn toggle_mini(app: &AppHandle, anchor: Option<PhysicalPosition<f64>>) {
    if let Some(w) = app.get_webview_window(MINI_LABEL) {
        if w.is_visible().unwrap_or(false) {
            hide_mini(app);
        } else if !recently_blurred(app) {
            place_and_show(app, &w, anchor);
        }
        return;
    }
    if recently_blurred(app) {
        return;
    }
    // First use (or after being destroyed): create the window hidden and show it once the page is ready
    *app.state::<MiniState>().pending.lock().unwrap() = Some(anchor);
    let a = app.clone();
    tauri::async_runtime::spawn(async move {
        if create_mini(&a).is_err() {
            a.state::<MiniState>().pending.lock().unwrap().take();
        }
    });
    let a = app.clone();
    thread::spawn(move || {
        thread::sleep(MINI_READY_TIMEOUT);
        mini_ready(&a);
    });
}

pub fn on_window_event(window: &Window, event: &WindowEvent) {
    match (window.label(), event) {
        (MAIN_LABEL, WindowEvent::CloseRequested { api, .. }) => {
            if window.state::<Prefs>().close_to_tray.load(Ordering::Relaxed) {
                api.prevent_close();
                let _ = window.hide();
                destroy_later(window.app_handle(), MAIN_LABEL, MAIN_IDLE_SECS);
            } else {
                window.app_handle().exit(0);
            }
        }
        (MINI_LABEL, WindowEvent::Focused(false)) => {
            let st = window.state::<MiniState>();
            let just_shown = st.shown_at.lock().unwrap().is_some_and(|t| t.elapsed() < Duration::from_millis(400));
            if just_shown {
                return;
            }
            *st.blurred_at.lock().unwrap() = Some(Instant::now());
            hide_mini(window.app_handle());
        }
        _ => {}
    }
}

/// Handles to the tray menu entries so their text can follow the UI language.
pub struct TrayLabels {
    show: MenuItem<Wry>,
    mini: MenuItem<Wry>,
    quit: MenuItem<Wry>,
}

fn label(vi: &str, en: &str) -> String {
    tr(vi, en)
}

/// Re-apply the menu text after the language changed.
pub fn refresh_labels(app: &AppHandle) {
    if let Some(l) = app.try_state::<TrayLabels>() {
        let _ = l.show.set_text(label("Mở Task Manager", "Open Task Manager"));
        let _ = l.mini.set_text(label("Hiện/ẩn mini-widget", "Show/hide mini-widget"));
        let _ = l.quit.set_text(label("Thoát", "Quit"));
    }
}

pub fn setup(app: &mut App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", label("Mở Task Manager", "Open Task Manager"), true, None::<&str>)?;
    let mini = MenuItem::with_id(app, "mini", label("Hiện/ẩn mini-widget", "Show/hide mini-widget"), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", label("Thoát", "Quit"), true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &mini, &quit])?;
    app.manage(TrayLabels { show, mini, quit });

    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Lightweight Task Manager")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, ev| match ev.id.as_ref() {
            "show" => show_main(app),
            "mini" => toggle_mini(app, None),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, ev| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                position,
                ..
            } = ev
            {
                toggle_mini(tray.app_handle(), Some(position));
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;

    // Started with the system: stay in the tray without creating any window (no renderer = minimal memory)
    if !std::env::args().any(|a| a == "--minimized") {
        create_main(app.handle())?;
    }
    Ok(())
}
