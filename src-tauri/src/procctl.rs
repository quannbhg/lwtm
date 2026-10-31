//! Extra process info (32/64-bit platform, priority, efficiency mode) and process adjustment.
//! Supported on Windows and Unix (Linux/macOS, via `nice`).

use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct Extra {
    pub platform: &'static str,
    pub priority: &'static str,
    pub efficiency: bool,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Realtime,
    High,
    AboveNormal,
    Normal,
    BelowNormal,
    Idle,
}

#[cfg(windows)]
pub use win::{extras, set_efficiency, set_priority};

#[cfg(unix)]
pub use unix::{extras, set_efficiency, set_priority};

/// macOS/Linux: uses the process "nice" value. "Efficiency mode" = nice 19 (lowest priority).
#[cfg(unix)]
mod unix {
    use std::collections::HashMap;

    use super::{Extra, Priority};

    const EFFICIENT_NICE: i32 = 19;

    fn nice_of(p: Priority) -> i32 {
        match p {
            Priority::Realtime => -20,
            Priority::High => -10,
            Priority::AboveNormal => -5,
            Priority::Normal => 0,
            Priority::BelowNormal => 5,
            Priority::Idle => EFFICIENT_NICE,
        }
    }

    fn label(nice: i32) -> &'static str {
        match nice {
            n if n <= -15 => "realtime",
            n if n <= -6 => "high",
            n if n < 0 => "above_normal",
            0 => "normal",
            n if n < 15 => "below_normal",
            _ => "idle",
        }
    }

    pub fn extras(pids: &[u32]) -> HashMap<u32, Extra> {
        let platform = if cfg!(target_arch = "aarch64") {
            "ARM64"
        } else if cfg!(target_pointer_width = "64") {
            "64 bit"
        } else {
            "32 bit"
        };
        pids.iter()
            .map(|&pid| {
                let nice = unsafe { libc::getpriority(libc::PRIO_PROCESS, pid as libc::id_t) };
                (pid, Extra { platform, priority: label(nice), efficiency: nice >= 15 })
            })
            .collect()
    }

    fn apply(pids: &[u32], nice: i32) -> Result<(), String> {
        let failed = pids
            .iter()
            .filter(|&&pid| unsafe { libc::setpriority(libc::PRIO_PROCESS, pid as libc::id_t, nice) } != 0)
            .count();
        if failed == 0 {
            Ok(())
        } else {
            Err(format!(
                "Không thể thay đổi {failed}/{} tiến trình (không đủ quyền: chỉ root mới được tăng ưu tiên hoặc đổi tiến trình của người khác)",
                pids.len()
            ))
        }
    }

    pub fn set_priority(pids: &[u32], p: Priority) -> Result<(), String> {
        apply(pids, nice_of(p))
    }

    pub fn set_efficiency(pids: &[u32], on: bool) -> Result<(), String> {
        apply(pids, if on { EFFICIENT_NICE } else { 0 })
    }
}

#[cfg(not(any(windows, unix)))]
pub fn extras(_pids: &[u32]) -> std::collections::HashMap<u32, Extra> {
    Default::default()
}
#[cfg(not(any(windows, unix)))]
pub fn set_priority(_pids: &[u32], _p: Priority) -> Result<(), String> {
    Err("Đặt độ ưu tiên chưa được hỗ trợ trên hệ điều hành này".into())
}
#[cfg(not(any(windows, unix)))]
pub fn set_efficiency(_pids: &[u32], _on: bool) -> Result<(), String> {
    Err("Chế độ tiết kiệm chưa được hỗ trợ trên hệ điều hành này".into())
}

#[cfg(windows)]
mod win {
    use std::{collections::HashMap, ffi::c_void, mem::size_of};

    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Threading::*;

    use super::{Extra, Priority};

    struct Proc(HANDLE);
    impl Drop for Proc {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    fn open(pid: u32, access: PROCESS_ACCESS_RIGHTS) -> Option<Proc> {
        unsafe { OpenProcess(access, false, pid).ok().map(Proc) }
    }

    pub fn extras(pids: &[u32]) -> HashMap<u32, Extra> {
        let mut out = HashMap::new();
        for &pid in pids {
            let Some(p) = open(pid, PROCESS_QUERY_LIMITED_INFORMATION) else { continue };
            unsafe {
                let (mut proc_m, mut native_m) = (Default::default(), Default::default());
                let platform = if IsWow64Process2(p.0, &mut proc_m, Some(&mut native_m)).is_ok() {
                    if proc_m.0 != 0 {
                        "32 bit"
                    } else if native_m.0 == 0xAA64 {
                        "ARM64"
                    } else {
                        "64 bit"
                    }
                } else {
                    "–"
                };

                let priority = match GetPriorityClass(p.0) {
                    x if x == REALTIME_PRIORITY_CLASS.0 => "realtime",
                    x if x == HIGH_PRIORITY_CLASS.0 => "high",
                    x if x == ABOVE_NORMAL_PRIORITY_CLASS.0 => "above_normal",
                    x if x == BELOW_NORMAL_PRIORITY_CLASS.0 => "below_normal",
                    x if x == IDLE_PRIORITY_CLASS.0 => "idle",
                    _ => "normal",
                };

                let mut st = PROCESS_POWER_THROTTLING_STATE {
                    Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
                    ..Default::default()
                };
                let efficiency = GetProcessInformation(
                    p.0,
                    ProcessPowerThrottling,
                    &mut st as *mut _ as *mut c_void,
                    size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
                )
                .is_ok()
                    && st.StateMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED != 0;

                out.insert(pid, Extra { platform, priority, efficiency });
            }
        }
        out
    }

    fn class(p: Priority) -> PROCESS_CREATION_FLAGS {
        match p {
            Priority::Realtime => REALTIME_PRIORITY_CLASS,
            Priority::High => HIGH_PRIORITY_CLASS,
            Priority::AboveNormal => ABOVE_NORMAL_PRIORITY_CLASS,
            Priority::Normal => NORMAL_PRIORITY_CLASS,
            Priority::BelowNormal => BELOW_NORMAL_PRIORITY_CLASS,
            Priority::Idle => IDLE_PRIORITY_CLASS,
        }
    }

    fn apply(pids: &[u32], f: impl Fn(&Proc) -> bool) -> Result<(), String> {
        let failed = pids
            .iter()
            .filter(|&&pid| !open(pid, PROCESS_SET_INFORMATION).map(|p| f(&p)).unwrap_or(false))
            .count();
        if failed == 0 {
            Ok(())
        } else {
            Err(format!(
                "Không thể thay đổi {failed}/{} tiến trình (tiến trình được bảo vệ hoặc cần quyền Administrator)",
                pids.len()
            ))
        }
    }

    pub fn set_priority(pids: &[u32], p: Priority) -> Result<(), String> {
        apply(pids, |h| unsafe { SetPriorityClass(h.0, class(p)).is_ok() })
    }

    pub fn set_efficiency(pids: &[u32], on: bool) -> Result<(), String> {
        apply(pids, |h| unsafe {
            let st = PROCESS_POWER_THROTTLING_STATE {
                Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
                ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
                StateMask: if on { PROCESS_POWER_THROTTLING_EXECUTION_SPEED } else { 0 },
            };
            let throttled = SetProcessInformation(
                h.0,
                ProcessPowerThrottling,
                &st as *const _ as *const c_void,
                size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
            )
            .is_ok();
            // Like Task Manager: also drop priority to idle when enabled, restore to normal when disabled
            let prio = SetPriorityClass(h.0, if on { IDLE_PRIORITY_CLASS } else { NORMAL_PRIORITY_CLASS }).is_ok();
            throttled && prio
        })
    }
}
