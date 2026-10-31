//! Display name + icon of an application, read from its .exe file. Results are cached by path.

use std::{collections::HashMap, sync::Mutex};

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct AppMeta {
    /// File description (e.g. "Google Chrome"), None if unavailable
    pub description: Option<String>,
    /// PNG data URL, None if it could not be read
    pub icon: Option<String>,
}

#[derive(Default)]
pub struct MetaCache(Mutex<HashMap<String, AppMeta>>);

impl MetaCache {
    pub fn get(&self, exe: &str) -> AppMeta {
        if let Some(m) = self.0.lock().unwrap().get(exe) {
            return m.clone();
        }
        // Extract outside the lock because it can be slow
        let meta = AppMeta {
            description: platform::description(exe),
            icon: platform::icon_png(exe).map(|png| format!("data:image/png;base64,{}", STANDARD.encode(png))),
        };
        self.0.lock().unwrap().insert(exe.to_string(), meta.clone());
        meta
    }
}

#[cfg(windows)]
mod platform {
    use std::ffi::c_void;
    use std::mem::size_of;

    use windows::core::{w, PCWSTR};
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::Storage::FileSystem::*;
    use windows::Win32::UI::Shell::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    pub fn description(path: &str) -> Option<String> {
        unsafe {
            let p = wide(path);
            let size = GetFileVersionInfoSizeW(PCWSTR(p.as_ptr()), None);
            if size == 0 {
                return None;
            }
            let mut data = vec![0u8; size as usize];
            GetFileVersionInfoW(PCWSTR(p.as_ptr()), None, size, data.as_mut_ptr() as *mut c_void).ok()?;

            let mut ptr: *mut c_void = std::ptr::null_mut();
            let mut len = 0u32;
            if !VerQueryValueW(data.as_ptr() as *const c_void, w!("\\VarFileInfo\\Translation"), &mut ptr, &mut len).as_bool()
                || len < 4
            {
                return None;
            }
            let t = ptr as *const u16;
            let (lang, cp) = (*t, *t.add(1));
            let sub = wide(&format!("\\StringFileInfo\\{lang:04x}{cp:04x}\\FileDescription"));
            if !VerQueryValueW(data.as_ptr() as *const c_void, PCWSTR(sub.as_ptr()), &mut ptr, &mut len).as_bool()
                || len < 2
            {
                return None;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(ptr as *const u16, (len - 1) as usize));
            let s = s.trim().to_string();
            (!s.is_empty()).then_some(s)
        }
    }

    pub fn icon_png(path: &str) -> Option<Vec<u8>> {
        unsafe {
            let p = wide(path);
            let mut info = SHFILEINFOW::default();
            let r = SHGetFileInfoW(
                PCWSTR(p.as_ptr()),
                FILE_FLAGS_AND_ATTRIBUTES(0),
                Some(&mut info),
                size_of::<SHFILEINFOW>() as u32,
                SHGFI_ICON | SHGFI_LARGEICON,
            );
            if r == 0 || info.hIcon.is_invalid() {
                return None;
            }
            let hicon = info.hIcon;
            let mut ii = ICONINFO::default();
            let ok = GetIconInfo(hicon, &mut ii).is_ok();
            let result = if ok { read_color_bitmap(&ii) } else { None };
            if ok {
                let _ = DeleteObject(ii.hbmColor.into());
                let _ = DeleteObject(ii.hbmMask.into());
            }
            let _ = DestroyIcon(hicon);
            let (w, h, mut px) = result?;

            // BGRA -> RGBA; old icons without an alpha channel are treated as fully opaque
            let opaque = px.chunks_exact(4).all(|c| c[3] == 0);
            for c in px.chunks_exact_mut(4) {
                c.swap(0, 2);
                if opaque {
                    c[3] = 255;
                }
            }
            let mut out = Vec::new();
            let mut enc = png::Encoder::new(&mut out, w, h);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().ok()?.write_image_data(&px).ok()?;
            Some(out)
        }
    }

    unsafe fn read_color_bitmap(ii: &ICONINFO) -> Option<(u32, u32, Vec<u8>)> {
        let mut bm = BITMAP::default();
        if GetObjectW(ii.hbmColor.into(), size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut c_void)) == 0 {
            return None;
        }
        let (w, h) = (bm.bmWidth, bm.bmHeight);
        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let hdc = GetDC(None);
        let lines = GetDIBits(hdc, ii.hbmColor, 0, h as u32, Some(buf.as_mut_ptr() as *mut c_void), &mut bmi, DIB_RGB_COLORS);
        ReleaseDC(None, hdc);
        (lines != 0).then_some((w as u32, h as u32, buf))
    }
}

#[cfg(not(windows))]
mod platform {
    // TODO: macOS (NSWorkspace) and Linux (.desktop + icon theme)
    pub fn description(_path: &str) -> Option<String> {
        None
    }
    pub fn icon_png(_path: &str) -> Option<Vec<u8>> {
        None
    }
}
