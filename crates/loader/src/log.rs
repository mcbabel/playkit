use std::io::Write;
use std::sync::Mutex;

static LOCK: Mutex<()> = Mutex::new(());

pub fn write(msg: &str) {
    let _guard = LOCK.lock();
    let Some(dir) = crate::loose::exe_dir() else {
        return;
    };
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("playkit-loader.log"))
    {
        let _ = writeln!(f, "{msg}");
    }
}

pub fn reset() {
    if let Some(dir) = crate::loose::exe_dir() {
        let _ = std::fs::write(
            dir.join("playkit-loader.log"),
            concat!("playkit loader ", env!("CARGO_PKG_VERSION"), "\n"),
        );
    }
}
