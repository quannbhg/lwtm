//! System tray, mini-widget (small floating window) and "close window = hide to tray" behaviour.

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder, Window, WindowEvent,
};

const MINI_LABEL: &str = "mini";

/// When the mini-widget last hid itself after losing focus. Clicking the tray icon takes focus away
/// from the widget before the click event arrives; without this guard it would hide and then
/// immediately reappear.
#[derive(Default)]
pub struct MiniState(Mutex<Option<Instant>>);

/// User preferences the Rust side needs to know (sent by the frontend at startup and on change).
pub struct Prefs {
    pub close_to_tray: AtomicBool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { close_to_tray: AtomicBool::new(true) }
    }
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

pub fn hide_mini(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MINI_LABEL) {
        let _ = w.hide();
    }
}

fn toggle_mini(app: &AppHandle, anchor: Option<PhysicalPosition<f64>>) {
    let Some(w) = app.get_webview_window(MINI_LABEL) else { return };
    if w.is_visible().unwrap_or(false) {
        let _ = w.hide();
        return;
    }
    if let Some(t) = *app.state::<MiniState>().0.lock().unwrap() {
        if t.elapsed() < Duration::from_millis(300) {
            return;
        }
    }
    // Place the widget just above the click position (or below it if the taskbar is at the top), kept on screen
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
    let _ = w.show();
    let _ = w.set_focus();
}

pub fn on_window_event(window: &Window, event: &WindowEvent) {
    match (window.label(), event) {
        ("main", WindowEvent::CloseRequested { api, .. }) => {
            // The hidden mini window keeps the app alive, so exit must be called explicitly
            if window.state::<Prefs>().close_to_tray.load(Ordering::Relaxed) {
                api.prevent_close();
                let _ = window.hide();
            } else {
                window.app_handle().exit(0);
            }
        }
        (MINI_LABEL, WindowEvent::Focused(false)) => {
            let _ = window.hide();
            *window.state::<MiniState>().0.lock().unwrap() = Some(Instant::now());
        }
        _ => {}
    }
}

pub fn setup(app: &mut App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Mở Task Manager", true, None::<&str>)?;
    let mini = MenuItem::with_id(app, "mini", "Hiện/ẩn mini-widget", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Thoát", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &mini, &quit])?;

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

    WebviewWindowBuilder::new(app, MINI_LABEL, WebviewUrl::App("index.html".into()))
        .title("Mini")
        .inner_size(280.0, 168.0)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false)
        .build()?;

    // Started with the system: run hidden in the tray
    if std::env::args().any(|a| a == "--minimized") {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.hide();
        }
    }
    Ok(())
}
