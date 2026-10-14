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
pub const MAX_FILE: u64 = 50 * 1024 * 1024;
/// Folder of native (C++) mods, loaded by the Playkit loader.
pub const NATIVE_DIR: &str = "Binaries/Win64/playkit-mods";
/// Source code that comes with a mod: required for native mods, and `.bps` patches are kept there.
pub const SOURCE_DIR: &str = "source";
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
    /// Source files kept next to the mod for review.
    pub sources: Vec<String>,
}

impl Manifest {
    pub fn load(path: &Path) -> Result<Manifest> {
        let s = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
        toml::from_str(&s).map_err(|e| Error::Invalid(format!("{}: {e}", path.display())))
    }

    pub fn validate(&self) -> Result<()> {
        let bad = |m: String| Err(Error::Invalid(format!("{}: {m}", self.id)));
        let id_ok = valid_id(&self.id);
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

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 48
        && !id.starts_with('-')
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
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

/// The only place a native mod may put code: `Binaries/Win64/playkit-mods/<id>.dll`.
pub fn native_path(id: &str) -> String {
    format!("{PROJECT}/{NATIVE_DIR}/{id}.dll")
}

/// Like [`check_path`], but also allows the native DLL of mod `id`.
pub fn check_mod_path(rel: &str, id: &str) -> Result<()> {
    if valid_id(id) && rel == native_path(id) {
        return Ok(());
    }
    check_path(rel)
}

/// Files below `dir`, relative to it, with `/` separators.
fn rel_files(dir: &Path) -> Result<Vec<String>> {
    let mut paths = Vec::new();
    if dir.is_dir() {
        walk(dir, &mut paths)?;
    }
    let mut out: Vec<String> = paths
        .iter()
        .map(|p| {
            p.strip_prefix(dir)
                .expect("inside dir")
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    out.sort();
    Ok(out)
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

/// Packs the mod folder `src` into `repo/mods/<id>`. Changed game files become patches against
/// the original. A `.bps` file next to where an asset would be (`UI_Foo.bps` for
/// `UI_Foo.uasset`) is applied to the original asset with bpscript first; `reflection` is the
/// database it uses for short names. A native mod puts its DLL at [`native_path`] and its code in
/// `source/`.
pub fn pack(
    src: &Path,
    game_pak: &Path,
    pak_key: &str,
    repo: &Path,
    reflection: Option<&Path>,
) -> Result<PackReport> {
    let manifest = Manifest::load(&src.join("mod.toml"))?;
    manifest.validate()?;
    let id = manifest.id.clone();
    let content = src.join(PROJECT);
    if !content.is_dir() {
        return Err(Error::Invalid(format!(
            "{} needs a {PROJECT} folder with your changed files",
            src.display()
        )));
    }
    let rels: Vec<String> = rel_files(&content)?
        .into_iter()
        .map(|r| format!("{PROJECT}/{r}"))
        .collect();
    let sources = rel_files(&src.join(SOURCE_DIR))?;
    for s in &sources {
        let ext = s
            .rsplit('.')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if ["exe", "dll", "so", "lib", "a", "o", "obj", "pdb"].contains(&ext.as_str()) {
            return Err(Error::Invalid(format!(
                "{SOURCE_DIR}/{s}: only source code belongs in {SOURCE_DIR}/, no built files"
            )));
        }
    }
    let native = native_path(&id);
    if rels.contains(&native) && sources.is_empty() {
        return Err(Error::Invalid(format!(
            "{native} needs its source code in a {SOURCE_DIR} folder next to mod.toml"
        )));
    }
    if let Some(dll) = rels.iter().find(|r| {
        r.to_ascii_lowercase().ends_with(".dll") && **r != native && r.contains(NATIVE_DIR)
    }) {
        return Err(Error::Invalid(format!(
            "{dll}: a native mod has exactly one DLL, named {native}"
        )));
    }
    let mut pak = Pak::open(game_pak, pak_key)?;
    let mut targets: Vec<(String, Vec<u8>)> = Vec::new();
    let mut scripts: Vec<String> = Vec::new();
    let mut db: Option<playkit_reflect::Db> = None;
    for rel in &rels {
        if !rel.to_ascii_lowercase().ends_with(".bps") {
            check_mod_path(rel, &id)?;
            let p = src.join(rel);
            targets.push((
                rel.clone(),
                std::fs::read(&p).map_err(|e| Error::io(&p, e))?,
            ));
            continue;
        }
        let stem = &rel[..rel.len() - 4];
        let header = [".uasset", ".umap"]
            .iter()
            .map(|e| format!("{stem}{e}"))
            .find(|h| pak.contains(h))
            .ok_or_else(|| Error::Invalid(format!("{rel}: the game has no asset {stem}.uasset")))?;
        let uexp = format!("{stem}.uexp");
        if let Some(both) = [&header, &uexp].into_iter().find(|f| rels.contains(f)) {
            return Err(Error::Invalid(format!(
                "{rel} and {both}: use either the .bps patch or the changed asset"
            )));
        }
        if db.is_none() {
            if let Some(p) = reflection.filter(|p| p.is_file()) {
                db = Some(playkit_reflect::Db::load(p).map_err(Error::Invalid)?);
            }
        }
        let index = db.as_ref().map(playkit_reflect::Db::index);
        let text =
            std::fs::read_to_string(src.join(rel)).map_err(|e| Error::io(&src.join(rel), e))?;
        let base = bpscript::Files {
            uasset: pak.read(&header)?,
            uexp: pak.read(&uexp)?,
        };
        let bad = |e: bpscript::Error| Error::Invalid(format!("{rel}: {e}"));
        let mut asset = bpscript::load(&base).map_err(bad)?;
        bpscript::patch::apply_with(&mut asset, &text, index.as_ref()).map_err(bad)?;
        let out = bpscript::save(&asset).map_err(bad)?;
        targets.push((header, out.uasset));
        targets.push((uexp, out.uexp));
        scripts.push(rel.clone());
    }
    let out = repo.join("mods").join(&id);
    for dir in [out.join("files"), out.join(SOURCE_DIR)] {
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        }
    }
    let files_dir = out.join("files");
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    for (rel, target) in targets {
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
    let mut source_files = Vec::new();
    for s in &sources {
        fsx::copy_into(&src.join(SOURCE_DIR).join(s), &out.join(SOURCE_DIR).join(s))?;
        source_files.push(format!("{SOURCE_DIR}/{s}"));
    }
    for rel in scripts {
        fsx::copy_into(&src.join(&rel), &out.join(SOURCE_DIR).join(&rel))?;
        source_files.push(format!("{SOURCE_DIR}/{rel}"));
    }
    fsx::copy_into(&src.join("mod.toml"), &out.join("mod.toml"))?;
    fsx::write_atomic(&out.join("files.json"), &serde_json::to_vec_pretty(&files)?)?;
    Ok(PackReport {
        id,
        out,
        files,
        skipped_unchanged: skipped,
        sources: source_files,
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
        let native = packed.iter().any(|f| f.path == native_path(&m.id));
        if native && rel_files(&dir.join(SOURCE_DIR))?.is_empty() {
            return Err(Error::Invalid(format!(
                "{}: native mods must include their source code in {SOURCE_DIR}/",
                m.id
            )));
        }
        let mut files = Vec::new();
        for f in packed {
            check_mod_path(&f.path, &m.id)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_dll_only_at_its_own_path() {
        let own = native_path("hello");
        assert_eq!(
            own,
            "Playtime_Multiplayer/Binaries/Win64/playkit-mods/hello.dll"
        );
        assert!(check_mod_path(&own, "hello").is_ok());
        assert!(check_mod_path(&own, "other").is_err());
        assert!(check_path(&own).is_err());
        for bad in [
            "Playtime_Multiplayer/Binaries/Win64/dxgi.dll",
            "Playtime_Multiplayer/Binaries/Win64/playkit-mods/../dxgi.dll",
            "Playtime_Multiplayer/Binaries/Win64/playkit-mods/hello.exe",
            "Playtime_Multiplayer/Content/Mods/hello.dll",
        ] {
            assert!(check_mod_path(bad, "hello").is_err(), "{bad}");
        }
        assert!(check_mod_path("Playtime_Multiplayer/Content/UI/UI_A.uasset", "hello").is_ok());
    }
}
