//! Network use per process.
//!
//! No operating system offers this as a simple query, so each one needs its own trick:
//! - Windows: an ETW session on the Microsoft-Windows-Kernel-Network provider (needs administrator rights);
//! - macOS: the `nettop` tool in streaming mode (no special rights);
//! - Linux: per-socket TCP byte counters from `ss` (only for the processes the user may inspect;
//!   UDP is not covered).
//!
//! A collector thread starts the first time the data is requested and keeps the latest rates.

use std::{collections::HashMap, sync::Mutex, sync::Once};

use serde::Serialize;

#[derive(Serialize, Clone, Default)]
pub struct NetRates {
    /// false when this system cannot provide the data right now (see `needs_admin`)
    pub supported: bool,
    /// Windows without administrator rights: restarting as administrator would enable it
    pub needs_admin: bool,
    /// pid -> [received, sent] in bytes per second
    pub rates: HashMap<u32, [f64; 2]>,
}

static RATES: Mutex<Option<NetRates>> = Mutex::new(None);
static START: Once = Once::new();

fn publish(r: NetRates) {
    *RATES.lock().unwrap() = Some(r);
}

pub fn per_process() -> NetRates {
    START.call_once(|| {
        std::thread::spawn(collector);
    });
    RATES.lock().unwrap().clone().unwrap_or_default()
}

/// Stop anything that outlives the process (the Windows ETW session). Called when the app exits.
pub fn shutdown() {
    #[cfg(windows)]
    win::stop();
}

fn collector() {
    #[cfg(windows)]
    win::run();
    #[cfg(target_os = "macos")]
    macos_run();
    #[cfg(target_os = "linux")]
    linux_run();
}

// ------------------------------------------------------------------ Linux

#[cfg(target_os = "linux")]
fn linux_run() {
    use std::{process::Command, thread::sleep, time::{Duration, Instant}};

    // socket key -> (pid, bytes sent, bytes received)
    let mut prev: HashMap<String, (u32, u64, u64)> = HashMap::new();
    let mut last = Instant::now();
    let mut first = true;
    loop {
        sleep(Duration::from_secs(2));
        let Ok(out) = Command::new("ss").args(["-tinpH"]).output() else {
            publish(NetRates::default());
            return;
        };
        let text = String::from_utf8_lossy(&out.stdout);

        let mut now: HashMap<String, (u32, u64, u64)> = HashMap::new();
        let mut current: Option<(String, u32)> = None;
        for line in text.lines() {
            if !line.starts_with(char::is_whitespace) {
                // "ESTAB 0 0 local:port peer:port users:(("firefox",pid=1234,fd=70))"
                let f: Vec<&str> = line.split_whitespace().collect();
                let pid = line
                    .split("pid=")
                    .nth(1)
                    .and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next())
                    .and_then(|p| p.parse::<u32>().ok());
                current = match (f.get(3), f.get(4), pid) {
                    (Some(l), Some(p), Some(pid)) => Some((format!("{l}>{p}#{pid}"), pid)),
                    _ => None,
                };
            } else if let Some((key, pid)) = &current {
                let num = |name: &str| {
                    line.split_whitespace()
                        .find_map(|t| t.strip_prefix(name))
                        .and_then(|v| v.parse::<u64>().ok())
                };
                if let (Some(s), Some(r)) = (num("bytes_sent:"), num("bytes_received:")) {
                    now.insert(key.clone(), (*pid, s, r));
                }
            }
        }

        let secs = last.elapsed().as_secs_f64().max(0.001);
        last = Instant::now();
        let mut rates: HashMap<u32, [f64; 2]> = HashMap::new();
        if !first {
            for (k, (pid, s, r)) in &now {
                // a socket seen for the first time has no baseline yet, so it counts from the next round
                if let Some((_, ps, pr)) = prev.get(k) {
                    let e = rates.entry(*pid).or_default();
                    e[0] += r.saturating_sub(*pr) as f64 / secs;
                    e[1] += s.saturating_sub(*ps) as f64 / secs;
                }
            }
        }
        first = false;
        prev = now;
        publish(NetRates { supported: true, needs_admin: false, rates });
    }
}

// ------------------------------------------------------------------ macOS

#[cfg(target_os = "macos")]
fn macos_run() {
    use std::{
        io::{BufRead, BufReader},
        process::{Command, Stdio},
    };

    // -P: per process, -d: deltas per sample, -L 0: stream CSV forever, -s 1: every second, -x: raw numbers
    let child = Command::new("nettop")
        .args(["-P", "-d", "-L", "0", "-s", "1", "-x", "-J", "bytes_in,bytes_out"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        publish(NetRates::default());
        return;
    };
    let Some(stdout) = child.stdout.take() else { return };

    let mut batch: HashMap<u32, [f64; 2]> = HashMap::new();
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        let f: Vec<&str> = line.split(',').collect();
        if f.first().is_some_and(|c| c.trim_start().starts_with("time")) {
            // a header line starts a new sample: publish the previous one
            if !batch.is_empty() {
                publish(NetRates { supported: true, needs_admin: false, rates: std::mem::take(&mut batch) });
            }
            continue;
        }
        // "23:08:13.123456,Safari.1234,123,456,"
        if f.len() >= 4 {
            let pid = f[1].rsplit('.').next().and_then(|p| p.parse::<u32>().ok());
            if let (Some(pid), Ok(rx), Ok(tx)) = (pid, f[2].parse::<f64>(), f[3].parse::<f64>()) {
                let e = batch.entry(pid).or_default();
                e[0] += rx;
                e[1] += tx;
            }
        }
    }
    let _ = child.kill();
}

// ------------------------------------------------------------------ Windows (ETW)

#[cfg(windows)]
mod win {
    use std::{
        collections::HashMap,
        mem::size_of,
        sync::Mutex,
        thread::sleep,
        time::{Duration, Instant},
    };

    use windows::core::{GUID, PCWSTR, PWSTR};
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Diagnostics::Etw::*;

    use super::{publish, NetRates};

    const SESSION: &str = "LightweightTaskManagerNet";
    /// Microsoft-Windows-Kernel-Network
    const PROVIDER: GUID = GUID::from_u128(0x7dd42a49_5329_4832_8dfd_43d979153a88);

    /// pid -> (bytes received, bytes sent) since the session started
    static TOTALS: Mutex<Option<HashMap<u32, (u64, u64)>>> = Mutex::new(None);
    static SESSION_HANDLE: Mutex<u64> = Mutex::new(0);

    fn name_w() -> Vec<u16> {
        SESSION.encode_utf16().chain(Some(0)).collect()
    }

    /// Properties block: the struct followed by room for the session name.
    fn properties() -> Vec<u64> {
        let size = size_of::<EVENT_TRACE_PROPERTIES>() + name_w().len() * 2;
        let mut buf = vec![0u64; size / 8 + 1];
        let p = buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES;
        unsafe {
            (*p).Wnode.BufferSize = size as u32;
            (*p).Wnode.Flags = WNODE_FLAG_TRACED_GUID;
            (*p).Wnode.ClientContext = 1; // QPC timestamps
            (*p).LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
            (*p).LoggerNameOffset = size_of::<EVENT_TRACE_PROPERTIES>() as u32;
        }
        buf
    }

    pub fn stop() {
        let name = name_w();
        let mut props = properties();
        unsafe {
            let _ = ControlTraceW(
                CONTROLTRACE_HANDLE { Value: *SESSION_HANDLE.lock().unwrap() },
                PCWSTR(name.as_ptr()),
                props.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES,
                EVENT_TRACE_CONTROL_STOP,
            );
        }
    }

    /// Called by ETW for every event; only the TCP/UDP send and receive events of the provider matter.
    unsafe extern "system" fn on_event(rec: *mut EVENT_RECORD) {
        let r = &*rec;
        if r.EventHeader.ProviderId != PROVIDER || r.UserDataLength < 8 || r.UserData.is_null() {
            return;
        }
        // UserData starts with: u32 pid, u32 size
        let d = r.UserData as *const u32;
        let (pid, size) = (*d, *d.add(1) as u64);
        let (sent, recv) = match r.EventHeader.EventDescriptor.Id {
            10 | 26 | 42 | 58 => (size, 0), // TCP/UDP send, IPv4/IPv6
            11 | 27 | 43 | 59 => (0, size), // TCP/UDP receive, IPv4/IPv6
            _ => return,
        };
        if let Some(m) = TOTALS.lock().unwrap().as_mut() {
            let e = m.entry(pid).or_default();
            e.0 += recv;
            e.1 += sent;
        }
    }

    pub fn run() {
        // An ETW session needs administrator rights
        if !crate::admin::is_elevated() {
            publish(NetRates { supported: false, needs_admin: true, rates: HashMap::new() });
            return;
        }
        *TOTALS.lock().unwrap() = Some(HashMap::new());

        // A session left behind by a crashed run would make StartTrace fail, so remove it first
        stop();

        let name = name_w();
        let mut props = properties();
        let mut handle = CONTROLTRACE_HANDLE::default();
        unsafe {
            let st = StartTraceW(&mut handle, PCWSTR(name.as_ptr()), props.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES);
            if st != ERROR_SUCCESS {
                publish(NetRates::default());
                return;
            }
            *SESSION_HANDLE.lock().unwrap() = handle.Value;
            // keywords 0x10 | 0x20: IPv4 and IPv6 traffic
            let st = EnableTraceEx2(handle, &PROVIDER, EVENT_CONTROL_CODE_ENABLE_PROVIDER.0, TRACE_LEVEL_INFORMATION as u8, 0x30, 0, 0, None);
            if st != ERROR_SUCCESS {
                stop();
                publish(NetRates::default());
                return;
            }
        }

        // Consumer: ProcessTrace blocks, so it gets a thread of its own
        std::thread::spawn(move || unsafe {
            let mut name = name_w();
            let mut logfile = EVENT_TRACE_LOGFILEW::default();
            logfile.LoggerName = PWSTR(name.as_mut_ptr());
            logfile.Anonymous1.ProcessTraceMode = PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD;
            logfile.Anonymous2.EventRecordCallback = Some(on_event);
            let th = OpenTraceW(&mut logfile);
            let _ = ProcessTrace(&[th], None, None);
        });

        // Turn the running totals into per-second rates
        let mut prev: HashMap<u32, (u64, u64)> = HashMap::new();
        let mut last = Instant::now();
        loop {
            sleep(Duration::from_secs(1));
            let now = TOTALS.lock().unwrap().clone().unwrap_or_default();
            let secs = last.elapsed().as_secs_f64().max(0.001);
            last = Instant::now();
            let rates = now
                .iter()
                .filter_map(|(pid, (rx, tx))| {
                    let (prx, ptx) = prev.get(pid).copied().unwrap_or((*rx, *tx));
                    let r = [rx.saturating_sub(prx) as f64 / secs, tx.saturating_sub(ptx) as f64 / secs];
                    (r[0] > 0.0 || r[1] > 0.0).then_some((*pid, r))
                })
                .collect();
            prev = now;
            publish(NetRates { supported: true, needs_admin: false, rates });
        }
    }

}
