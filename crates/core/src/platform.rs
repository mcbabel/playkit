use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Os {
    Windows,
    Linux,
    Other,
}

#[derive(Debug, Clone, Serialize)]
pub struct Platform {
    pub os: Os,
    pub name: String,
    pub steam_deck: bool,
    pub uses_proton: bool,
}

pub fn detect() -> Platform {
    if cfg!(windows) {
        return Platform {
            os: Os::Windows,
            name: "Windows".into(),
            steam_deck: false,
            uses_proton: false,
        };
    }
    if cfg!(target_os = "linux") {
        let release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
        let field = |k: &str| {
            release
                .lines()
                .find_map(|l| l.strip_prefix(&format!("{k}=")))
                .map(|v| v.trim_matches('"').to_string())
        };
        let name = field("PRETTY_NAME")
            .or_else(|| field("NAME"))
            .unwrap_or_else(|| "Linux".into());
        let steamos = field("ID").is_some_and(|id| id == "steamos");
        let board =
            std::fs::read_to_string("/sys/devices/virtual/dmi/id/board_name").unwrap_or_default();
        let deck = steamos || matches!(board.trim(), "Jupiter" | "Galileo");
        return Platform {
            os: Os::Linux,
            name,
            steam_deck: deck,
            uses_proton: true,
        };
    }
    Platform {
        os: Os::Other,
        name: std::env::consts::OS.into(),
        steam_deck: false,
        uses_proton: false,
    }
}

pub fn process_running(names: &[&str]) -> bool {
    process_running_in(names, None)
}

pub fn process_running_in(names: &[&str], dir: Option<&std::path::Path>) -> bool {
    use sysinfo::{ProcessRefreshKind, RefreshKind, System, UpdateKind};
    let kind = ProcessRefreshKind::nothing()
        .with_exe(UpdateKind::Always)
        .with_cmd(UpdateKind::Always);
    let sys = System::new_with_specifics(RefreshKind::nothing().with_processes(kind));
    let needle = dir.map(normalize);
    sys.processes().values().any(|p| {
        let n = p.name().to_string_lossy().to_ascii_lowercase();
        let n = n.strip_suffix(".exe").unwrap_or(&n);
        let named = names.iter().any(|w| {
            let w = w.to_ascii_lowercase();
            n == w || (n.len() == 15 && w.starts_with(n))
        });
        if !named {
            return false;
        }
        let Some(needle) = &needle else { return true };
        let mut seen: Vec<String> = p
            .cmd()
            .iter()
            .map(|c| normalize(std::path::Path::new(c)))
            .collect();
        if let Some(exe) = p.exe() {
            seen.push(normalize(exe));
        }
        seen.iter().all(|s| s.is_empty()) || seen.iter().any(|s| s.contains(needle.as_str()))
    })
}

fn normalize(p: &std::path::Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/").to_lowercase();
    let s = s.trim_end_matches('/');
    match s.split_once(":/") {
        Some((drive, rest)) if drive.len() == 1 => rest.to_string(),
        _ => s.trim_start_matches('/').to_string(),
    }
}
