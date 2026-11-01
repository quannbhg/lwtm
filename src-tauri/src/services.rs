//! List and control system services. Supported on Windows (SCM) and Linux (systemd).

use serde::Serialize;

#[derive(Serialize)]
pub struct ServiceInfo {
    /// Internal name (used to control the service)
    pub name: String,
    pub display: String,
    /// running | stopped | paused | pending
    pub status: &'static str,
    /// auto | manual | disabled | boot | system
    pub start_type: &'static str,
    pub pid: Option<u32>,
    pub description: Option<String>,
    pub can_stop: bool,
    /// Full launch command line (including arguments)
    pub binary: Option<String>,
    /// Account the service runs as (LocalSystem, NT AUTHORITY\LocalService…)
    pub account: Option<String>,
}

#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Start,
    Stop,
    Restart,
}

#[cfg(windows)]
pub use win::{control, list};

#[cfg(target_os = "linux")]
pub use linux::{control, list};

#[cfg(not(any(windows, target_os = "linux")))]
pub fn list() -> Result<Vec<ServiceInfo>, String> {
    Err(unsupported())
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn control(_name: &str, _action: Action) -> Result<(), String> {
    Err(unsupported())
}

#[cfg(not(any(windows, target_os = "linux")))]
fn unsupported() -> String {
    crate::i18n::tr(
        "Quản lý dịch vụ chưa được hỗ trợ trên hệ điều hành này",
        "Service management is not supported on this operating system",
    )
}

/// Linux: read and control systemd services through `systemctl`.
#[cfg(target_os = "linux")]
mod linux {
    use std::process::Command;

    use super::{Action, ServiceInfo};

    const PROPS: &str = "Id,Description,ActiveState,UnitFileState,MainPID,ExecStart,User,CanStop";

    pub fn list() -> Result<Vec<ServiceInfo>, String> {
        let out = Command::new("systemctl")
            .args(["show", "--no-pager", "--all", "--type=service", "-p", PROPS, "*.service"])
            .output()
            .map_err(|e| {
                crate::i18n::tr(
                    format!("Không chạy được systemctl (hệ thống không dùng systemd?): {e}"),
                    format!("Could not run systemctl (is this system using systemd?): {e}"),
                )
            })?;
        let text = String::from_utf8_lossy(&out.stdout);

        let mut res = Vec::new();
        for block in text.split("\n\n") {
            let get = |k: &str| {
                block
                    .lines()
                    .find_map(|l| l.strip_prefix(k).and_then(|r| r.strip_prefix('=')))
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
            };
            let Some(id) = get("Id") else { continue };
            let name = id.trim_end_matches(".service").to_string();
            let desc = get("Description").map(String::from);

            // ExecStart={ path=/usr/bin/x ; argv[]=/usr/bin/x -a ; ignore_errors=no ; ... }
            let binary = get("ExecStart").and_then(|e| {
                let a = e.find("argv[]=")? + 7;
                Some(e[a..].split(" ; ").next()?.trim().to_string())
            });
            let pid = get("MainPID").and_then(|p| p.parse::<u32>().ok()).filter(|&p| p != 0);

            res.push(ServiceInfo {
                display: desc.clone().unwrap_or_else(|| name.clone()),
                name,
                status: match get("ActiveState") {
                    Some("active") => "running",
                    Some("activating") | Some("deactivating") | Some("reloading") => "pending",
                    _ => "stopped",
                },
                start_type: match get("UnitFileState") {
                    Some("enabled") | Some("enabled-runtime") => "auto",
                    Some("masked") | Some("masked-runtime") => "disabled",
                    _ => "manual",
                },
                pid,
                description: desc,
                can_stop: get("CanStop") == Some("yes") && get("ActiveState") == Some("active"),
                binary,
                account: Some(get("User").unwrap_or("root").to_string()),
            });
        }
        if res.is_empty() && !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        Ok(res)
    }

    pub fn control(name: &str, action: Action) -> Result<(), String> {
        let verb = match action {
            Action::Start => "start",
            Action::Stop => "stop",
            Action::Restart => "restart",
        };
        let out = Command::new("systemctl")
            .args([verb, &format!("{name}.service")])
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(())
        } else {
            let err = String::from_utf8_lossy(&out.stderr);
            let msg = err.trim();
            Err(crate::i18n::tr(
                format!("{msg} (có thể cần chạy app bằng sudo)"),
                format!("{msg} (you may need to run the app with sudo)"),
            ))
        }
    }
}

#[cfg(windows)]
mod win {
    use std::{
        collections::HashMap,
        sync::{Mutex, OnceLock},
        thread::sleep,
        time::{Duration, Instant},
    };

    use windows::core::{Error, PCWSTR};
    use windows::Win32::Foundation::ERROR_ACCESS_DENIED;
    use windows::Win32::System::Services::*;
    use windows::Win32::UI::Shell::SHLoadIndirectString;

    use super::{Action, ServiceInfo};

    struct Handle(SC_HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseServiceHandle(self.0);
            }
        }
    }

    fn err(e: Error) -> String {
        if e.code() == ERROR_ACCESS_DENIED.to_hresult() {
            crate::i18n::tr(
                "Bị từ chối truy cập — hãy chạy ứng dụng với quyền Administrator",
                "Access denied — run the app as Administrator",
            )
        } else {
            e.message()
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    /// Service descriptions rarely change and resolving "@dll,-id" strings is slow, so cache them.
    fn desc_cache() -> &'static Mutex<HashMap<String, Option<String>>> {
        static C: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
        C.get_or_init(Default::default)
    }

    unsafe fn description(h: &Handle) -> Option<String> {
        let mut need = 0u32;
        let _ = QueryServiceConfig2W(h.0, SERVICE_CONFIG_DESCRIPTION, None, &mut need);
        if need == 0 {
            return None;
        }
        let mut buf = vec![0u64; need as usize / 8 + 1];
        let bytes = std::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut u8, buf.len() * 8);
        QueryServiceConfig2W(h.0, SERVICE_CONFIG_DESCRIPTION, Some(bytes), &mut need).ok()?;
        let d = &*(buf.as_ptr() as *const SERVICE_DESCRIPTIONW);
        if d.lpDescription.is_null() {
            return None;
        }
        let s = d.lpDescription.to_string().ok()?;
        if s.starts_with('@') {
            let src = wide(&s);
            let mut out = [0u16; 512];
            SHLoadIndirectString(PCWSTR(src.as_ptr()), &mut out, None).ok()?;
            let n = out.iter().position(|&c| c == 0).unwrap_or(out.len());
            Some(String::from_utf16_lossy(&out[..n]))
        } else {
            Some(s)
        }
    }

    /// (start type, command line, account)
    unsafe fn config(h: &Handle) -> (&'static str, Option<String>, Option<String>) {
        let mut need = 0u32;
        let _ = QueryServiceConfigW(h.0, None, 0, &mut need);
        if need == 0 {
            return ("manual", None, None);
        }
        let mut buf = vec![0u64; need as usize / 8 + 1];
        let cfg = buf.as_mut_ptr() as *mut QUERY_SERVICE_CONFIGW;
        if QueryServiceConfigW(h.0, Some(cfg), (buf.len() * 8) as u32, &mut need).is_err() {
            return ("manual", None, None);
        }
        let c = &*cfg;
        let start = match c.dwStartType {
            t if t == SERVICE_AUTO_START => "auto",
            t if t == SERVICE_DEMAND_START => "manual",
            t if t == SERVICE_DISABLED => "disabled",
            t if t == SERVICE_BOOT_START => "boot",
            _ => "system",
        };
        let s = |p: windows::core::PWSTR| if p.is_null() { None } else { p.to_string().ok().filter(|x| !x.is_empty()) };
        (start, s(c.lpBinaryPathName), s(c.lpServiceStartName))
    }

    pub fn list() -> Result<Vec<ServiceInfo>, String> {
        unsafe {
            let scm = Handle(OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_ENUMERATE_SERVICE).map_err(err)?);

            let (mut need, mut count, mut resume) = (0u32, 0u32, 0u32);
            let _ = EnumServicesStatusExW(
                scm.0, SC_ENUM_PROCESS_INFO, SERVICE_WIN32, SERVICE_STATE_ALL,
                None, &mut need, &mut count, Some(&mut resume), PCWSTR::null(),
            );
            // u64 so the buffer is aligned enough for ENUM_SERVICE_STATUS_PROCESSW
            let mut buf = vec![0u64; need as usize / 8 + 8];
            let bytes = std::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut u8, buf.len() * 8);
            resume = 0;
            EnumServicesStatusExW(
                scm.0, SC_ENUM_PROCESS_INFO, SERVICE_WIN32, SERVICE_STATE_ALL,
                Some(bytes), &mut need, &mut count, Some(&mut resume), PCWSTR::null(),
            )
            .map_err(err)?;

            let items = std::slice::from_raw_parts(buf.as_ptr() as *const ENUM_SERVICE_STATUS_PROCESSW, count as usize);
            let mut out = Vec::with_capacity(items.len());
            for it in items {
                let name = it.lpServiceName.to_string().unwrap_or_default();
                let st = &it.ServiceStatusProcess;
                let status = match st.dwCurrentState {
                    s if s == SERVICE_RUNNING => "running",
                    s if s == SERVICE_STOPPED => "stopped",
                    s if s == SERVICE_PAUSED => "paused",
                    _ => "pending",
                };

                let (mut start, mut desc, mut binary, mut account) = ("manual", None, None, None);
                let wname = wide(&name);
                if let Ok(h) = OpenServiceW(scm.0, PCWSTR(wname.as_ptr()), SERVICE_QUERY_CONFIG) {
                    let h = Handle(h);
                    (start, binary, account) = config(&h);
                    let mut cache = desc_cache().lock().unwrap();
                    desc = cache.entry(name.clone()).or_insert_with(|| description(&h)).clone();
                }

                out.push(ServiceInfo {
                    display: it.lpDisplayName.to_string().unwrap_or_else(|_| name.clone()),
                    name,
                    status,
                    start_type: start,
                    pid: (st.dwProcessId != 0).then_some(st.dwProcessId),
                    description: desc,
                    can_stop: st.dwControlsAccepted & SERVICE_ACCEPT_STOP != 0,
                    binary,
                    account,
                });
            }
            Ok(out)
        }
    }

    unsafe fn open(name: &str, access: u32) -> Result<Handle, String> {
        let scm = Handle(OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT).map_err(err)?);
        let w = wide(name);
        OpenServiceW(scm.0, PCWSTR(w.as_ptr()), access).map(Handle).map_err(err)
    }

    unsafe fn stop(name: &str) -> Result<(), String> {
        let h = open(name, SERVICE_STOP | SERVICE_QUERY_STATUS)?;
        let mut st = SERVICE_STATUS::default();
        ControlService(h.0, SERVICE_CONTROL_STOP, &mut st).map_err(err)?;
        // wait for the service to stop completely (up to 15s) so Restart does not start too early
        let t0 = Instant::now();
        while st.dwCurrentState != SERVICE_STOPPED && t0.elapsed() < Duration::from_secs(15) {
            sleep(Duration::from_millis(200));
            if QueryServiceStatus(h.0, &mut st).is_err() {
                break;
            }
        }
        Ok(())
    }

    unsafe fn start(name: &str) -> Result<(), String> {
        let h = open(name, SERVICE_START)?;
        StartServiceW(h.0, None).map_err(err)
    }

    pub fn control(name: &str, action: Action) -> Result<(), String> {
        unsafe {
            match action {
                Action::Start => start(name),
                Action::Stop => stop(name),
                Action::Restart => {
                    stop(name)?;
                    start(name)
                }
            }
        }
    }
}
