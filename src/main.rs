mod backend;
mod blocker;
mod bridge;
mod bus;
mod db;
mod models;
mod shim;
mod text;
mod web;

use std::io::Write;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use signal_hook::consts::{SIGINT, SIGTERM};

pub const APP_ID: &str = "dev.lacamar.Tidings";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = args
        .iter()
        .find(|a| !a.starts_with('-'))
        .map_or("", String::as_str);
    let url = arg
        .strip_prefix("feed://")
        .or_else(|| arg.strip_prefix("feed:"))
        .unwrap_or(arg);
    let sock = std::env::var_os("XDG_RUNTIME_DIR")
        .map_or_else(|| PathBuf::from("/tmp"), PathBuf::from)
        .join(APP_ID);
    if let Ok(mut peer) = UnixStream::connect(&sock) {
        let _ = peer.write_all(format!("{url}\n").as_bytes());
        return;
    }
    let _ = std::fs::remove_file(&sock);
    match UnixListener::bind(&sock) {
        Ok(l) => *backend::LISTENER.lock().unwrap() = Some(l),
        Err(e) => eprintln!("{}: {e}", sock.display()),
    }

    match bus::Bus::new().and_then(|b| b.read_setting("color-scheme")) {
        // Chromium ignores Qt's color scheme; only read at startup
        Ok(bus::Setting::Dark(true)) => {
            let flags = std::env::var("QTWEBENGINE_CHROMIUM_FLAGS").unwrap_or_default();
            // SAFETY: no other threads yet
            unsafe {
                std::env::set_var(
                    "QTWEBENGINE_CHROMIUM_FLAGS",
                    flags + " --blink-settings=preferredColorScheme=0",
                )
            };
        }
        Ok(_) => {}
        Err(e) => eprintln!("portal color-scheme: {e}"),
    }

    if let Ok(mut signals) = signal_hook::iterator::Signals::new([SIGINT, SIGTERM]) {
        std::thread::spawn(move || {
            for _ in signals.forever() {
                shim::app_quit();
            }
        });
    }
    let code = shim::run(!args.iter().any(|a| a == "--background"), url);
    let _ = std::fs::remove_file(&sock);
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_matches_spec() {
        let spec = include_str!("../packaging/tidings.spec");
        let v = spec
            .lines()
            .find_map(|l| l.strip_prefix("Version:"))
            .unwrap()
            .trim();
        assert_eq!(v, env!("CARGO_PKG_VERSION"));
    }
}
