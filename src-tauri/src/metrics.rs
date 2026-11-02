//! Hardware metrics Task Manager shows but sysinfo does not provide: GPU, temperatures and the
//! "active time" of the disks, plus per-process GPU use.
//!
//! Everything here is best-effort: each source can be missing (no GPU driver, no sensors, no
//! permission), and then the matching field is simply `None` and the UI hides it.

use std::{collections::HashMap, process::Command, sync::Mutex, time::Instant};

use serde::Serialize;
use sysinfo::Components;

#[derive(Serialize, Clone, Default)]
pub struct GpuInfo {
    pub name: String,
    /// % busy (0–100)
    pub usage: Option<f32>,
    /// dedicated video memory, bytes
    pub mem_used: Option<u64>,
    pub mem_total: Option<u64>,
    pub temp: Option<f32>,
}

#[derive(Serialize)]
pub struct Temp {
    pub label: String,
    pub celsius: f32,
}

#[derive(Serialize, Default)]
pub struct Hardware {
    pub gpus: Vec<GpuInfo>,
    /// best guess for the CPU temperature, °C
    pub cpu_temp: Option<f32>,
    /// every sensor the system exposes
    pub temps: Vec<Temp>,
    /// % of time the busiest disk was servicing requests
    pub disk_active: Option<f32>,
}

#[derive(Serialize, Default)]
pub struct ProcGpu {
    /// pid -> % of the GPU used by that process
    pub usage: HashMap<u32, f32>,
    pub supported: bool,
}

struct State {
    components: Option<Components>,
    #[cfg(target_os = "linux")]
    disk_prev: Option<(Instant, HashMap<String, u64>)>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

/// Run a helper tool and return its stdout, hiding the console window on Windows.
fn run(program: &str, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// NVIDIA GPUs through `nvidia-smi` (ships with the driver on Windows and Linux). Cached for a few
/// seconds because it is a separate process.
#[cfg(any(windows, target_os = "linux"))]
fn nvidia() -> Vec<GpuInfo> {
    static CACHE: Mutex<Option<(Instant, Vec<GpuInfo>)>> = Mutex::new(None);
    let mut c = CACHE.lock().unwrap();
    if let Some((t, v)) = c.as_ref() {
        if t.elapsed().as_secs() < 3 {
            return v.clone();
        }
    }
    let list: Vec<GpuInfo> = run(
        "nvidia-smi",
        &["--query-gpu=name,utilization.gpu,memory.used,memory.total,temperature.gpu", "--format=csv,noheader,nounits"],
    )
    .map(|s| {
        s.lines()
            .filter_map(|l| {
                let f: Vec<&str> = l.split(',').map(str::trim).collect();
                (f.len() >= 5).then(|| GpuInfo {
                    name: f[0].to_string(),
                    usage: f[1].parse().ok(),
                    mem_used: f[2].parse::<u64>().ok().map(|m| m << 20),
                    mem_total: f[3].parse::<u64>().ok().map(|m| m << 20),
                    temp: f[4].parse().ok(),
                })
            })
            .collect()
    })
    .unwrap_or_default();
    *c = Some((Instant::now(), list.clone()));
    list
}

/// Pick the sensor that most likely is the CPU package/core temperature.
fn pick_cpu_temp(temps: &[Temp]) -> Option<f32> {
    const HINTS: &[&str] = &["package", "tctl", "tdie", "cpu", "core", "k10temp", "coretemp", "soc", "die"];
    let hinted = temps
        .iter()
        .filter(|t| {
            let l = t.label.to_lowercase();
            HINTS.iter().any(|h| l.contains(h))
        })
        .map(|t| t.celsius)
        .fold(None, |m: Option<f32>, v| Some(m.map_or(v, |x| x.max(v))));
    hinted.or_else(|| temps.iter().map(|t| t.celsius).fold(None, |m: Option<f32>, v| Some(m.map_or(v, |x| x.max(v)))))
}

pub fn hardware() -> Hardware {
    let mut guard = STATE.lock().unwrap();
    let st = guard.get_or_insert_with(|| State {
        components: Components::new_with_refreshed_list().ok(),
        #[cfg(target_os = "linux")]
        disk_prev: None,
    });

    let mut temps = Vec::new();
    if let Some(c) = st.components.as_mut() {
        c.refresh(true);
        for comp in c.iter() {
            if let Some(t) = comp.temperature().filter(|t| t.is_finite() && *t > 0.0 && *t < 150.0) {
                temps.push(Temp { label: comp.label().to_string(), celsius: t });
            }
        }
    }

    #[cfg(target_os = "linux")]
    let disk_active = linux_disk_active(&mut st.disk_prev);
    #[cfg(windows)]
    let disk_active = win::disk_active();
    #[cfg(not(any(windows, target_os = "linux")))]
    let disk_active = None;

    Hardware { gpus: gpus(), cpu_temp: pick_cpu_temp(&temps), temps, disk_active }
}

/// GPU use per process (Windows: performance counters, Linux/NVIDIA: `nvidia-smi pmon`).
pub fn process_gpu() -> ProcGpu {
    #[cfg(windows)]
    {
        return win::process_gpu();
    }
    #[cfg(target_os = "linux")]
    {
        // columns: gpu pid type sm mem enc dec command   ("-" when unknown)
        if let Some(out) = run("nvidia-smi", &["pmon", "-c", "1", "-s", "u"]) {
            let mut usage = HashMap::new();
            for l in out.lines().filter(|l| !l.trim_start().starts_with('#')) {
                let f: Vec<&str> = l.split_whitespace().collect();
                if f.len() >= 4 {
                    if let (Ok(pid), Ok(sm)) = (f[1].parse::<u32>(), f[3].parse::<f32>()) {
                        *usage.entry(pid).or_insert(0.0) += sm;
                    }
                }
            }
            return ProcGpu { usage, supported: true };
        }
    }
    #[allow(unreachable_code)]
    ProcGpu::default()
}

fn gpus() -> Vec<GpuInfo> {
    #[cfg(windows)]
    {
        return win::gpus(&nvidia());
    }
    #[cfg(target_os = "linux")]
    {
        let mut v = nvidia();
        v.extend(linux_drm_gpus());
        return v;
    }
    #[cfg(target_os = "macos")]
    {
        return macos_gpus();
    }
    #[allow(unreachable_code)]
    Vec::new()
}

// ------------------------------------------------------------------ Linux

/// AMD (and name-only Intel) GPUs from sysfs. NVIDIA cards are skipped here: `nvidia-smi` covers them.
#[cfg(target_os = "linux")]
fn linux_drm_gpus() -> Vec<GpuInfo> {
    use std::fs;

    let read = |p: &std::path::Path| fs::read_to_string(p).ok().map(|s| s.trim().to_string());
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir("/sys/class/drm") else { return out };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        // "card0" but not "card0-HDMI-A-1"
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let dev = e.path().join("device");
        let vendor = read(&dev.join("vendor")).unwrap_or_default();
        let label = match vendor.as_str() {
            "0x1002" => "AMD GPU",
            "0x8086" => "Intel GPU",
            _ => continue, // NVIDIA (0x10de) handled elsewhere, others unknown
        };
        let temp = fs::read_dir(dev.join("hwmon"))
            .ok()
            .and_then(|mut h| h.next())
            .and_then(|h| read(&h.ok()?.path().join("temp1_input")))
            .and_then(|t| t.parse::<f32>().ok())
            .map(|t| t / 1000.0);
        out.push(GpuInfo {
            name: label.to_string(),
            usage: read(&dev.join("gpu_busy_percent")).and_then(|v| v.parse().ok()),
            mem_used: read(&dev.join("mem_info_vram_used")).and_then(|v| v.parse().ok()),
            mem_total: read(&dev.join("mem_info_vram_total")).and_then(|v| v.parse().ok()),
            temp,
        });
    }
    out
}

/// Utilization of the busiest whole disk, from the `io_ticks` counter of /proc/diskstats
/// (milliseconds spent doing I/O): the same "active time" Task Manager shows.
#[cfg(target_os = "linux")]
fn linux_disk_active(prev: &mut Option<(Instant, HashMap<String, u64>)>) -> Option<f32> {
    let text = std::fs::read_to_string("/proc/diskstats").ok()?;
    let is_whole_disk = |n: &str| {
        let digits_end = |p: &str| n.strip_prefix(p).is_some_and(|r| !r.is_empty() && r.chars().all(|c| c.is_ascii_alphabetic()));
        digits_end("sd")
            || digits_end("vd")
            || digits_end("xvd")
            || n.strip_prefix("nvme").is_some_and(|r| r.contains('n') && !r.contains('p'))
            || n.strip_prefix("mmcblk").is_some_and(|r| r.chars().all(|c| c.is_ascii_digit()))
    };
    let mut now = HashMap::new();
    for l in text.lines() {
        let f: Vec<&str> = l.split_whitespace().collect();
        // major minor name  reads … ; io_ticks is the 13th field after the name's 3 leading columns
        if f.len() >= 13 && is_whole_disk(f[2]) {
            if let Ok(ticks) = f[12].parse::<u64>() {
                now.insert(f[2].to_string(), ticks);
            }
        }
    }
    let t = Instant::now();
    let result = prev.as_ref().and_then(|(t0, old)| {
        let ms = t.duration_since(*t0).as_secs_f32() * 1000.0;
        if ms < 100.0 {
            return None;
        }
        now.iter()
            .filter_map(|(n, v)| old.get(n).map(|o| (v.saturating_sub(*o) as f32 / ms * 100.0).min(100.0)))
            .fold(None, |m: Option<f32>, v| Some(m.map_or(v, |x| x.max(v))))
    });
    *prev = Some((t, now));
    result
}

// ------------------------------------------------------------------ macOS

/// GPU utilization from the IOKit registry (`ioreg`), which needs no special permission.
#[cfg(target_os = "macos")]
fn macos_gpus() -> Vec<GpuInfo> {
    let Some(out) = run("ioreg", &["-r", "-d", "1", "-w", "0", "-c", "IOAccelerator"]) else { return Vec::new() };
    let number_after = |s: &str, key: &str| -> Option<f32> {
        let i = s.find(key)? + key.len();
        let rest = &s[i..];
        let digits: String = rest.trim_start_matches(|c: char| c == '=' || c == ' ').chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse().ok()
    };
    let name = out
        .find("\"model\" = \"")
        .and_then(|i| out[i + 11..].split('"').next())
        .unwrap_or("GPU")
        .to_string();
    vec![GpuInfo {
        name,
        usage: number_after(&out, "\"Device Utilization %\""),
        mem_used: number_after(&out, "\"In use system memory\"").map(|b| b as u64),
        mem_total: None,
        temp: None,
    }]
}

// ------------------------------------------------------------------ Windows (PDH + DXGI)

#[cfg(windows)]
mod win {
    use std::{collections::HashMap, sync::Mutex};

    use windows::core::{w, PCWSTR};
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
    use windows::Win32::System::Performance::*;

    use super::{GpuInfo, ProcGpu};

    struct Adapter {
        /// "0x00000000_0x0000a5b1", the form used in performance-counter instance names
        luid: String,
        name: String,
        mem_total: u64,
    }

    struct Pdh {
        query: PDH_HQUERY,
        idle: Option<PDH_HCOUNTER>,
        engine: Option<PDH_HCOUNTER>,
        mem_used: Option<PDH_HCOUNTER>,
        adapters: Vec<Adapter>,
    }

    // The PDH handles wrap raw pointers, but they are opaque ids that PDH itself does not tie to a
    // thread, and every access goes through this mutex.
    unsafe impl Send for Pdh {}

    static PDH: Mutex<Option<Pdh>> = Mutex::new(None);

    fn add(query: PDH_HQUERY, path: PCWSTR) -> Option<PDH_HCOUNTER> {
        let mut c = PDH_HCOUNTER::default();
        (unsafe { PdhAddEnglishCounterW(query, path, 0, &mut c) } == 0).then_some(c)
    }

    fn adapters() -> Vec<Adapter> {
        let mut out = Vec::new();
        unsafe {
            let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else { return out };
            let mut i = 0;
            while let Ok(a) = factory.EnumAdapters1(i) {
                i += 1;
                let Ok(d) = a.GetDesc1() else { continue };
                if d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                    continue;
                }
                let n = d.Description.iter().position(|&c| c == 0).unwrap_or(d.Description.len());
                out.push(Adapter {
                    luid: format!("0x{:08x}_0x{:08x}", d.AdapterLuid.HighPart as u32, d.AdapterLuid.LowPart),
                    name: String::from_utf16_lossy(&d.Description[..n]),
                    mem_total: d.DedicatedVideoMemory as u64,
                });
            }
        }
        out
    }

    fn init() -> Option<Pdh> {
        let mut query = PDH_HQUERY::default();
        if unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut query) } != 0 {
            return None;
        }
        let p = Pdh {
            query,
            idle: add(query, w!("\\PhysicalDisk(_Total)\\% Idle Time")),
            engine: add(query, w!("\\GPU Engine(*)\\Utilization Percentage")),
            mem_used: add(query, w!("\\GPU Adapter Memory(*)\\Dedicated Usage")),
            adapters: adapters(),
        };
        // rate counters need a first sample as their baseline
        unsafe { PdhCollectQueryData(query) };
        Some(p)
    }

    /// All (instance name, value) pairs of a wildcard counter.
    fn array(counter: PDH_HCOUNTER) -> Vec<(String, f64)> {
        const MORE_DATA: u32 = 0x8000_07D2;
        let (mut size, mut count) = (0u32, 0u32);
        let st = unsafe { PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, None) };
        if st != MORE_DATA || size == 0 {
            return Vec::new();
        }
        // u64 backing keeps the buffer aligned for PDH_FMT_COUNTERVALUE_ITEM_W
        let mut buf = vec![0u64; size as usize / 8 + 1];
        let st = unsafe {
            PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, Some(buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W))
        };
        if st != 0 {
            return Vec::new();
        }
        let items = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const PDH_FMT_COUNTERVALUE_ITEM_W, count as usize) };
        items
            .iter()
            .filter(|i| i.FmtValue.CStatus == 0)
            .filter_map(|i| Some((unsafe { i.szName.to_string() }.ok()?, unsafe { i.FmtValue.Anonymous.doubleValue })))
            .collect()
    }

    fn with_pdh<T>(f: impl FnOnce(&Pdh) -> T) -> Option<T> {
        let mut g = PDH.lock().unwrap();
        if g.is_none() {
            *g = init();
        }
        let p = g.as_ref()?;
        unsafe { PdhCollectQueryData(p.query) };
        Some(f(p))
    }

    /// % of time the disks were busy: 100 − "% Idle Time".
    pub fn disk_active() -> Option<f32> {
        with_pdh(|p| {
            let c = p.idle?;
            let mut v = PDH_FMT_COUNTERVALUE::default();
            (unsafe { PdhGetFormattedCounterValue(c, PDH_FMT_DOUBLE, None, &mut v) } == 0 && v.CStatus == 0)
                .then(|| (100.0 - unsafe { v.Anonymous.doubleValue } as f32).clamp(0.0, 100.0))
        })
        .flatten()
    }

    /// Engine instance names look like `pid_1234_luid_0x00000000_0x0000a5b1_phys_0_eng_0_engtype_3D`.
    fn parse_engine(name: &str) -> Option<(u32, String, String)> {
        let rest = name.strip_prefix("pid_")?;
        let (pid, rest) = rest.split_once("_luid_")?;
        let (luid, engine) = rest.split_once("_phys_")?;
        Some((pid.parse().ok()?, luid.to_lowercase(), engine.to_string()))
    }

    pub fn gpus(nvidia: &[GpuInfo]) -> Vec<GpuInfo> {
        with_pdh(|p| {
            // an engine's load is the sum over processes; an adapter's load is its busiest engine
            let mut engines: HashMap<(String, String), f64> = HashMap::new();
            if let Some(c) = p.engine {
                for (name, v) in array(c) {
                    if let Some((_, luid, eng)) = parse_engine(&name) {
                        *engines.entry((luid, eng)).or_default() += v;
                    }
                }
            }
            let mut used: HashMap<String, u64> = HashMap::new();
            if let Some(c) = p.mem_used {
                for (name, v) in array(c) {
                    // "luid_0x00000000_0x0000a5b1_phys_0"
                    if let Some(rest) = name.strip_prefix("luid_") {
                        let luid = rest.split("_phys_").next().unwrap_or(rest).to_lowercase();
                        *used.entry(luid).or_default() += v as u64;
                    }
                }
            }
            p.adapters
                .iter()
                .map(|a| {
                    let usage = engines
                        .iter()
                        .filter(|((l, _), _)| *l == a.luid)
                        .map(|(_, v)| *v)
                        .fold(None, |m: Option<f64>, v| Some(m.map_or(v, |x| x.max(v))))
                        .map(|v| v.min(100.0) as f32);
                    let temp = nvidia
                        .iter()
                        .find(|n| a.name.to_lowercase().contains("nvidia") && n.name.to_lowercase().contains("nvidia"))
                        .and_then(|n| n.temp);
                    GpuInfo {
                        name: a.name.clone(),
                        usage,
                        mem_used: used.get(&a.luid).copied(),
                        mem_total: (a.mem_total > 0).then_some(a.mem_total),
                        temp,
                    }
                })
                .collect()
        })
        .unwrap_or_default()
    }

    /// GPU use per process: its busiest engine, like Task Manager's GPU column.
    pub fn process_gpu() -> ProcGpu {
        with_pdh(|p| {
            let mut usage: HashMap<u32, f32> = HashMap::new();
            if let Some(c) = p.engine {
                for (name, v) in array(c) {
                    if let Some((pid, _, _)) = parse_engine(&name) {
                        let e = usage.entry(pid).or_default();
                        *e = e.max(v as f32);
                    }
                }
            }
            ProcGpu { usage, supported: p.engine.is_some() }
        })
        .unwrap_or_default()
    }
}
