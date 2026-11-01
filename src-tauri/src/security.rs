//! Heuristic scan of services to spot suspicious signs.
//!
//! This is NOT an antivirus: it only scores signs commonly seen in malware installed as a service
//! (temp-folder location, impersonating system process names, no digital signature, running PowerShell…).
//! A "suspicious" result means "worth a closer look", not a verdict that it is a virus.

use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{
    i18n::tr,
    services::{self, ServiceInfo},
    startup::{self, StartupEntry},
};

/// Anything the scanner can judge: a service, a running application or a startup entry.
pub struct Subject {
    /// service | process | startup
    kind: &'static str,
    name: String,
    display: String,
    /// running | stopped for services/processes, enabled | disabled for startup entries
    status: &'static str,
    /// Full command line (the executable comes first)
    binary: Option<String>,
    account: Option<String>,
    description: Option<String>,
    pids: Vec<u32>,
    source: Option<String>,
    key: Option<String>,
}

impl From<&ServiceInfo> for Subject {
    fn from(s: &ServiceInfo) -> Self {
        Subject {
            kind: "service",
            name: s.name.clone(),
            display: s.display.clone(),
            status: s.status,
            binary: s.binary.clone(),
            account: s.account.clone(),
            description: s.description.clone(),
            pids: s.pid.into_iter().collect(),
            source: None,
            key: None,
        }
    }
}

impl From<&StartupEntry> for Subject {
    fn from(e: &StartupEntry) -> Self {
        Subject {
            kind: "startup",
            name: e.name.clone(),
            display: e.name.clone(),
            status: if e.enabled { "enabled" } else { "disabled" },
            binary: Some(e.command.clone()),
            account: None,
            description: None,
            pids: Vec::new(),
            source: Some(e.source.to_string()),
            key: Some(e.key.clone()),
        }
    }
}

/// One subject per distinct executable; all its running processes are collected in `pids`.
fn running_subjects() -> Result<Vec<Subject>, String> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

    let mut sys = System::new().map_err(|e| e.to_string())?;
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet).with_cmd(UpdateKind::OnlyIfNotSet),
    );
    let mut by_exe: std::collections::HashMap<String, Subject> = Default::default();
    for (pid, p) in sys.processes() {
        let Some(exe) = p.exe() else { continue };
        let exe_s = exe.to_string_lossy().into_owned();
        let s = by_exe.entry(exe_s.to_lowercase()).or_insert_with(|| {
            // quote the exe so paths with spaces parse correctly, then append the real arguments
            let args: Vec<String> = p.cmd().iter().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
            let name = p.name().to_string_lossy().into_owned();
            Subject {
                kind: "process",
                display: name.clone(),
                name,
                status: "running",
                binary: Some(format!("\"{exe_s}\" {}", args.join(" ")).trim_end().to_string()),
                account: None,
                description: None,
                pids: Vec::new(),
                source: None,
                key: None,
            }
        });
        s.pids.push(pid.as_u32());
    }
    Ok(by_exe.into_values().collect())
}

#[derive(Serialize)]
pub struct Finding {
    pub kind: &'static str,
    /// Running processes behind this finding (to end them)
    pub pids: Vec<u32>,
    /// For startup entries: where it is registered and its identifier (to disable it)
    pub source: Option<String>,
    pub key: Option<String>,
    pub name: String,
    pub display: String,
    pub status: &'static str,
    pub path: Option<String>,
    pub account: Option<String>,
    pub score: u32,
    /// ok | notice | suspicious
    pub verdict: &'static str,
    pub reasons: Vec<String>,
    /// Some(true) = valid signature; None = not checked / unknown
    pub signed: Option<bool>,
    pub sha256: Option<String>,
}

#[derive(PartialEq)]
#[cfg_attr(not(windows), allow(dead_code))] // digital signatures can only be checked on Windows
pub enum Sig {
    Signed,
    Unsigned,
    Invalid,
    Unknown,
}

/// System process names that malware often impersonates
const SYSTEM_NAMES: &[&str] = &[
    "svchost", "lsass", "csrss", "winlogon", "services", "smss", "wininit", "explorer", "spoolsv",
    "taskhost", "taskhostw", "dwm", "conhost", "wuauclt", "searchindexer", "audiodg", "dllhost",
    "runtimebroker", "ctfmon",
];

const LOLBINS: &[&str] = &[
    "powershell", "pwsh", "mshta", "wscript", "cscript", "regsvr32", "certutil", "bitsadmin",
    "cmd.exe /c", "cmd /c", "http://", "https://", "-encodedcommand", " -enc ",
];

/// (path fragment, score, Vietnamese label, English label)
const USER_DIRS: &[(&str, u32, &str, &str)] = &[
    ("\\temp\\", 4, "thư mục tạm", "a temp folder"),
    ("\\appdata\\", 4, "thư mục AppData của người dùng", "the user's AppData folder"),
    ("\\downloads\\", 4, "thư mục Downloads", "the Downloads folder"),
    ("\\users\\public\\", 4, "thư mục Public", "the Public folder"),
    ("$recycle.bin", 5, "Thùng rác", "the Recycle Bin"),
    // Linux / macOS
    ("/tmp/", 4, "thư mục tạm", "a temp folder"),
    ("/var/tmp/", 4, "thư mục tạm", "a temp folder"),
    ("/dev/shm/", 5, "bộ nhớ chia sẻ /dev/shm", "shared memory (/dev/shm)"),
    ("/downloads/", 4, "thư mục Downloads", "the Downloads folder"),
];

pub(crate) fn expand_env(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('%') {
            Some(j) => match std::env::var(&after[..j]) {
                Ok(v) => {
                    out.push_str(&v);
                    rest = &after[j + 1..];
                }
                Err(_) => {
                    out.push('%');
                    rest = after;
                }
            },
            None => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn windir() -> String {
    std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()).to_lowercase()
}

/// Extract the exe path from a command line. Returns (path, whether quotes are missing despite spaces in the path)
pub(crate) fn exe_path(binary: &str) -> (Option<PathBuf>, bool) {
    let b = binary.trim();
    let b = b.strip_prefix("\\??\\").unwrap_or(b);
    let (raw, quoted) = if let Some(rest) = b.strip_prefix('"') {
        (rest.split('"').next().unwrap_or(""), true)
    } else {
        let lower = b.to_ascii_lowercase();
        match lower.find(".exe").or_else(|| lower.find(".sys")) {
            Some(i) => (&b[..i + 4], false),
            None => (b.split_whitespace().next().unwrap_or(""), false),
        }
    };
    if raw.is_empty() {
        return (None, false);
    }
    let mut p = expand_env(raw);
    if p.to_lowercase().starts_with("system32\\") {
        p = format!("{}\\{}", std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()), p);
    }
    (Some(PathBuf::from(p)), !quoted && raw.contains(' '))
}

fn edit_distance_le1(a: &str, b: &str) -> bool {
    if a == b {
        return false;
    }
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let (mut i, mut j, mut diff) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            i += 1;
            j += 1;
            continue;
        }
        diff += 1;
        if diff > 1 {
            return false;
        }
        if a.len() > b.len() {
            i += 1;
        } else if b.len() > a.len() {
            j += 1;
        } else {
            i += 1;
            j += 1;
        }
    }
    diff + (a.len() - i) + (b.len() - j) <= 1
}

fn looks_random(name: &str) -> bool {
    if name.contains('_') || name.len() < 8 {
        return false;
    }
    let l = name.to_lowercase();
    let digits = l.chars().filter(|c| c.is_ascii_digit()).count();
    let letters = l.chars().filter(|c| c.is_ascii_alphabetic()).count();
    let vowels = l.chars().filter(|c| "aeiou".contains(*c)).count();
    (digits > 0 && letters > 0 && digits * 5 >= l.len()) || (vowels == 0 && letters >= 7)
}

fn sha256(path: &Path) -> Option<String> {
    let mut f = File::open(path).ok()?;
    if f.metadata().ok()?.len() > 200 << 20 {
        return None;
    }
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Some(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn analyze(s: &Subject) -> Finding {
    let mut score = 0u32;
    let mut reasons: Vec<String> = Vec::new();
    let mut add = |pts: u32, why: String| {
        score += pts;
        reasons.push(why);
    };

    let binary = s.binary.clone().unwrap_or_default();
    let (path, unquoted) = exe_path(&binary);
    let win = windir();
    let path_lc = path.as_ref().map(|p| p.to_string_lossy().to_lowercase());
    let in_windows = path_lc.as_deref().is_some_and(|p| p.starts_with(&win));
    let in_program_files = path_lc.as_deref().is_some_and(|p| {
        ["programfiles", "programfiles(x86)"]
            .iter()
            .filter_map(|v| std::env::var(v).ok())
            .any(|d| p.starts_with(&d.to_lowercase()))
    });

    let mut signed = None;
    let mut hash = None;

    if let (Some(p), Some(plc)) = (&path, &path_lc) {
        let stem = p.file_stem().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();

        if !in_windows {
            if SYSTEM_NAMES.contains(&stem.as_str()) {
                add(6, tr(
                    format!("Tên file \"{stem}.exe\" trùng tiến trình hệ thống nhưng nằm ngoài thư mục Windows — dấu hiệu giả mạo"),
                    format!("File name \"{stem}.exe\" matches a system process but it is outside the Windows folder — a sign of impersonation"),
                ));
            } else if SYSTEM_NAMES.iter().any(|n| edit_distance_le1(&stem, n)) {
                add(5, tr(
                    format!("Tên file \"{stem}\" gần giống tên tiến trình hệ thống — có thể là cố tình đặt nhầm lẫn"),
                    format!("File name \"{stem}\" is almost identical to a system process name — possibly meant to be confused with it"),
                ));
            }
            for (frag, pts, vi, en) in USER_DIRS {
                if plc.contains(frag) {
                    add(*pts, tr(
                        format!("Chạy từ {vi} — dịch vụ hợp lệ hiếm khi nằm ở đây"),
                        format!("Runs from {en} — legitimate software rarely lives there"),
                    ));
                    break;
                }
            }
            if plc.contains("\\programdata\\") && !plc.contains("\\microsoft\\") {
                add(2, tr("Chạy từ thư mục ProgramData", "Runs from the ProgramData folder"));
            }
            if p.parent().and_then(|x| x.parent()).is_none() {
                add(3, tr("Nằm ngay ở thư mục gốc của ổ đĩa", "Sits directly in the root of a drive"));
            }
        }

        if !p.exists() {
            add(2, tr(
                "File chương trình không còn tồn tại trên đĩa (dịch vụ mồ côi)",
                "The program file no longer exists on disk (orphaned entry)",
            ));
        } else if !in_windows && !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk")) {
            // shortcuts (.lnk) carry no signature of their own, so they are not checked
            match signature(p) {
                Sig::Signed => signed = Some(true),
                Sig::Unsigned => {
                    signed = Some(false);
                    add(2, tr(
                        "File chưa được ký số — không xác minh được nhà phát hành",
                        "File is not digitally signed — the publisher cannot be verified",
                    ));
                }
                Sig::Invalid => {
                    signed = Some(false);
                    add(5, tr(
                        "Chữ ký số KHÔNG hợp lệ — file có thể đã bị chỉnh sửa sau khi ký",
                        "The digital signature is NOT valid — the file may have been modified after signing",
                    ));
                }
                Sig::Unknown => {}
            }
        }
        if looks_random(&stem) {
            add(2, tr("Tên file trông như chuỗi ngẫu nhiên", "The file name looks like a random string"));
        }
    }

    let lc = binary.to_lowercase();
    if let Some(k) = LOLBINS.iter().find(|k| lc.contains(**k)) {
        let k = k.trim();
        add(5, tr(
            format!("Dòng lệnh dùng công cụ/hành vi thường bị lạm dụng (\"{k}\")"),
            format!("The command line uses a tool or behaviour that is often abused (\"{k}\")"),
        ));
    }
    if looks_random(&s.name) {
        add(2, tr("Tên trông như chuỗi ngẫu nhiên", "The name looks like a random string"));
    }
    if unquoted {
        add(1, tr(
            "Đường dẫn chứa dấu cách nhưng không có dấu ngoặc kép (lỗ hổng leo thang đặc quyền)",
            "The path contains spaces but is not quoted (a privilege-escalation weakness)",
        ));
    }
    if s.kind == "service" && !in_windows && s.description.is_none() {
        add(1, tr("Không có mô tả", "No description"));
    }
    if s.kind == "service"
        && !in_windows
        && !in_program_files
        && s.account.as_deref().is_some_and(|a| a.eq_ignore_ascii_case("localsystem"))
        && path.is_some()
    {
        add(1, tr(
            "Chạy với quyền SYSTEM từ vị trí ngoài Program Files/Windows",
            "Runs as SYSTEM from a location outside Program Files/Windows",
        ));
    }

    if score >= 2 {
        hash = path.as_deref().and_then(sha256);
    }

    Finding {
        kind: s.kind,
        pids: s.pids.clone(),
        source: s.source.clone(),
        key: s.key.clone(),
        name: s.name.clone(),
        display: s.display.clone(),
        status: s.status,
        path: path.map(|p| p.to_string_lossy().into_owned()),
        account: s.account.clone(),
        score,
        verdict: if score >= 5 { "suspicious" } else if score >= 2 { "notice" } else { "ok" },
        reasons,
        signed,
        sha256: hash,
    }
}

/// Scan one category: "service", "process" or "startup".
pub fn scan(kind: &str) -> Result<Vec<Finding>, String> {
    let subjects: Vec<Subject> = match kind {
        "service" => services::list()?.iter().map(Subject::from).collect(),
        "process" => running_subjects()?,
        "startup" => startup::list()?.iter().map(Subject::from).collect(),
        _ => return Err(tr(format!("Loại quét không hợp lệ: {kind}"), format!("Invalid scan type: {kind}"))),
    };
    let mut v: Vec<Finding> = subjects.iter().map(analyze).collect();
    v.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.display.cmp(&b.display)));
    Ok(v)
}

#[cfg(windows)]
fn signature(path: &Path) -> Sig {
    use std::{ffi::c_void, mem::size_of, os::windows::ffi::OsStrExt};

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Security::WinTrust::*;

    unsafe {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut file = WINTRUST_FILE_INFO {
            cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
            pcwszFilePath: PCWSTR(wide.as_ptr()),
            ..Default::default()
        };
        let mut data = WINTRUST_DATA {
            cbStruct: size_of::<WINTRUST_DATA>() as u32,
            dwUIChoice: WTD_UI_NONE,
            fdwRevocationChecks: WTD_REVOKE_NONE,
            dwUnionChoice: WTD_CHOICE_FILE,
            dwStateAction: WTD_STATEACTION_VERIFY,
            // do not use the network while verifying
            dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
            Anonymous: WINTRUST_DATA_0 { pFile: &mut file },
            ..Default::default()
        };
        let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
        let r = WinVerifyTrust(HWND(std::ptr::null_mut()), &mut action, &mut data as *mut _ as *mut c_void) as u32;
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        WinVerifyTrust(HWND(std::ptr::null_mut()), &mut action, &mut data as *mut _ as *mut c_void);

        match r {
            0 => Sig::Signed,
            0x800B0100 | 0x800B0003 | 0x800B0001 => Sig::Unsigned, // NOSIGNATURE, SUBJECT_FORM_UNKNOWN, PROVIDER_UNKNOWN
            0x80096010 | 0x80096011 => Sig::Invalid,               // BAD_DIGEST and related signature-tampering errors
            _ => Sig::Unknown, // expired, untrusted root… not enough evidence to conclude
        }
    }
}

#[cfg(not(windows))]
fn signature(_path: &Path) -> Sig {
    Sig::Unknown
}
