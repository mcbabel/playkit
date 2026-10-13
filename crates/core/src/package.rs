use crate::game::PROJECT;
use crate::index::{Build, GameInfo, Index, Mod, ModFile, Network, Release};
use crate::pak::{self, Pak};
use crate::{fsx, Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const CATEGORIES: &[&str] = &[
    "multiplayer",
    "interface",
    "fixes",
    "offline",
    "library",
    "other",
];
const MAX_FILE: u64 = 50 * 1024 * 1024;
const BLOCKED_EXT: &[&str] = &["exe", "dll", "bat", "cmd", "ps1", "sh", "so", "lua", "py"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackedFile {
    pub path: String,
    pub payload: String,
    pub sha256: String,
    pub payload_sha256: String,
    pub size: u64,
    #[serde(default)]
    pub base_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Registry {
    pub game: GameInfo,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackReport {
    pub id: String,
    pub out: PathBuf,
    pub files: Vec<PackedFile>,
    pub skipped_unchanged: Vec<String>,
}

impl Manifest {
    pub fn load(path: &Path) -> Result<Manifest> {
        let s = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
        toml::from_str(&s).map_err(|e| Error::Invalid(format!("{}: {e}", path.display())))
    }

    pub fn validate(&self) -> Result<()> {
        let bad = |m: String| Err(Error::Invalid(format!("{}: {m}", self.id)));
        let id_ok = !self.id.is_empty()
            && self.id.len() <= 48
            && self
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && !self.id.starts_with('-');
        if !id_ok {
            return bad("id must be lowercase letters, digits and dashes".into());
        }
        if self.name.trim().is_empty() || self.summary.trim().is_empty() || self.authors.is_empty()
        {
            return bad("name, summary and authors are required".into());
        }
        if semver::Version::parse(&self.version).is_err() {
            return bad(format!("version {} is not semver (1.2.3)", self.version));
        }
        if !CATEGORIES.contains(&self.category.as_str()) {
            return bad(format!("category must be one of {}", CATEGORIES.join(", ")));
        }
        if self.game_builds.is_empty() {
            return bad("game_builds is empty".into());
        }
        if let Some(c) = &self.color {
            if !(c.len() == 7
                && c.starts_with('#')
                && c[1..].chars().all(|x| x.is_ascii_hexdigit()))
            {
                return bad("color must look like #3A6BFF".into());
            }
        }
        Ok(())
    }
}

pub fn check_path(rel: &str) -> Result<()> {
    let ok = rel.starts_with(&format!("{PROJECT}/Content/"))
        && rel
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
        && !rel.contains(['\\', ':']);
    let ext = rel
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !ok || BLOCKED_EXT.contains(&ext.as_str()) || rel.contains("/Paks/") {
        return Err(Error::Invalid(format!(
            "{rel}: only game content below {PROJECT}/Content is allowed"
        )));
    }
    Ok(())
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for e in std::fs::read_dir(dir).map_err(|e| Error::io(dir, e))? {
        let e = e.map_err(|e| Error::io(dir, e))?;
        let p = e.path();
        if p.is_dir() {
            walk(&p, out)?;
        } else {
            out.push(p);
        }
    }
    Ok(())
}

pub fn pack(src: &Path, game_pak: &Path, pak_key: &str, repo: &Path) -> Result<PackReport> {
    let manifest = Manifest::load(&src.join("mod.toml"))?;
    manifest.validate()?;
    let content = src.join(PROJECT);
    if !content.is_dir() {
        return Err(Error::Invalid(format!(
            "{} needs a {PROJECT} folder with your changed files",
            src.display()
        )));
    }
    let mut paths = Vec::new();
    walk(&content, &mut paths)?;
    paths.sort();
    let out = repo.join("mods").join(&manifest.id);
    let files_dir = out.join("files");
    if files_dir.exists() {
        std::fs::remove_dir_all(&files_dir).map_err(|e| Error::io(&files_dir, e))?;
    }
    let mut pak = Pak::open(game_pak, pak_key)?;
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    for p in paths {
        let rel = p
            .strip_prefix(src)
            .expect("inside src")
            .to_string_lossy()
            .replace('\\', "/");
        check_path(&rel)?;
        let target = std::fs::read(&p).map_err(|e| Error::io(&p, e))?;
        let (payload_name, payload, base_sha256) = if pak.contains(&rel) {
            let base = pak.read(&rel)?;
            if base == target {
                skipped.push(rel);
                continue;
            }
            (
                format!("{rel}.bsdiff"),
                pak::make_patch(&base, &target),
                Some(fsx::sha256(&base)),
            )
        } else {
            (rel.clone(), target.clone(), None)
        };
        if payload.len() as u64 > MAX_FILE {
            return Err(Error::Invalid(format!("{rel} is larger than 50 MB")));
        }
        let dst = files_dir.join(&payload_name);
        fsx::write_atomic(&dst, &payload)?;
        files.push(PackedFile {
            path: rel,
            payload: format!("files/{payload_name}"),
            sha256: fsx::sha256(&target),
            payload_sha256: fsx::sha256(&payload),
            size: payload.len() as u64,
            base_sha256,
        });
    }
    if files.is_empty() {
        return Err(Error::Invalid("no changed files found".into()));
    }
    fsx::copy_into(&src.join("mod.toml"), &out.join("mod.toml"))?;
    fsx::write_atomic(&out.join("files.json"), &serde_json::to_vec_pretty(&files)?)?;
    Ok(PackReport {
        id: manifest.id,
        out,
        files,
        skipped_unchanged: skipped,
    })
}

pub fn build_index(repo: &Path, base_url: &str) -> Result<Index> {
    let reg_path = repo.join("registry.toml");
    let reg_src = std::fs::read_to_string(&reg_path).map_err(|e| Error::io(&reg_path, e))?;
    let reg: Registry =
        toml::from_str(&reg_src).map_err(|e| Error::Invalid(format!("registry.toml: {e}")))?;
    let known: Vec<&str> = reg
        .game
        .builds
        .iter()
        .map(|b: &Build| b.id.as_str())
        .collect();
    let mods_dir = repo.join("mods");
    let mut dirs: Vec<PathBuf> = match std::fs::read_dir(&mods_dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_dir())
            .collect(),
        Err(_) => Vec::new(),
    };
    dirs.sort();
    let base = base_url.trim_end_matches('/');
    let mut mods = Vec::new();
    for dir in dirs {
        let m = Manifest::load(&dir.join("mod.toml"))?;
        m.validate()?;
        let folder = dir.file_name().unwrap_or_default().to_string_lossy();
        if folder != m.id {
            return Err(Error::Invalid(format!(
                "folder {folder} must be named like its id {}",
                m.id
            )));
        }
        if let Some(b) = m.game_builds.iter().find(|b| !known.contains(&b.as_str())) {
            return Err(Error::Invalid(format!("{}: unknown game build {b}", m.id)));
        }
        let fj = dir.join("files.json");
        let packed: Vec<PackedFile> =
            serde_json::from_slice(&std::fs::read(&fj).map_err(|e| Error::io(&fj, e))?)?;
        if packed.is_empty() {
            return Err(Error::Invalid(format!("{}: no files", m.id)));
        }
        let mut files = Vec::new();
        for f in packed {
            check_path(&f.path)?;
            let payload_path = dir.join(&f.payload);
            let canon_ok =
                f.payload.starts_with("files/") && !f.payload.split('/').any(|p| p == "..");
            if !canon_ok {
                return Err(Error::Invalid(format!(
                    "{}: bad payload path {}",
                    m.id, f.payload
                )));
            }
            if fsx::sha256_file(&payload_path)? != f.payload_sha256 {
                return Err(Error::Checksum(format!("{}/{}", m.id, f.payload)));
            }
            if f.base_sha256.is_none() && f.payload_sha256 != f.sha256 {
                return Err(Error::Checksum(format!("{}/{}", m.id, f.path)));
            }
            files.push(ModFile {
                url: format!("{base}/mods/{}/{}", m.id, f.payload),
                path: f.path,
                sha256: f.sha256,
                size: f.size,
                base_sha256: f.base_sha256,
            });
        }
        mods.push(Mod {
            id: m.id,
            name: m.name,
            version: m.version,
            authors: m.authors,
            summary: m.summary,
            description: m.description,
            category: m.category,
            network: m.network,
            game_builds: m.game_builds,
            color: m.color,
            homepage: m.homepage,
            changelog: m.changelog,
            requires: m.requires,
            files,
        });
    }
    check_requirements(&mods)?;
    let mut owners: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for m in &mods {
        for f in &m.files {
            if let Some(other) = owners.insert(f.path.to_ascii_lowercase(), m.id.clone()) {
                if other != m.id {
                    eprintln!("note: {} and {} both change {}", other, m.id, f.path);
                }
            }
        }
    }
    Ok(Index {
        schema: 1,
        game: reg.game,
        mods,
    })
}

pub fn parse_toml<T: serde::de::DeserializeOwned>(s: &str) -> Result<T> {
    toml::from_str(s).map_err(|e| Error::Invalid(e.to_string()))
}

fn check_requirements(mods: &[Mod]) -> Result<()> {
    for m in mods {
        for r in &m.requires {
            if r == &m.id || !mods.iter().any(|x| &x.id == r) {
                return Err(Error::Invalid(format!(
                    "{}: requires unknown mod {r}",
                    m.id
                )));
            }
        }
        crate::install::resolve(mods, &m.id)?;
    }
    Ok(())
}
