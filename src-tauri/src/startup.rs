//! Apps that start with the system (Windows: registry Run keys + Startup folders, Linux: ~/.config/autostart)
//! and autostart for this app itself.

use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct StartupEntry {
    /// Source (used to enable/disable): hkcu_run, hklm_run, folder_user…
    pub source: &'static str,
    /// Source name shown to the user
    pub source_label: String,
    /// Identifier within the source (registry value name or file name)
    pub key: String,
    pub name: String,
    pub command: String,
    /// Executable (or shortcut) used to look up the icon and name
    pub exe: Option<String>,
    pub enabled: bool,
}

#[derive(Deserialize)]
pub struct Target {
    pub source: String,
    pub key: String,
}

pub use imp::{autostart_enabled, list, set_autostart, set_enabled};

const APP_ENTRY: &str = "LightweightTaskManager";

#[cfg(windows)]
mod imp {
    use std::{borrow::Cow, fs, path::PathBuf};

    use winreg::{enums::*, RegKey, RegValue, HKEY};

    use super::{StartupEntry, Target, APP_ENTRY};
    use crate::security::{exe_path, expand_env};

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const RUN32: &str = r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run";
    const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";

    struct Source {
        id: &'static str,
        /// (Vietnamese, English)
        label: (&'static str, &'static str),
        hive: HKEY,
        /// Registry key holding the list (None for a folder source)
        run_key: Option<&'static str>,
        folder: Option<fn() -> Option<PathBuf>>,
        /// Subkey of StartupApproved that stores the enabled/disabled state
        approved: &'static str,
    }

    fn user_folder() -> Option<PathBuf> {
        std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join(r"Microsoft\Windows\Start Menu\Programs\Startup"))
    }
    fn common_folder() -> Option<PathBuf> {
        std::env::var_os("ProgramData").map(|p| PathBuf::from(p).join(r"Microsoft\Windows\Start Menu\Programs\StartUp"))
    }

    const SOURCES: [Source; 5] = [
        Source { id: "hkcu_run", label: ("Registry (người dùng)", "Registry (current user)"), hive: HKEY_CURRENT_USER, run_key: Some(RUN), folder: None, approved: "Run" },
        Source { id: "hklm_run", label: ("Registry (mọi người dùng)", "Registry (all users)"), hive: HKEY_LOCAL_MACHINE, run_key: Some(RUN), folder: None, approved: "Run" },
        Source { id: "hklm_run32", label: ("Registry (32-bit)", "Registry (32-bit)"), hive: HKEY_LOCAL_MACHINE, run_key: Some(RUN32), folder: None, approved: "Run32" },
        Source { id: "folder_user", label: ("Thư mục Startup (người dùng)", "Startup folder (current user)"), hive: HKEY_CURRENT_USER, run_key: None, folder: Some(user_folder), approved: "StartupFolder" },
        Source { id: "folder_common", label: ("Thư mục Startup (mọi người dùng)", "Startup folder (all users)"), hive: HKEY_LOCAL_MACHINE, run_key: None, folder: Some(common_folder), approved: "StartupFolder" },
    ];

    fn is_enabled(src: &Source, key: &str) -> bool {
        RegKey::predef(src.hive)
            .open_subkey(format!(r"{APPROVED}\{}", src.approved))
            .and_then(|k| k.get_raw_value(key))
            // Windows stores 02/06 = enabled, 03/07 = disabled; no value = enabled
            .map(|v| v.bytes.first().map_or(true, |b| b % 2 == 0))
            .unwrap_or(true)
    }

    pub fn list() -> Result<Vec<StartupEntry>, String> {
        let mut out = Vec::new();
        for src in &SOURCES {
            if let Some(path) = src.run_key {
                let Ok(k) = RegKey::predef(src.hive).open_subkey(path) else { continue };
                for (name, _) in k.enum_values().flatten() {
                    let Ok(cmd) = k.get_value::<String, _>(&name) else { continue };
                    let command = expand_env(&cmd);
                    let exe = exe_path(&command).0.map(|p| p.to_string_lossy().into_owned());
                    out.push(StartupEntry {
                        source: src.id,
                        source_label: crate::i18n::tr(src.label.0, src.label.1),
                        enabled: is_enabled(src, &name),
                        key: name.clone(),
                        name,
                        command,
                        exe,
                    });
                }
            } else if let Some(dir) = src.folder.and_then(|f| f()) {
                let Ok(rd) = fs::read_dir(dir) else { continue };
                for e in rd.flatten() {
                    let file = e.file_name().to_string_lossy().into_owned();
                    if file.eq_ignore_ascii_case("desktop.ini") {
                        continue;
                    }
                    let path = e.path().to_string_lossy().into_owned();
                    out.push(StartupEntry {
                        source: src.id,
                        source_label: crate::i18n::tr(src.label.0, src.label.1),
                        enabled: is_enabled(src, &file),
                        name: file.rsplit_once('.').map(|(n, _)| n.to_string()).unwrap_or_else(|| file.clone()),
                        key: file,
                        command: path.clone(),
                        exe: Some(path),
                    });
                }
            }
        }
        Ok(out)
    }

    pub fn set_enabled(t: &Target, enabled: bool) -> Result<(), String> {
        let src = SOURCES
            .iter()
            .find(|s| s.id == t.source)
            .ok_or_else(|| crate::i18n::tr("Nguồn không hợp lệ", "Invalid source"))?;
        let (key, _) = RegKey::predef(src.hive)
            .create_subkey(format!(r"{APPROVED}\{}", src.approved))
            .map_err(denied)?;
        let mut bytes = vec![0u8; 12];
        bytes[0] = if enabled { 0x02 } else { 0x03 };
        key.set_raw_value(&t.key, &RegValue { bytes: Cow::Owned(bytes), vtype: REG_BINARY }).map_err(denied)
    }

    fn denied(e: std::io::Error) -> String {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            crate::i18n::tr(
                "Cần quyền Administrator để thay đổi mục này",
                "Administrator rights are required to change this entry",
            )
        } else {
            e.to_string()
        }
    }

    pub fn autostart_enabled() -> bool {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(RUN)
            .and_then(|k| k.get_value::<String, _>(APP_ENTRY))
            .is_ok()
    }

    pub fn set_autostart(on: bool) -> Result<(), String> {
        let (k, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN).map_err(denied)?;
        if on {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            k.set_value(APP_ENTRY, &format!("\"{}\" --minimized", exe.display())).map_err(denied)
        } else {
            match k.delete_value(APP_ENTRY) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(denied(e)),
                _ => Ok(()),
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::{fs, path::PathBuf};

    use super::{StartupEntry, Target, APP_ENTRY};

    fn user_dir() -> Option<PathBuf> {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|p| p.join("autostart"))
    }

    fn field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
        text.lines()
            .map(str::trim)
            .find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix('=')))
            .map(str::trim)
    }

    fn parse(source: &'static str, label: &'static str, file: String, text: &str) -> StartupEntry {
        let exec = field(text, "Exec").unwrap_or("").to_string();
        let hidden = field(text, "Hidden").is_some_and(|v| v.eq_ignore_ascii_case("true"));
        let gnome_off = field(text, "X-GNOME-Autostart-enabled").is_some_and(|v| v.eq_ignore_ascii_case("false"));
        StartupEntry {
            source,
            source_label: label.to_string(),
            name: field(text, "Name").map(String::from).unwrap_or_else(|| file.clone()),
            exe: exec.split_whitespace().next().map(String::from),
            command: exec,
            enabled: !hidden && !gnome_off,
            key: file,
        }
    }

    pub fn list() -> Result<Vec<StartupEntry>, String> {
        let mut out = Vec::new();
        let dirs = [
            ("folder_user", "~/.config/autostart", user_dir()),
            ("folder_common", "/etc/xdg/autostart", Some(PathBuf::from("/etc/xdg/autostart"))),
        ];
        for (id, label, dir) in dirs {
            let Some(Ok(rd)) = dir.map(fs::read_dir) else { continue };
            for e in rd.flatten() {
                let file = e.file_name().to_string_lossy().into_owned();
                if !file.ends_with(".desktop") {
                    continue;
                }
                if let Ok(text) = fs::read_to_string(e.path()) {
                    // a user entry overrides a system entry with the same name
                    if id == "folder_common" && out.iter().any(|x: &StartupEntry| x.key == file) {
                        continue;
                    }
                    out.push(parse(id, label, file, &text));
                }
            }
        }
        Ok(out)
    }

    pub fn set_enabled(t: &Target, enabled: bool) -> Result<(), String> {
        let dir = user_dir().ok_or_else(no_config_dir)?;
        let src = if t.source == "folder_common" { PathBuf::from("/etc/xdg/autostart").join(&t.key) } else { dir.join(&t.key) };
        let text = fs::read_to_string(&src).map_err(|e| e.to_string())?;
        let mut lines: Vec<String> = text
            .lines()
            .filter(|l| !l.trim_start().starts_with("Hidden=") && !l.trim_start().starts_with("X-GNOME-Autostart-enabled="))
            .map(String::from)
            .collect();
        if !enabled {
            let at = lines.iter().position(|l| l.trim() == "[Desktop Entry]").map_or(0, |i| i + 1);
            lines.insert(at, "Hidden=true".into());
        }
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        fs::write(dir.join(&t.key), lines.join("\n") + "\n").map_err(|e| e.to_string())
    }

    fn no_config_dir() -> String {
        crate::i18n::tr("Không xác định được thư mục cấu hình", "Could not determine the config folder")
    }

    fn own_file() -> Option<PathBuf> {
        user_dir().map(|d| d.join(format!("{APP_ENTRY}.desktop")))
    }

    pub fn autostart_enabled() -> bool {
        own_file().is_some_and(|p| p.exists())
    }

    pub fn set_autostart(on: bool) -> Result<(), String> {
        let path = own_file().ok_or_else(no_config_dir)?;
        if on {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
            let body = format!(
                "[Desktop Entry]\nType=Application\nName=Lightweight Task Manager\nExec=\"{}\" --minimized\nTerminal=false\n",
                exe.display()
            );
            fs::write(path, body).map_err(|e| e.to_string())
        } else {
            match fs::remove_file(path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
                _ => Ok(()),
            }
        }
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    use super::{StartupEntry, Target};

    fn msg() -> String {
        crate::i18n::tr(
            "Quản lý ứng dụng khởi động chưa được hỗ trợ trên hệ điều hành này",
            "Startup app management is not supported on this operating system",
        )
    }

    pub fn list() -> Result<Vec<StartupEntry>, String> {
        Err(msg())
    }
    pub fn set_enabled(_t: &Target, _enabled: bool) -> Result<(), String> {
        Err(msg())
    }
    pub fn autostart_enabled() -> bool {
        false
    }
    pub fn set_autostart(_on: bool) -> Result<(), String> {
        Err(msg())
    }
}
