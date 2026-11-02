mod admin;
mod appmeta;
mod i18n;
mod memory;
mod metrics;
mod netmon;
mod procctl;
mod security;
mod services;
mod startup;
mod tray;
mod watch;

use std::{
    collections::HashMap,
    sync::Mutex,
    time::Instant,
};

use serde::Serialize;
use sysinfo::{Networks, Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind, Users};
use tauri::State;

use appmeta::{AppMeta, MetaCache};

struct Monitor {
    sys: System,
    users: Users,
    nets: Networks,
    last: Instant,
}

struct SysState(Mutex<Monitor>);

#[derive(Serialize)]
struct ProcessInfo {
    pid: u32,
    parent: Option<u32>,
    name: String,
    exe: Option<String>,
    user: Option<String>,
    status: String,
    /// % of total CPU (0–100), the way Task Manager displays it
    cpu: f32,
    /// bytes (working set)
    memory: u64,
    /// bytes per second
    disk_read: f64,
    disk_write: f64,
}

#[derive(Serialize)]
struct Snapshot {
    cpu: f32,
    cores: Vec<f32>,
    cpu_name: String,
    cpu_freq_mhz: u64,
    memory_used: u64,
    memory_total: u64,
    swap_used: u64,
    swap_total: u64,
    /// bytes per second, total disk I/O of all processes
    disk_read: f64,
    disk_write: f64,
    /// bytes per second, total across all network adapters
    net_rx: f64,
    net_tx: f64,
    uptime: u64,
    host: String,
    os: String,
    processes: Vec<ProcessInfo>,
}

#[tauri::command(async)]
fn get_snapshot(app: tauri::AppHandle, state: State<SysState>) -> Snapshot {
    let mut guard = state.0.lock().unwrap();
    let m = &mut *guard; // deref once so fields can be borrowed independently
    let secs = m.last.elapsed().as_secs_f64().max(0.001);
    m.last = Instant::now();

    let sys = &mut m.sys;
    sys.refresh_cpu_usage();
    sys.refresh_cpu_frequency();
    sys.refresh_memory();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .with_disk_usage()
            .with_exe(UpdateKind::OnlyIfNotSet),
    );

    let cpu_count = sys.cpus().len().max(1);
    let users = &m.users;
    // Task Manager-style memory (private working set) on Windows; sysinfo's number elsewhere
    let pids: Vec<u32> = sys.processes().keys().map(|p| p.as_u32()).collect();
    let private = memory::private_working_set(&pids);
    let processes: Vec<ProcessInfo> = sys
        .processes()
        .iter()
        .map(|(pid, p)| {
            let d = p.disk_usage();
            ProcessInfo {
                pid: pid.as_u32(),
                parent: p.parent().map(|x| x.as_u32()),
                name: p.name().to_string_lossy().into_owned(),
                exe: p.exe().map(|e| e.to_string_lossy().into_owned()),
                user: p.user_id().and_then(|u| users.get_user_by_id(u)).map(|u| u.name().to_string()),
                status: p.status().to_string(),
                cpu: p.cpu_usage() / cpu_count as f32,
                memory: private.get(&pid.as_u32()).copied().unwrap_or_else(|| p.memory()),
                disk_read: d.read_bytes as f64 / secs,
                disk_write: d.written_bytes as f64 / secs,
            }
        })
        .collect();

    let cores = sys.cpus().iter().map(|c| c.usage()).collect();
    let cpu_name = sys.cpus().first().map(|c| c.brand().trim().to_string()).unwrap_or_default();
    let cpu_freq_mhz = sys.cpus().first().map(|c| c.frequency()).unwrap_or(0);
    let snap_cpu = sys.global_cpu_usage();
    let (memory_used, memory_total) = (sys.used_memory(), sys.total_memory());
    let (swap_used, swap_total) = (sys.used_swap(), sys.total_swap());
    let disk_read = processes.iter().map(|p| p.disk_read).sum();
    let disk_write = processes.iter().map(|p| p.disk_write).sum();

    m.nets.refresh(true);
    let (rx, tx) = m
        .nets
        .values()
        .fold((0u64, 0u64), |(r, t), n| (r + n.received(), t + n.transmitted()));

    if let Some(tray) = app.tray_by_id("main") {
        let ram = memory_used as f64 / memory_total.max(1) as f64 * 100.0;
        let _ = tray.set_tooltip(Some(format!("CPU {snap_cpu:.0}% · RAM {ram:.0}%")));
    }

    Snapshot {
        cpu: snap_cpu,
        cores,
        cpu_name,
        cpu_freq_mhz,
        memory_used,
        memory_total,
        swap_used,
        swap_total,
        disk_read,
        disk_write,
        net_rx: rx as f64 / secs,
        net_tx: tx as f64 / secs,
        uptime: System::uptime().unwrap_or(0),
        host: System::host_name().unwrap_or_default(),
        os: System::long_os_version().unwrap_or_default(),
        processes,
    }
}

/// Names + icons for a batch of exe paths (the frontend caches them and only asks for missing ones).
#[tauri::command(async)]
fn get_app_meta(exes: Vec<String>, cache: State<MetaCache>) -> HashMap<String, AppMeta> {
    exes.into_iter()
        .map(|e| {
            let m = cache.get(&e);
            (e, m)
        })
        .collect()
}

#[tauri::command(async)]
fn kill_processes(pids: Vec<u32>, state: State<SysState>) -> Result<(), String> {
    let m = state.0.lock().unwrap();
    let mut failed = 0;
    for pid in &pids {
        match m.sys.process(Pid::from_u32(*pid)) {
            Some(p) if p.kill() => {}
            _ => failed += 1,
        }
    }
    if failed == 0 {
        Ok(())
    } else {
        let n = pids.len();
        Err(i18n::tr(
            format!("Không thể kết thúc {failed}/{n} tiến trình (thiếu quyền hoặc đã thoát)"),
            format!("Could not end {failed}/{n} processes (missing permission or already exited)"),
        ))
    }
}

#[tauri::command(async)]
fn get_process_extras(pids: Vec<u32>) -> HashMap<u32, procctl::Extra> {
    procctl::extras(&pids)
}

#[tauri::command(async)]
fn set_priority(pids: Vec<u32>, priority: procctl::Priority) -> Result<(), String> {
    procctl::set_priority(&pids, priority)
}

#[tauri::command(async)]
fn set_efficiency(pids: Vec<u32>, enabled: bool) -> Result<(), String> {
    procctl::set_efficiency(&pids, enabled)
}

#[tauri::command(async)]
fn list_services() -> Result<Vec<services::ServiceInfo>, String> {
    services::list()
}

#[tauri::command(async)]
fn scan_security(kind: String) -> Result<Vec<security::Finding>, String> {
    security::scan(&kind)
}

/// Open the VirusTotal hash lookup page in the default browser (accepts valid SHA-256 hashes only).
#[tauri::command]
fn open_virustotal(hash: String) -> Result<(), String> {
    if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(i18n::tr("Mã hash không hợp lệ", "Invalid hash"));
    }
    let url = format!("https://www.virustotal.com/gui/file/{hash}");
    #[cfg(windows)]
    let mut cmd = std::process::Command::new("rundll32");
    #[cfg(windows)]
    cmd.args(["url.dll,FileProtocolHandler", &url]);
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(target_os = "macos")]
    cmd.arg(&url);
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = std::process::Command::new("xdg-open");
    #[cfg(all(unix, not(target_os = "macos")))]
    cmd.arg(&url);
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command]
fn is_elevated() -> bool {
    admin::is_elevated()
}

#[tauri::command]
fn restart_as_admin(app: tauri::AppHandle) -> Result<(), String> {
    admin::relaunch_elevated()?;
    app.exit(0);
    Ok(())
}

#[tauri::command(async)]
fn list_startup() -> Result<Vec<startup::StartupEntry>, String> {
    startup::list()
}

#[tauri::command(async)]
fn set_startup_enabled(target: startup::Target, enabled: bool) -> Result<(), String> {
    startup::set_enabled(&target, enabled)
}

#[tauri::command]
fn get_autostart() -> bool {
    startup::autostart_enabled()
}

#[tauri::command]
fn set_autostart(enabled: bool) -> Result<(), String> {
    startup::set_autostart(enabled)
}

#[tauri::command]
fn set_close_to_tray(enabled: bool, prefs: State<tray::Prefs>) {
    prefs.close_to_tray.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

/// GPU, temperatures and disk active time (each field is None when the system does not expose it).
#[tauri::command(async)]
fn get_hardware() -> metrics::Hardware {
    metrics::hardware()
}

/// Per-process live I/O that the snapshot does not carry: GPU use and network rates.
#[derive(Serialize)]
struct ProcIo {
    gpu: metrics::ProcGpu,
    net: netmon::NetRates,
}

#[tauri::command(async)]
fn get_process_io() -> ProcIo {
    ProcIo { gpu: metrics::process_gpu(), net: netmon::per_process() }
}

/// Memory used by this app itself (Rust process + WebView processes), measured the Task Manager way.
#[tauri::command(async)]
fn get_self_usage() -> Option<memory::SelfUsage> {
    memory::self_usage()
}

/// False on desktops without a tray (stock GNOME): the "hide to tray" option cannot work there.
#[tauri::command]
fn tray_available(app: tauri::AppHandle) -> bool {
    tray::tray_available(&app)
}

#[tauri::command]
fn show_main_window(app: tauri::AppHandle) {
    tray::show_main(&app);
    tray::hide_mini(&app);
}

/// The mini-widget page calls this once it has data, so the window is shown already painted.
#[tauri::command]
fn mini_ready(app: tauri::AppHandle) {
    tray::mini_ready(&app);
}

#[tauri::command(async)]
fn control_service(name: String, action: services::Action) -> Result<(), String> {
    services::control(&name, action)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let monitor = Monitor {
        sys: System::new().expect("failed to initialise sysinfo"),
        users: Users::new_with_refreshed_list().expect("failed to read the user list"),
        nets: Networks::new_with_refreshed_list().expect("failed to read network information"),
        last: Instant::now(),
    };
    tauri::Builder::default()
        .manage(SysState(Mutex::new(monitor)))
        .manage(MetaCache::default())
        .manage(tray::MiniState::default())
        .manage(tray::Prefs::default())
        .manage(watch::Watch::default())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            // watch::start first: it loads the saved language that the tray menu text depends on
            watch::start(app.handle().clone());
            tray::setup(app)?;
            Ok(())
        })
        .on_window_event(|window, event| tray::on_window_event(window, event))
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            get_app_meta,
            kill_processes,
            get_process_extras,
            set_priority,
            set_efficiency,
            list_services,
            control_service,
            scan_security,
            open_virustotal,
            is_elevated,
            restart_as_admin,
            list_startup,
            set_startup_enabled,
            get_autostart,
            set_autostart,
            set_close_to_tray,
            show_main_window,
            tray_available,
            get_self_usage,
            get_hardware,
            get_process_io,
            mini_ready,
            watch::get_history,
            watch::clear_history,
            watch::get_watch_config,
            watch::set_watch_config,
            watch::set_lang
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            // Windows are destroyed when idle, so the "no windows left" that follows must not quit the
            // app: it keeps living in the tray. Real quits (tray "Quit", Cmd+Q, Dock → Quit) still work.
            tauri::RunEvent::ExitRequested { api, code, .. } => {
                if code.is_none() && tray::should_prevent_exit(app) {
                    api.prevent_exit();
                }
            }
            // macOS: clicking the Dock icon while no window is open should bring the window back
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => tray::show_main(app),
            tauri::RunEvent::Exit => {
                watch::flush(app);
                netmon::shutdown();
            }
            _ => {}
        });
}
