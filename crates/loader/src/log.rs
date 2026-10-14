use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

static LOCK: Mutex<()> = Mutex::new(());
static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();

const NAME: &str = "playkit-loader.log";

fn header() -> String {
    let exe = crate::loose::exe_path()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    format!(
        "playkit loader {}\nprocess: {exe}\n",
        env!("CARGO_PKG_VERSION")
    )
}

/// Next to the game executable, or in the temp folder when the game folder is not writable.
fn path() -> Option<&'static PathBuf> {
    PATH.get_or_init(|| {
        let head = header();
        let candidates = [
            crate::loose::exe_dir().map(|d| d.join(NAME)),
            Some(std::env::temp_dir().join(NAME)),
        ];
        candidates
            .into_iter()
            .flatten()
            .find(|p| std::fs::write(p, &head).is_ok())
    })
    .as_ref()
}

pub fn reset() {
    let _ = path();
}

pub fn write(msg: &str) {
    let _guard = LOCK.lock();
    let Some(p) = path() else { return };
    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(p) {
        let _ = writeln!(f, "{msg}");
    }
}
