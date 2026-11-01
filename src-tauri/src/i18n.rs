//! Language of the messages produced on the Rust side (errors, security findings, tray menu).
//! The frontend sets it through the `set_lang` command; the UI text itself is translated in the web page.

use std::sync::atomic::{AtomicBool, Ordering};

static ENGLISH: AtomicBool = AtomicBool::new(false);

pub fn set(lang: &str) {
    ENGLISH.store(lang == "en", Ordering::Relaxed);
}

pub fn is_en() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

/// Pick the Vietnamese or English wording. Both are built eagerly, which is fine for rare error paths.
pub fn tr(vi: impl Into<String>, en: impl Into<String>) -> String {
    if is_en() {
        en.into()
    } else {
        vi.into()
    }
}
