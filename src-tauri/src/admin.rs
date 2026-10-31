//! Check for administrator rights and relaunch the app with elevated privileges.

#[cfg(windows)]
pub fn is_elevated() -> bool {
    use std::{ffi::c_void, mem::size_of};

    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elev = TOKEN_ELEVATION::default();
        let mut ret = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elev as *mut _ as *mut c_void),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elev.TokenIsElevated != 0
    }
}

#[cfg(unix)]
pub fn is_elevated() -> bool {
    unsafe { libc::geteuid() == 0 }
}

/// Start a new copy of the app as Administrator (shows the UAC prompt). Returns Ok once the new copy
/// has been launched, at which point the current one should exit.
#[cfg(windows)]
pub fn relaunch_elevated() -> Result<(), String> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    // In dev mode the window loads its UI from the dev server, and quitting the current copy stops that server too
    if cfg!(debug_assertions) {
        return Err("Ở chế độ phát triển, hãy mở terminal bằng \"Run as administrator\" rồi chạy lại `npm run tauri dev`".into());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe: Vec<u16> = exe.to_string_lossy().encode_utf16().chain(Some(0)).collect();
    unsafe {
        let r = ShellExecuteW(None, w!("runas"), PCWSTR(exe.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
        // Per the docs: a value > 32 means success; if the user clicks "No" in UAC a small error code is returned
        if r.0 as usize > 32 {
            Ok(())
        } else {
            Err("Bạn đã từ chối yêu cầu quyền Administrator".into())
        }
    }
}

#[cfg(not(windows))]
pub fn relaunch_elevated() -> Result<(), String> {
    Err("Hãy chạy lại ứng dụng bằng sudo hoặc pkexec để có quyền quản trị".into())
}
