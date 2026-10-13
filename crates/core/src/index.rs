use crate::Result;
use serde::{Deserialize, Serialize};

pub const DEFAULT_URL: &str =
    "https://raw.githubusercontent.com/MCbabel/playkit-mods/main/index.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Index {
    pub schema: u32,
    pub game: GameInfo,
    pub mods: Vec<Mod>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameInfo {
    pub builds: Vec<Build>,
    pub pak_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Build {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Network {
    Safe,
    Lobby,
    Offline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mod {
    pub id: String,
    pub name: String,
    pub version: String,
    pub authors: Vec<String>,
    pub summary: String,
    #[serde(default)]
    pub description: String,
    pub category: String,
    pub network: Network,
    pub game_builds: Vec<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub changelog: Vec<Release>,
    #[serde(default)]
    pub requires: Vec<String>,
    pub files: Vec<ModFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    pub changes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModFile {
    pub path: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    #[serde(default)]
    pub base_sha256: Option<String>,
}

impl ModFile {
    pub fn is_patch(&self) -> bool {
        self.base_sha256.is_some()
    }
}

impl Index {
    pub fn get(&self, id: &str) -> Option<&Mod> {
        self.mods.iter().find(|m| m.id == id)
    }

    pub fn build_name(&self, id: &str) -> Option<&str> {
        self.game
            .builds
            .iter()
            .find(|b| b.id == id)
            .map(|b| b.name.as_str())
    }
}

pub async fn fetch(url: &str) -> Result<Index> {
    if let Some(path) = url.strip_prefix("file://") {
        let data = std::fs::read(path).map_err(|e| crate::Error::io(path.as_ref(), e))?;
        return Ok(serde_json::from_slice(&data)?);
    }
    let body = client()
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    Ok(serde_json::from_slice(&body)?)
}

pub async fn download(url: &str) -> Result<Vec<u8>> {
    if let Some(path) = url.strip_prefix("file://") {
        return std::fs::read(path).map_err(|e| crate::Error::io(path.as_ref(), e));
    }
    Ok(client()
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?
        .to_vec())
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("playkit/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("http client")
}

pub fn version_newer(available: &str, installed: &str) -> bool {
    match (lenient(available), lenient(installed)) {
        (Some(a), Some(b)) => a > b,
        _ => available != installed,
    }
}

fn lenient(v: &str) -> Option<semver::Version> {
    let v = v.trim().trim_start_matches('v');
    let parts = v.split('.').count();
    let full = match parts {
        1 => format!("{v}.0.0"),
        2 => format!("{v}.0"),
        _ => v.to_string(),
    };
    semver::Version::parse(&full).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(version_newer("2.2", "2.1"));
        assert!(version_newer("1.0.1", "1.0"));
        assert!(!version_newer("1.0", "1.0.0"));
        assert!(!version_newer("v2.0", "2.0"));
    }
}
