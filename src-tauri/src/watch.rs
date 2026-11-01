//! Background watcher: records a 24-hour history of system load and raises alerts when the machine
//! stays overloaded.
//!
//! It runs in Rust (not in the web page) because the UI window may not exist at all while the app sits
//! in the tray. It uses its own `System` so its sampling never disturbs the per-process CPU readings
//! of the live views.

use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::PathBuf,
    sync::Mutex,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use sysinfo::{Networks, ProcessRefreshKind, ProcessesToUpdate, System};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_notification::NotificationExt;

/// Seconds between samples. Each sample refreshes the process list, so keep this coarse.
const INTERVAL_SECS: u64 = 10;
/// History kept in memory and on disk.
const RETENTION_SECS: u64 = 24 * 3600;
/// Persist the history every this many samples (30 × 10 s = 5 min).
const SAVE_EVERY: u32 = 30;
/// Minimum time between two alerts of the same kind.
const ALERT_COOLDOWN: Duration = Duration::from_secs(10 * 60);

#[derive(Clone, Serialize, Deserialize)]
pub struct Top {
    /// process (application) name
    pub n: String,
    /// % of total CPU
    pub c: f32,
    /// resident memory, bytes
    pub m: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Sample {
    /// unix time, seconds
    pub t: u64,
    pub cpu: f32,
    /// % of RAM in use
    pub ram: f32,
    /// bytes/s
    pub disk: f32,
    pub net: f32,
    /// the busiest applications at that moment
    pub top: Vec<Top>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub history: bool,
    pub notify: bool,
    pub cpu_threshold: f32,
    pub ram_threshold: f32,
    pub sustain_secs: u64,
    /// "vi" or "en": language of alert notifications
    pub lang: String,
}

impl Default for Config {
    fn default() -> Self {
        Self { history: true, notify: true, cpu_threshold: 90.0, ram_threshold: 90.0, sustain_secs: 120, lang: "vi".into() }
    }
}

#[derive(Default)]
pub struct Watch {
    samples: Mutex<VecDeque<Sample>>,
    cfg: Mutex<Config>,
    dir: Mutex<Option<PathBuf>>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Watch {
    fn file(&self, name: &str) -> Option<PathBuf> {
        self.dir.lock().unwrap().as_ref().map(|d| d.join(name))
    }

    fn save_history(&self) {
        let Some(path) = self.file("history.json") else { return };
        let json = {
            let s = self.samples.lock().unwrap();
            serde_json::to_vec(&*s)
        };
        if let Ok(json) = json {
            // write to a temp file first so a crash mid-write cannot corrupt the history
            let tmp = path.with_extension("json.tmp");
            if fs::write(&tmp, json).is_ok() {
                let _ = fs::rename(tmp, path);
            }
        }
    }

    fn save_config(&self) {
        if let (Some(path), Ok(json)) = (self.file("watch.json"), serde_json::to_vec(&*self.cfg.lock().unwrap())) {
            let _ = fs::write(path, json);
        }
    }
}

/// Persist the history (called when the app exits).
pub fn flush(app: &AppHandle) {
    app.state::<Watch>().save_history();
}

/// Load saved state and start the sampling thread.
pub fn start(app: AppHandle) {
    let w = app.state::<Watch>();
    if let Ok(dir) = app.path().app_data_dir() {
        let _ = fs::create_dir_all(&dir);
        *w.dir.lock().unwrap() = Some(dir);
    }
    if let Some(cfg) = w.file("watch.json").and_then(|p| fs::read(p).ok()).and_then(|b| serde_json::from_slice::<Config>(&b).ok()) {
        crate::i18n::set(&cfg.lang);
        *w.cfg.lock().unwrap() = cfg;
    }
    if let Some(old) = w.file("history.json").and_then(|p| fs::read(p).ok()).and_then(|b| serde_json::from_slice::<VecDeque<Sample>>(&b).ok()) {
        let min = now().saturating_sub(RETENTION_SECS);
        *w.samples.lock().unwrap() = old.into_iter().filter(|s| s.t >= min).collect();
    }
    drop(w);
    thread::spawn(move || run(app));
}

fn run(app: AppHandle) {
    let Ok(mut sys) = System::new() else { return };
    let Ok(mut nets) = Networks::new_with_refreshed_list() else { return };
    let kind = ProcessRefreshKind::nothing().with_cpu().with_memory().with_disk_usage();
    let (mut over_cpu, mut over_ram) = (0u64, 0u64);
    let (mut last_cpu_alert, mut last_ram_alert): (Option<Instant>, Option<Instant>) = (None, None);
    let mut since_save = 0u32;
    let mut last = Instant::now();
    let mut warmed_up = false;

    loop {
        thread::sleep(Duration::from_secs(if warmed_up { INTERVAL_SECS } else { 2 }));
        let cfg = app.state::<Watch>().cfg.lock().unwrap().clone();
        if !cfg.history && !cfg.notify {
            warmed_up = false;
            (over_cpu, over_ram) = (0, 0);
            continue;
        }

        let secs = last.elapsed().as_secs_f64().max(0.001);
        last = Instant::now();
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
        nets.refresh(true);
        // The first reading after (re)starting has no CPU baseline; skip it
        if !warmed_up {
            warmed_up = true;
            continue;
        }

        let cores = sys.cpus().len().max(1) as f32;
        let mut by_name: HashMap<String, (f32, u64)> = HashMap::new();
        let mut disk = 0f64;
        for p in sys.processes().values() {
            let name = p.name().to_string_lossy().into_owned();
            if name.eq_ignore_ascii_case("system idle process") {
                continue;
            }
            let d = p.disk_usage();
            disk += (d.read_bytes + d.written_bytes) as f64;
            let e = by_name.entry(name).or_default();
            e.0 += p.cpu_usage() / cores;
            e.1 += p.memory();
        }
        let mut apps: Vec<Top> = by_name.into_iter().map(|(n, (c, m))| Top { n, c, m }).collect();
        apps.sort_by(|a, b| b.c.total_cmp(&a.c));
        let mut top: Vec<Top> = apps.iter().take(3).cloned().collect();
        apps.sort_by(|a, b| b.m.cmp(&a.m));
        for a in apps.into_iter().take(2) {
            if !top.iter().any(|t| t.n == a.n) {
                top.push(a);
            }
        }

        let total = sys.total_memory().max(1) as f32;
        let sample = Sample {
            t: now(),
            cpu: sys.global_cpu_usage(),
            ram: sys.used_memory() as f32 / total * 100.0,
            disk: (disk / secs) as f32,
            net: (nets.values().map(|n| n.received() + n.transmitted()).sum::<u64>() as f64 / secs) as f32,
            top,
        };

        // --- alerts: sustained overload, with a cooldown per kind ---
        let need = (cfg.sustain_secs / INTERVAL_SECS).max(1);
        over_cpu = if sample.cpu >= cfg.cpu_threshold { over_cpu + 1 } else { 0 };
        over_ram = if sample.ram >= cfg.ram_threshold { over_ram + 1 } else { 0 };
        if cfg.notify {
            if over_cpu >= need && last_cpu_alert.map_or(true, |t| t.elapsed() > ALERT_COOLDOWN) {
                last_cpu_alert = Some(Instant::now());
                let culprit = sample.top.iter().max_by(|a, b| a.c.total_cmp(&b.c));
                alert(&app, &cfg.lang, true, sample.cpu, culprit.map(|t| (t.n.as_str(), format!("{:.0}%", t.c))));
            }
            if over_ram >= need && last_ram_alert.map_or(true, |t| t.elapsed() > ALERT_COOLDOWN) {
                last_ram_alert = Some(Instant::now());
                let culprit = sample.top.iter().max_by_key(|t| t.m);
                alert(&app, &cfg.lang, false, sample.ram, culprit.map(|t| (t.n.as_str(), format!("{} MB", t.m >> 20))));
            }
        }

        if cfg.history {
            let w = app.state::<Watch>();
            {
                let mut s = w.samples.lock().unwrap();
                s.push_back(sample);
                let min = now().saturating_sub(RETENTION_SECS);
                while s.front().is_some_and(|x| x.t < min) {
                    s.pop_front();
                }
            }
            since_save += 1;
            if since_save >= SAVE_EVERY {
                since_save = 0;
                w.save_history();
            }
        }
    }
}

fn alert(app: &AppHandle, lang: &str, is_cpu: bool, value: f32, culprit: Option<(&str, String)>) {
    let en = lang == "en";
    let (title, mut body) = match (is_cpu, en) {
        // (Vietnamese first, then English)
        (true, false) => ("CPU đang quá tải".to_string(), format!("CPU ở mức {value:.0}% trong một thời gian dài.")),
        (true, true) => ("CPU is overloaded".to_string(), format!("CPU has been at {value:.0}% for a while.")),
        (false, false) => ("RAM gần đầy".to_string(), format!("RAM đang dùng {value:.0}% và máy có thể bị giật.")),
        (false, true) => ("Memory is almost full".to_string(), format!("RAM is {value:.0}% used and the system may stutter.")),
    };
    if let Some((name, amount)) = culprit {
        body.push_str(&if en { format!(" Biggest: {name} ({amount}).") } else { format!(" Nặng nhất: {name} ({amount}).") });
    }
    let _ = app.notification().builder().title(title).body(body).show();
}

#[tauri::command]
pub fn get_history(w: State<Watch>) -> Vec<Sample> {
    w.samples.lock().unwrap().iter().cloned().collect()
}

#[tauri::command]
pub fn clear_history(w: State<Watch>) {
    w.samples.lock().unwrap().clear();
    w.save_history();
}

#[tauri::command]
pub fn get_watch_config(w: State<Watch>) -> Config {
    w.cfg.lock().unwrap().clone()
}

#[tauri::command]
pub fn set_watch_config(cfg: Config, w: State<Watch>) {
    crate::i18n::set(&cfg.lang);
    *w.cfg.lock().unwrap() = cfg;
    w.save_config();
}

/// Change the language used by Rust-side messages (alerts, errors, tray menu), keeping the other settings.
#[tauri::command]
pub fn set_lang(lang: String, app: AppHandle, w: State<Watch>) {
    crate::i18n::set(&lang);
    let changed = {
        let mut c = w.cfg.lock().unwrap();
        let changed = c.lang != lang;
        c.lang = lang;
        changed
    };
    if changed {
        w.save_config();
    }
    crate::tray::refresh_labels(&app);
}
