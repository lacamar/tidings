#[cxx_qt::bridge]
mod ffi {
    unsafe extern "C++" {
        include!("shim.h");
        fn run(visible: bool, url: &str) -> i32;
        fn app_quit();
        fn window_active() -> bool;
        fn clipboard_text() -> String;
        fn set_clipboard(text: &str);
        fn ui_font() -> String;
        fn set_color_scheme(dark: bool);
        fn fmt_ts(ts: i64, kind: u8) -> String;
        fn days_ago(ts: i64) -> i64;
        fn save_icon(data: &[u8], path: &str) -> bool;
    }

    extern "Rust" {
        fn blocks(host: &str, path: &str) -> bool;
        fn clean_query(query: &str, out: &mut String) -> bool;
        fn theme_path() -> String;
    }
}

pub use ffi::*;

pub enum Fmt {
    Time = 0,
    ShortDate = 1,
    LongDate = 2,
    DayName = 3,
}

pub fn fmt(ts: i64, f: Fmt) -> String {
    fmt_ts(ts, f as u8)
}

fn blocks(host: &str, path: &str) -> bool {
    crate::blocker::blocks(host, path)
}

fn clean_query(query: &str, out: &mut String) -> bool {
    crate::blocker::clean_query(query)
        .map(|q| *out = q)
        .is_some()
}

fn theme_path() -> String {
    crate::backend::theme_path().to_string_lossy().into_owned()
}
