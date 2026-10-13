use crate::{vdf, Error, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

const STEAMID64_BASE: u64 = 76561197960265728;

#[derive(Debug, Clone, Serialize)]
pub struct Steam {
    pub root: PathBuf,
    pub flatpak: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppInstall {
    pub path: PathBuf,
    pub build_id: Option<String>,
    pub name: Option<String>,
}

pub fn find() -> Vec<Steam> {
    let mut out: Vec<Steam> = Vec::new();
    for (path, flatpak) in candidates() {
        if !path.join("steamapps").is_dir() {
            continue;
        }
        let canon = path.canonicalize().unwrap_or(path);
        if out.iter().all(|s| s.root != canon) {
            out.push(Steam {
                root: canon,
                flatpak,
            });
        }
    }
    out
}

#[cfg(windows)]
fn candidates() -> Vec<(PathBuf, bool)> {
    use winreg::{enums::*, RegKey};
    let mut v = Vec::new();
    if let Ok(k) = RegKey::predef(HKEY_CURRENT_USER).open_subkey("Software\\Valve\\Steam") {
        if let Ok(p) = k.get_value::<String, _>("SteamPath") {
            v.push((PathBuf::from(p.replace('/', "\\")), false));
        }
    }
    for sub in [
        "SOFTWARE\\WOW6432Node\\Valve\\Steam",
        "SOFTWARE\\Valve\\Steam",
    ] {
        if let Ok(k) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(sub) {
            if let Ok(p) = k.get_value::<String, _>("InstallPath") {
                v.push((PathBuf::from(p), false));
            }
        }
    }
    for env in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Ok(p) = std::env::var(env) {
            v.push((PathBuf::from(p).join("Steam"), false));
        }
    }
    v
}

#[cfg(not(windows))]
fn candidates() -> Vec<(PathBuf, bool)> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    vec![
        (home.join(".local/share/Steam"), false),
        (home.join(".steam/steam"), false),
        (home.join(".steam/root"), false),
        (home.join(".steam/debian-installation"), false),
        (
            home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
            true,
        ),
        (home.join("snap/steam/common/.local/share/Steam"), false),
    ]
}

fn read_vdf(path: &Path) -> Result<vdf::Obj> {
    let s = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
    vdf::parse(&s)
}

impl Steam {
    pub fn libraries(&self) -> Vec<PathBuf> {
        let mut libs = vec![self.root.clone()];
        if let Ok(o) = read_vdf(&self.root.join("steamapps/libraryfolders.vdf")) {
            if let Some(folders) = o.obj("libraryfolders") {
                for (_, v) in &folders.0 {
                    if let vdf::Value::Obj(f) = v {
                        if let Some(p) = f.str("path") {
                            let p = PathBuf::from(p);
                            if !libs.contains(&p) {
                                libs.push(p);
                            }
                        }
                    }
                }
            }
        }
        libs
    }

    pub fn find_app(&self, app_id: u32) -> Option<AppInstall> {
        self.libraries().into_iter().find_map(|lib| {
            let manifest = lib.join(format!("steamapps/appmanifest_{app_id}.acf"));
            let o = read_vdf(&manifest).ok()?;
            let state = o.obj("AppState")?;
            let dir = lib.join("steamapps/common").join(state.str("installdir")?);
            dir.is_dir().then(|| AppInstall {
                path: dir,
                build_id: state.str("buildid").map(str::to_string),
                name: state.str("name").map(str::to_string),
            })
        })
    }

    pub fn active_account(&self) -> Option<u32> {
        let users = read_vdf(&self.root.join("config/loginusers.vdf")).ok()?;
        let users = users.obj("users")?;
        let pick = users
            .0
            .iter()
            .find(|(_, v)| matches!(v, vdf::Value::Obj(u) if u.str("MostRecent") == Some("1")))
            .or_else(|| users.0.first())?;
        let id64: u64 = pick.0.parse().ok()?;
        u32::try_from(id64.checked_sub(STEAMID64_BASE)?).ok()
    }

    fn localconfig(&self, account: u32) -> PathBuf {
        self.root
            .join("userdata")
            .join(account.to_string())
            .join("config/localconfig.vdf")
    }

    pub fn launch_options(&self, account: u32, app_id: u32) -> Result<String> {
        let o = read_vdf(&self.localconfig(account))?;
        let app_id = app_id.to_string();
        let opts = o
            .path(&[
                "UserLocalConfigStore",
                "Software",
                "Valve",
                "Steam",
                "apps",
                &app_id,
            ])
            .and_then(|a| a.str("LaunchOptions"))
            .unwrap_or_default();
        Ok(opts.to_string())
    }

    pub fn set_launch_options(&self, account: u32, app_id: u32, value: &str) -> Result<()> {
        if is_running() {
            return Err(Error::SteamRunning);
        }
        let path = self.localconfig(account);
        let mut o = read_vdf(&path)?;
        let app = [
            "UserLocalConfigStore",
            "Software",
            "Valve",
            "Steam",
            "apps",
            &app_id.to_string(),
        ]
        .iter()
        .fold(&mut o, |obj, k| obj.obj_mut_or_insert(k));
        app.set_str("LaunchOptions", value);
        let backup = path.with_extension("vdf.playkit-backup");
        std::fs::copy(&path, &backup).map_err(|e| Error::io(&backup, e))?;
        crate::fsx::write_atomic(&path, vdf::write(&o).as_bytes())
    }
}

pub fn is_running() -> bool {
    crate::platform::process_running(&["steam", "steamwebhelper"])
}

pub fn run_url(app_id: u32) -> String {
    format!("steam://rungameid/{app_id}")
}
