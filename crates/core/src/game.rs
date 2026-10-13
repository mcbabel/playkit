use crate::{platform, steam, Error, Result, APP_ID};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub const PROJECT: &str = "Playtime_Multiplayer";
pub const EXE: &str = "Playtime_Multiplayer-Win64-Shipping";

#[derive(Debug, Clone, Serialize)]
pub struct Game {
    pub root: PathBuf,
    pub build_id: Option<String>,
    pub steam: Option<steam::Steam>,
}

impl Game {
    pub fn detect() -> Result<Game> {
        for s in steam::find() {
            if let Some(app) = s.find_app(APP_ID) {
                if is_game_dir(&app.path) {
                    return Ok(Game {
                        root: app.path,
                        build_id: app.build_id,
                        steam: Some(s),
                    });
                }
            }
        }
        Err(Error::GameNotFound)
    }

    pub fn from_dir(dir: &Path) -> Result<Game> {
        if !is_game_dir(dir) {
            return Err(Error::NotAGameFolder(dir.to_path_buf()));
        }
        let root = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        let steam = steam::find()
            .into_iter()
            .find(|s| s.libraries().iter().any(|l| root.starts_with(l)));
        let build_id = steam
            .as_ref()
            .and_then(|s| s.find_app(APP_ID))
            .and_then(|a| a.build_id);
        Ok(Game {
            root,
            build_id,
            steam,
        })
    }

    pub fn binaries(&self) -> PathBuf {
        self.root.join(PROJECT).join("Binaries/Win64")
    }

    pub fn pak(&self) -> PathBuf {
        self.root
            .join(PROJECT)
            .join("Content/Paks")
            .join(format!("{PROJECT}-WindowsNoEditor.pak"))
    }

    pub fn data_dir(&self) -> PathBuf {
        self.root.join(".playkit")
    }

    pub fn resolve(&self, rel: &str) -> Result<PathBuf> {
        let rel = rel.replace('\\', "/");
        let clean = !rel.is_empty()
            && rel.starts_with(&format!("{PROJECT}/"))
            && rel
                .split('/')
                .all(|p| !p.is_empty() && p != "." && p != "..")
            && !rel.contains(':');
        if !clean {
            return Err(Error::Invalid(format!("bad file path in mod: {rel}")));
        }
        Ok(self.root.join(rel))
    }
}

pub fn is_game_dir(dir: &Path) -> bool {
    dir.join(PROJECT)
        .join("Binaries/Win64")
        .join(format!("{EXE}.exe"))
        .is_file()
}

pub fn is_running() -> bool {
    platform::process_running(&[EXE])
}

impl Game {
    pub fn is_running(&self) -> bool {
        platform::process_running_in(&[EXE], Some(&self.root))
    }
}
