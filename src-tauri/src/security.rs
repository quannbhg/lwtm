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

use crate::services::{self, ServiceInfo};

#[derive(Serialize)]
pub struct Finding {
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

const USER_DIRS: &[(&str, u32, &str)] = &[
    ("\\temp\\", 4, "thư mục tạm"),
    ("\\appdata\\", 4, "thư mục AppData của người dùng"),
    ("\\downloads\\", 4, "thư mục Downloads"),
    ("\\users\\public\\", 4, "thư mục Public"),
    ("$recycle.bin", 5, "Thùng rác"),
    // Linux / macOS
    ("/tmp/", 4, "thư mục tạm"),
    ("/var/tmp/", 4, "thư mục tạm"),
    ("/dev/shm/", 5, "bộ nhớ chia sẻ /dev/shm"),
    ("/downloads/", 4, "thư mục Downloads"),
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

fn analyze(s: &ServiceInfo) -> Finding {
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
                add(6, format!("Tên file \"{stem}.exe\" trùng tiến trình hệ thống nhưng nằm ngoài thư mục Windows — dấu hiệu giả mạo"));
            } else if SYSTEM_NAMES.iter().any(|n| edit_distance_le1(&stem, n)) {
                add(5, format!("Tên file \"{stem}\" gần giống tên tiến trình hệ thống — có thể là cố tình đặt nhầm lẫn"));
            }
            for (frag, pts, label) in USER_DIRS {
                if plc.contains(frag) {
                    add(*pts, format!("Chạy từ {label} — dịch vụ hợp lệ hiếm khi nằm ở đây"));
                    break;
                }
            }
            if plc.contains("\\programdata\\") && !plc.contains("\\microsoft\\") {
                add(2, "Chạy từ thư mục ProgramData".into());
            }
            if p.parent().and_then(|x| x.parent()).is_none() {
                add(3, "Nằm ngay ở thư mục gốc của ổ đĩa".into());
            }
        }

        if !p.exists() {
            add(2, "File chương trình không còn tồn tại trên đĩa (dịch vụ mồ côi)".into());
        } else if !in_windows {
            match signature(p) {
                Sig::Signed => signed = Some(true),
                Sig::Unsigned => {
                    signed = Some(false);
                    add(2, "File chưa được ký số — không xác minh được nhà phát hành".into());
                }
                Sig::Invalid => {
                    signed = Some(false);
                    add(5, "Chữ ký số KHÔNG hợp lệ — file có thể đã bị chỉnh sửa sau khi ký".into());
                }
                Sig::Unknown => {}
            }
        }
        if looks_random(&stem) {
            add(2, "Tên file trông như chuỗi ngẫu nhiên".into());
        }
    }

    let lc = binary.to_lowercase();
    if let Some(k) = LOLBINS.iter().find(|k| lc.contains(**k)) {
        add(5, format!("Dòng lệnh dùng công cụ/hành vi thường bị lạm dụng (\"{}\")", k.trim()));
    }
    if looks_random(&s.name) {
        add(2, "Tên dịch vụ trông như chuỗi ngẫu nhiên".into());
    }
    if unquoted {
        add(1, "Đường dẫn chứa dấu cách nhưng không có dấu ngoặc kép (lỗ hổng leo thang đặc quyền)".into());
    }
    if !in_windows && s.description.is_none() {
        add(1, "Không có mô tả".into());
    }
    if !in_windows
        && !in_program_files
        && s.account.as_deref().is_some_and(|a| a.eq_ignore_ascii_case("localsystem"))
        && path.is_some()
    {
        add(1, "Chạy với quyền SYSTEM từ vị trí ngoài Program Files/Windows".into());
    }

    if score >= 2 {
        hash = path.as_deref().and_then(sha256);
    }

    Finding {
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

pub fn scan() -> Result<Vec<Finding>, String> {
    let mut v: Vec<Finding> = services::list()?.iter().map(analyze).collect();
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
