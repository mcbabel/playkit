//! Checks why mods might not load on a player's PC, as a list and as a text report to share.

use crate::anticheat::{self, AntiCheat};
use crate::fsx;
use crate::game::{Game, EXE, PROJECT};
use crate::index::Index;
use crate::install::State as InstallState;
use crate::loader::{self, Bundled, DLL};
use crate::platform::{Os, Platform};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Ok,
    Info,
    Warn,
    Bad,
}

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub level: Level,
    pub title: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub checks: Vec<Check>,
    pub text: String,
}

/// DLL names other mod tools and overlays use as proxies; any of them can stop or replace our loader.
const PROXIES: &[&str] = &[
    "dinput8.dll",
    "d3d9.dll",
    "d3d11.dll",
    "d3d12.dll",
    "dxgi.dll",
    "version.dll",
    "winmm.dll",
    "winhttp.dll",
    "dsound.dll",
    "xinput1_3.dll",
    "xinput1_4.dll",
    "xinput9_1_0.dll",
    "opengl32.dll",
    "UE4SS.dll",
    "ue4ss.dll",
];
const OTHER_TOOLS: &[&str] = &[
    "ReShade.ini",
    "dxgi.ini",
    "UE4SS-settings.ini",
    "ue4ss",
    "Mods",
];

fn check(level: Level, title: impl Into<String>, detail: impl Into<String>) -> Check {
    Check {
        level,
        title: title.into(),
        detail: detail.into(),
    }
}

pub fn run(
    game: Option<&Game>,
    platform: &Platform,
    bundled: &Bundled,
    index: Option<&Index>,
    app_version: &str,
) -> Report {
    let mut c = Vec::new();
    c.push(check(
        Level::Info,
        "System",
        format!(
            "{} · Playkit {app_version}",
            if platform.steam_deck {
                "Steam Deck"
            } else {
                &platform.name
            }
        ),
    ));
    let Some(game) = game else {
        c.push(check(
            Level::Bad,
            "Game folder",
            "Project Playtime was not found. Pick the game folder in Setup.",
        ));
        return finish(c);
    };
    game_folder(&mut c, game);
    build(&mut c, game, index);
    loader_dll(&mut c, game, bundled);
    conflicts(&mut c, game);
    last_run(&mut c, game);
    anti_cheat(&mut c, game);
    mods(&mut c, game);
    if platform.os == Os::Linux {
        let st = loader::status(game, platform, bundled);
        let (level, detail) = match st.launch_option {
            loader::LaunchOption::Ok | loader::LaunchOption::NotNeeded => {
                (Level::Ok, "The dxgi override is set.".to_string())
            }
            loader::LaunchOption::Missing { current, .. } => (
                Level::Bad,
                format!("Missing WINEDLLOVERRIDES for dxgi. Current options: {current:?}"),
            ),
            loader::LaunchOption::Unknown => (
                Level::Warn,
                "Could not read the Steam launch options.".to_string(),
            ),
        };
        c.push(check(level, "Steam launch option", detail));
    }
    process(&mut c, game);
    finish(c)
}

fn finish(checks: Vec<Check>) -> Report {
    let mut text = String::from("Playkit diagnosis\n");
    for ch in &checks {
        let tag = match ch.level {
            Level::Ok => "OK  ",
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Bad => "BAD ",
        };
        text.push_str(&format!("[{tag}] {}: {}\n", ch.title, ch.detail));
    }
    Report { checks, text }
}

fn game_folder(c: &mut Vec<Check>, game: &Game) {
    let root = game.root.to_string_lossy().to_string();
    let lower = root.to_ascii_lowercase();
    let mut notes = Vec::new();
    if lower.contains("onedrive") {
        notes.push("the folder is inside OneDrive, which can hold back or lock files");
    }
    if lower.contains("program files") {
        notes.push("the folder is under Program Files, where Windows may block writes");
    }
    let probe = game.binaries().join(".playkit-write-test");
    let writable = std::fs::write(&probe, b"x").is_ok();
    let _ = std::fs::remove_file(&probe);
    if !writable {
        c.push(check(
            Level::Bad,
            "Game folder",
            format!("{root}: Playkit cannot write to Binaries/Win64. Controlled folder access or file permissions block it, so the loader and its log cannot be written either."),
        ));
    } else if notes.is_empty() {
        c.push(check(Level::Ok, "Game folder", root));
    } else {
        c.push(check(
            Level::Warn,
            "Game folder",
            format!("{root}: {}.", notes.join("; ")),
        ));
    }
}

fn build(c: &mut Vec<Check>, game: &Game, index: Option<&Index>) {
    let Some(id) = &game.build_id else {
        c.push(check(
            Level::Info,
            "Game version",
            "Unknown (no Steam app manifest found).",
        ));
        return;
    };
    match index {
        Some(idx) => match idx.build_name(id) {
            Some(name) => c.push(check(
                Level::Ok,
                "Game version",
                format!("{name} (build {id})"),
            )),
            None => c.push(check(
                Level::Warn,
                "Game version",
                format!("Build {id} is not in the mod list. Mods were made for another version."),
            )),
        },
        None => c.push(check(Level::Info, "Game version", format!("Build {id}"))),
    }
}

fn loader_dll(c: &mut Vec<Check>, game: &Game, bundled: &Bundled) {
    let path = game.binaries().join(DLL);
    let Ok(data) = std::fs::read(&path) else {
        c.push(check(
            Level::Bad,
            "Loader dxgi.dll",
            "Missing in Binaries/Win64. Run Patch game in Setup.",
        ));
        return;
    };
    let ours = loader::loader_version(bundled.bytes);
    match loader::loader_version(&data) {
        None => c.push(check(Level::Warn, "Loader dxgi.dll", format!("A dxgi.dll from another tool is installed ({} bytes). It may not load loose files.", data.len()))),
        Some(v) => {
            let same = fsx::sha256(&data) == fsx::sha256(bundled.bytes);
            let detail = format!("Playkit loader {v}{}", if same { ", identical to the one in this Playkit" } else { "" });
            let level = match &ours {
                Some(o) if v < *o => Level::Warn,
                _ => Level::Ok,
            };
            c.push(check(level, "Loader dxgi.dll", if level == Level::Warn { format!("{detail}; older than this Playkit, patch the game again.") } else { detail }));
        }
    }
    if cfg!(windows) {
        let zone = PathBuf::from(format!("{}:Zone.Identifier", path.display()));
        if std::fs::metadata(&zone).is_ok() {
            c.push(check(Level::Warn, "Download mark", "dxgi.dll is marked as downloaded from the internet. Some security software blocks such DLLs: right click, Properties, Unblock."));
        }
    }
}

fn conflicts(c: &mut Vec<Check>, game: &Game) {
    let mut found = Vec::new();
    for dir in [game.binaries(), game.root.clone(), game.root.join(PROJECT)] {
        for name in PROXIES.iter().chain(OTHER_TOOLS) {
            if dir == game.binaries() && *name == DLL {
                continue;
            }
            if dir.join(name).exists() {
                found.push(rel(game, &dir.join(name)));
            }
        }
    }
    let local = game.binaries().join(format!("{EXE}.exe.local"));
    if local.exists() {
        found.push(format!(
            "{} (makes Windows load DLLs from that folder instead)",
            rel(game, &local)
        ));
    }
    if found.is_empty() {
        c.push(check(
            Level::Ok,
            "Other mod tools",
            "No other proxy DLL or mod loader found.",
        ));
    } else {
        c.push(check(
            Level::Warn,
            "Other mod tools",
            format!(
                "Found: {}. These can load instead of the Playkit loader or stop it.",
                found.join(", ")
            ),
        ));
    }
}

fn last_run(c: &mut Vec<Check>, game: &Game) {
    match loader::last_run(game) {
        None => c.push(check(Level::Bad, "Last game start", "No loader log found, neither in Binaries/Win64 nor in the temp folder. The game has not loaded the Playkit dxgi.dll since it was installed, or it was never started.")),
        Some(r) => {
            let age = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|n| n.as_secs().saturating_sub(r.modified))
                .unwrap_or(0);
            let when = if age < 120 { "just now".to_string() } else if age < 7200 { format!("{} min ago", age / 60) } else { format!("{} h ago", age / 3600) };
            let files = r.loose_files.map(|n| format!(", {n} mod files found")).unwrap_or_default();
            if r.problems.is_empty() {
                c.push(check(Level::Ok, "Last game start", format!("The loader ran {when}{files}.")));
            } else {
                c.push(check(Level::Warn, "Last game start", format!("The loader ran {when}{files}, with problems: {}", r.problems.join(" | "))));
            }
        }
    }
}

fn anti_cheat(c: &mut Vec<Check>, game: &Game) {
    let st = anticheat::status(game);
    let (level, detail) = match st {
        AntiCheat::Absent => (Level::Ok, "Not present."),
        AntiCheat::Patched => (Level::Ok, "Settings.json is set to 123."),
        AntiCheat::Original => (
            Level::Warn,
            "Settings.json is original. Run Patch game in Setup.",
        ),
    };
    let mut detail = detail.to_string();
    if game.root.join("start_protected_game.exe").exists() {
        detail.push_str(" start_protected_game.exe exists; if Steam starts the game through it, Easy Anti-Cheat blocks the loader.");
    }
    c.push(check(level, "Easy Anti-Cheat", detail));
}

fn mods(c: &mut Vec<Check>, game: &Game) {
    let content = game.root.join(PROJECT).join("Content");
    let mut n = 0usize;
    count(&content, &mut n);
    let state = InstallState::load(game).unwrap_or_default();
    let active: Vec<&String> = state
        .mods
        .iter()
        .filter(|(_, m)| !m.disabled)
        .map(|(id, _)| id)
        .collect();
    let broken: Vec<String> = active
        .iter()
        .filter(|id| {
            matches!(
                state.health(game, id),
                crate::install::Health::Changed { .. }
            )
        })
        .map(|id| id.to_string())
        .collect();
    let detail = format!(
        "{n} loose files in Content; {} active Playkit mods{}",
        active.len(),
        if active.is_empty() {
            String::new()
        } else {
            format!(
                " ({})",
                active
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    );
    if !broken.is_empty() {
        c.push(check(
            Level::Warn,
            "Mod files",
            format!(
                "{detail}. Changed outside Playkit: {}. Use Repair.",
                broken.join(", ")
            ),
        ));
    } else {
        c.push(check(
            if n == 0 { Level::Info } else { Level::Ok },
            "Mod files",
            detail,
        ));
    }
}

fn count(dir: &Path, n: &mut usize) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.file_name().is_some_and(|f| f == "Paks") {
            continue;
        }
        if p.is_dir() {
            count(&p, n);
        } else if p.extension().is_some_and(|x| x == "uasset" || x == "umap") {
            *n += 1;
        }
    }
}

fn rel(game: &Game, p: &Path) -> String {
    p.strip_prefix(&game.root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

/// While the game runs: which executable Steam started and which dxgi.dll it loaded.
fn process(c: &mut Vec<Check>, game: &Game) {
    let procs = running_games();
    if procs.is_empty() {
        c.push(check(Level::Info, "Running game", "The game is not running. Start it, wait for the main menu and run the check again: Playkit then sees which dxgi.dll the game really loaded."));
        return;
    }
    for (pid, exe) in procs {
        let exe_s = exe
            .as_ref()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_else(|| "?".into());
        // Under Proton the process image is Wine's loader, so only a real game exe path says something.
        let is_game_exe = exe.as_ref().is_some_and(|e| {
            e.to_string_lossy()
                .to_ascii_lowercase()
                .contains("shipping")
        });
        if is_game_exe && !exe.as_ref().is_some_and(|e| same_dir(e, &game.binaries())) {
            c.push(check(Level::Bad, "Running game", format!("Steam started {exe_s}, not the game in {}. Playkit patched a different copy of the game.", game.root.display())));
            continue;
        }
        match loaded_dxgi(pid) {
            ModuleScan::Found(path) => {
                if same_dir(&path, &game.binaries()) {
                    c.push(check(
                        Level::Ok,
                        "Running game",
                        format!("Loaded {} (the Playkit loader).", path.display()),
                    ));
                } else {
                    c.push(check(
                        Level::Bad,
                        "Running game",
                        format!(
                            "The game loaded {} instead of the Playkit loader in its own folder.",
                            path.display()
                        ),
                    ));
                }
            }
            ModuleScan::NotLoaded => c.push(check(
                Level::Bad,
                "Running game",
                "The running game has no dxgi.dll loaded at all.",
            )),
            ModuleScan::Unsupported(why) => c.push(check(
                Level::Info,
                "Running game",
                format!("Running ({exe_s}); {why}"),
            )),
        }
    }
}

fn same_dir(file: &Path, dir: &Path) -> bool {
    let norm = |p: &Path| {
        p.to_string_lossy()
            .replace('\\', "/")
            .to_ascii_lowercase()
            .trim_end_matches('/')
            .to_string()
    };
    file.parent().is_some_and(|p| norm(p) == norm(dir))
}

fn running_games() -> Vec<(u32, Option<PathBuf>)> {
    use sysinfo::{ProcessRefreshKind, RefreshKind, System, UpdateKind};
    let sys = System::new_with_specifics(
        RefreshKind::nothing()
            .with_processes(ProcessRefreshKind::nothing().with_exe(UpdateKind::Always)),
    );
    let want = EXE.to_ascii_lowercase();
    sys.processes()
        .iter()
        .filter(|(_, p)| {
            let n = p.name().to_string_lossy().to_ascii_lowercase();
            let n = n.strip_suffix(".exe").unwrap_or(&n).to_string();
            n == want || (n.len() == 15 && want.starts_with(&n))
        })
        .map(|(pid, p)| (pid.as_u32(), p.exe().map(Path::to_path_buf)))
        .collect()
}

enum ModuleScan {
    Found(PathBuf),
    NotLoaded,
    Unsupported(&'static str),
}

#[cfg(windows)]
fn loaded_dxgi(pid: u32) -> ModuleScan {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, MODULEENTRY32W, TH32CS_SNAPMODULE,
        TH32CS_SNAPMODULE32,
    };
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid);
        if snap == INVALID_HANDLE_VALUE {
            return ModuleScan::Unsupported("Windows did not allow reading its loaded DLLs (try running Playkit normally, not as another user).");
        }
        let mut e: MODULEENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
        let mut found = None;
        let mut ok = Module32FirstW(snap, &mut e) != 0;
        while ok {
            let len = e
                .szExePath
                .iter()
                .position(|&ch| ch == 0)
                .unwrap_or(e.szExePath.len());
            let path = PathBuf::from(String::from_utf16_lossy(&e.szExePath[..len]));
            if path
                .file_name()
                .is_some_and(|f| f.to_string_lossy().eq_ignore_ascii_case(DLL))
            {
                found = Some(path);
                break;
            }
            ok = Module32NextW(snap, &mut e) != 0;
        }
        CloseHandle(snap);
        found.map_or(ModuleScan::NotLoaded, ModuleScan::Found)
    }
}

#[cfg(not(windows))]
fn loaded_dxgi(pid: u32) -> ModuleScan {
    // Under Proton the game is a Wine process; its maps list the Windows DLLs it loaded.
    let Ok(maps) = std::fs::read_to_string(format!("/proc/{pid}/maps")) else {
        return ModuleScan::Unsupported("its loaded DLLs cannot be read on this system.");
    };
    maps.lines()
        .filter_map(|l| l.split_whitespace().nth(5))
        .find(|p| p.to_ascii_lowercase().ends_with("/dxgi.dll"))
        .map_or(ModuleScan::NotLoaded, |p| {
            ModuleScan::Found(PathBuf::from(p))
        })
}
