//! Memory accounting that matches what Task Manager shows, plus the app's own footprint.
//!
//! sysinfo reports the *working set* of a process, which includes shared pages (DLLs, shared
//! caches) that every process maps. Summed over many processes (Chrome, WebView2…) it is far larger
//! than what the machine really spends. Task Manager shows the *private working set* instead, so on
//! Windows we ask for that and fall back to sysinfo's number when it is unavailable.

use std::collections::HashMap;

use serde::Serialize;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// Private working set (bytes) per pid. Windows only; empty elsewhere (callers fall back to sysinfo).
#[cfg(windows)]
pub fn private_working_set(pids: &[u32]) -> HashMap<u32, u64> {
    use std::mem::size_of;

    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    let mut out = HashMap::with_capacity(pids.len());
    for &pid in pids {
        unsafe {
            let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { continue };
            let mut c = PROCESS_MEMORY_COUNTERS_EX2::default();
            c.cb = size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32;
            // PROCESS_MEMORY_COUNTERS_EX2 is a superset of PROCESS_MEMORY_COUNTERS; the API takes the base pointer
            if GetProcessMemoryInfo(h, &mut c as *mut _ as *mut PROCESS_MEMORY_COUNTERS, c.cb).is_ok() && c.PrivateWorkingSetSize > 0 {
                out.insert(pid, c.PrivateWorkingSetSize as u64);
            }
            let _ = CloseHandle(h);
        }
    }
    out
}

#[cfg(not(windows))]
pub fn private_working_set(_pids: &[u32]) -> HashMap<u32, u64> {
    HashMap::new()
}

#[derive(Serialize)]
pub struct SelfUsage {
    /// this process (the Rust side)
    pub app: u64,
    /// all descendant processes (the WebView renderer, GPU and helper processes)
    pub webview: u64,
    pub webview_count: usize,
}

/// Memory used by this app: its own process plus every process it spawned.
pub fn self_usage() -> Option<SelfUsage> {
    let mut sys = System::new().ok()?;
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_memory());

    let me = std::process::id();
    // breadth-first walk down the parent → child links
    let mut family = vec![me];
    let mut i = 0;
    while i < family.len() {
        let parent = Pid::from_u32(family[i]);
        for (pid, p) in sys.processes() {
            if p.parent() == Some(parent) && !family.contains(&pid.as_u32()) {
                family.push(pid.as_u32());
            }
        }
        i += 1;
    }

    let private = private_working_set(&family);
    let bytes = |pid: u32| {
        private
            .get(&pid)
            .copied()
            .or_else(|| sys.process(Pid::from_u32(pid)).map(|p| p.memory()))
            .unwrap_or(0)
    };
    Some(SelfUsage {
        app: bytes(me),
        webview: family.iter().skip(1).map(|&p| bytes(p)).sum(),
        webview_count: family.len() - 1,
    })
}

/// Ask WebView2 to keep its memory low (it trims caches and the working set of the renderer).
/// The UI is light, so the small performance cost is irrelevant.
#[cfg(windows)]
pub fn low_memory(w: &tauri::WebviewWindow) {
    let _ = w.with_webview(|wv| unsafe {
        use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW};
        use windows::core::Interface;

        if let Ok(core) = wv.controller().CoreWebView2() {
            if let Ok(core19) = core.cast::<ICoreWebView2_19>() {
                let _ = core19.SetMemoryUsageTargetLevel(COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW);
            }
        }
    });
}

#[cfg(not(windows))]
pub fn low_memory(_w: &tauri::WebviewWindow) {}
