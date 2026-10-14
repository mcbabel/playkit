pub mod anticheat;
pub mod diagnose;
pub mod fsx;
pub mod game;
pub mod index;
pub mod install;
pub mod loader;
pub mod package;
pub mod pak;
pub mod platform;
pub mod steam;
pub mod vdf;

use std::path::{Path, PathBuf};

pub const APP_ID: u32 = 1961460;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not read Steam config: {0}")]
    Vdf(String),
    #[error("Project Playtime was not found. Pick the game folder by hand.")]
    GameNotFound,
    #[error("{0} is not a Project Playtime folder")]
    NotAGameFolder(PathBuf),
    #[error("Close Steam first, then try again. Steam overwrites launch options while it runs.")]
    SteamRunning,
    #[error("Close the game first.")]
    GameRunning,
    #[error("download failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("mod list is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}: checksum does not match")]
    Checksum(String),
    #[error("game archive: {0}")]
    Pak(String),
    #[error("patch for {0} could not be applied")]
    Patch(String),
    #[error("unknown mod {0}")]
    UnknownMod(String),
    #[error("{file} is already changed by {owner}")]
    Conflict { file: String, owner: String },
    #[error("{mods} cannot be used together: {reason}")]
    Merge { mods: String, reason: String },
    #[error("{0}")]
    Invalid(String),
}

impl Error {
    pub fn io(path: &Path, source: std::io::Error) -> Self {
        Error::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

impl serde::Serialize for Error {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
