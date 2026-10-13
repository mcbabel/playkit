use crate::game::{self, Game};
use crate::index::{self, Index, Mod};
use crate::pak::{self, Pak};
use crate::{fsx, Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    pub mods: BTreeMap<String, Installed>,
    #[serde(default)]
    pub written: BTreeMap<String, String>,
    #[serde(default)]
    pub backups: BTreeSet<String>,
    #[serde(default)]
    pub next_seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Installed {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub seq: u64,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub auto: bool,
    pub files: Vec<InstalledFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledFile {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Health {
    Ok,
    Changed { files: Vec<String> },
}

enum Action {
    Write(String, Vec<u8>),
    Restore(String),
}

const ASSET_EXT: &[&str] = &["uasset", "umap", "uexp", "ubulk"];

fn stem(path: &str) -> String {
    match path.rsplit_once('.') {
        Some((s, ext)) if ASSET_EXT.contains(&ext.to_ascii_lowercase().as_str()) => {
            s.to_ascii_lowercase()
        }
        _ => path.to_ascii_lowercase(),
    }
}

impl State {
    fn path(game: &Game) -> PathBuf {
        game.data_dir().join("state.json")
    }

    pub fn load(game: &Game) -> Result<State> {
        let p = Self::path(game);
        match std::fs::read(&p) {
            Ok(d) => Ok(serde_json::from_slice(&d)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
            Err(e) => Err(Error::io(&p, e)),
        }
    }

    pub fn save(&self, game: &Game) -> Result<()> {
        fsx::write_atomic(&Self::path(game), &serde_json::to_vec_pretty(self)?)
    }

    pub fn sharing(&self, id: &str, paths: &[String]) -> Vec<String> {
        let stems: BTreeSet<String> = paths.iter().map(|p| stem(p)).collect();
        self.mods
            .iter()
            .filter(|(other, m)| {
                *other != id && m.files.iter().any(|f| stems.contains(&stem(&f.path)))
            })
            .map(|(other, _)| other.clone())
            .collect()
    }

    pub fn health(&self, game: &Game, id: &str) -> Health {
        let Some(m) = self.mods.get(id) else {
            return Health::Ok;
        };
        let changed: Vec<String> = m
            .files
            .iter()
            .filter(|f| {
                let expected = self.written.get(&f.path);
                let actual = game
                    .resolve(&f.path)
                    .ok()
                    .and_then(|p| fsx::sha256_file(&p).ok());
                expected.is_none() || expected != actual.as_ref()
            })
            .map(|f| f.path.clone())
            .collect();
        if changed.is_empty() {
            Health::Ok
        } else {
            Health::Changed { files: changed }
        }
    }

    fn owners(&self, stem_key: &str) -> Vec<(&String, &Installed)> {
        let mut v: Vec<_> = self
            .mods
            .iter()
            .filter(|(_, m)| m.files.iter().any(|f| stem(&f.path) == stem_key))
            .collect();
        v.sort_by_key(|(_, m)| m.seq);
        v
    }
}

fn cache_path(game: &Game, id: &str, rel: &str) -> PathBuf {
    game.data_dir().join("mods").join(id).join(rel)
}

fn backup_path(game: &Game, rel: &str) -> PathBuf {
    game.data_dir().join("backup/files").join(rel)
}

fn ensure_idle(game: &Game) -> Result<()> {
    if game.is_running() {
        return Err(Error::GameRunning);
    }
    Ok(())
}

struct Sources<'a> {
    game: &'a Game,
    fresh: &'a HashMap<String, Vec<u8>>,
    fresh_id: &'a str,
    pak: Option<Pak>,
    pak_key: &'a str,
}

impl Sources<'_> {
    fn mod_file(&self, id: &str, rel: &str) -> Result<Vec<u8>> {
        if id == self.fresh_id {
            if let Some(d) = self.fresh.get(rel) {
                return Ok(d.clone());
            }
        }
        let p = cache_path(self.game, id, rel);
        std::fs::read(&p).map_err(|e| Error::io(&p, e))
    }

    fn original(&mut self, rel: &str) -> Result<Option<Vec<u8>>> {
        if self.pak.is_none() {
            self.pak = Some(Pak::open(&self.game.pak(), self.pak_key)?);
        }
        let pak = self.pak.as_mut().expect("pak opened");
        if pak.contains(rel) {
            pak.read(rel).map(Some)
        } else {
            Ok(None)
        }
    }
}

fn plan(state: &State, stems: &BTreeSet<String>, src: &mut Sources) -> Result<Vec<Action>> {
    let mut actions = Vec::new();
    for s in stems {
        let owners = state.owners(s);
        let mut paths: BTreeSet<String> = state
            .written
            .keys()
            .filter(|p| stem(p) == *s)
            .cloned()
            .collect();
        for (_, m) in &owners {
            paths.extend(
                m.files
                    .iter()
                    .filter(|f| stem(&f.path) == *s)
                    .map(|f| f.path.clone()),
            );
        }
        let has = |m: &Installed, p: &str| m.files.iter().any(|f| f.path == p);
        match owners.as_slice() {
            [] => actions.extend(paths.into_iter().map(Action::Restore)),
            [(id, m)] => {
                for p in paths {
                    if has(m, &p) {
                        actions.push(Action::Write(p.clone(), src.mod_file(id, &p)?));
                    } else {
                        actions.push(Action::Restore(p));
                    }
                }
            }
            many => {
                let header = paths
                    .iter()
                    .find(|p| p.ends_with(".uasset") || p.ends_with(".umap"))
                    .cloned();
                let uexp = header
                    .as_ref()
                    .map(|h| format!("{}.uexp", &h[..h.rfind('.').expect("has extension")]));
                let names = || {
                    many.iter()
                        .map(|(_, m)| m.name.clone())
                        .collect::<Vec<_>>()
                        .join(" and ")
                };
                let mut merged: HashMap<String, Vec<u8>> = HashMap::new();
                if let (Some(h), Some(x)) = (&header, &uexp) {
                    let base_h = src.original(h)?;
                    let base_x = src.original(x)?;
                    if let (Some(bh), Some(bx)) = (base_h, base_x) {
                        let mut pairs = Vec::new();
                        for (id, m) in many {
                            let uasset = if has(m, h) {
                                src.mod_file(id, h)?
                            } else {
                                bh.clone()
                            };
                            let uexp = if has(m, x) {
                                src.mod_file(id, x)?
                            } else {
                                bx.clone()
                            };
                            pairs.push(playkit_merge::Files { uasset, uexp });
                        }
                        let out = playkit_merge::merge(
                            &playkit_merge::Files {
                                uasset: bh,
                                uexp: bx,
                            },
                            &pairs,
                        )
                        .map_err(|e| Error::Merge {
                            mods: names(),
                            reason: e.to_string(),
                        })?;
                        merged.insert(h.clone(), out.uasset);
                        merged.insert(x.clone(), out.uexp);
                    }
                }
                for p in paths {
                    if let Some(d) = merged.remove(&p) {
                        actions.push(Action::Write(p, d));
                        continue;
                    }
                    let mut versions = Vec::new();
                    for (id, m) in many {
                        if has(m, &p) {
                            versions.push(src.mod_file(id, &p)?);
                        }
                    }
                    match versions.first() {
                        None => actions.push(Action::Restore(p)),
                        Some(first) if versions.iter().all(|v| v == first) => {
                            actions.push(Action::Write(p, first.clone()))
                        }
                        Some(_) => {
                            return Err(Error::Merge {
                                mods: names(),
                                reason: format!("both replace {p}"),
                            })
                        }
                    }
                }
            }
        }
    }
    Ok(actions)
}

fn execute(game: &Game, state: &mut State, actions: Vec<Action>) -> Result<()> {
    let content = game.root.join(game::PROJECT);
    for a in actions {
        match a {
            Action::Write(rel, data) => {
                let target = game.resolve(&rel)?;
                if target.is_file()
                    && !state.written.contains_key(&rel)
                    && !state.backups.contains(&rel)
                {
                    fsx::copy_into(&target, &backup_path(game, &rel))?;
                    state.backups.insert(rel.clone());
                }
                fsx::write_atomic(&target, &data)?;
                state.written.insert(rel, fsx::sha256(&data));
            }
            Action::Restore(rel) => {
                let target = game.resolve(&rel)?;
                if state.backups.remove(&rel) {
                    let b = backup_path(game, &rel);
                    fsx::copy_into(&b, &target)?;
                    std::fs::remove_file(&b).map_err(|e| Error::io(&b, e))?;
                    fsx::remove_empty_parents(&b, &game.data_dir());
                } else if target.is_file() {
                    std::fs::remove_file(&target).map_err(|e| Error::io(&target, e))?;
                    fsx::remove_empty_parents(&target, &content);
                }
                state.written.remove(&rel);
            }
        }
        state.save(game)?;
    }
    Ok(())
}

async fn build(
    game: &Game,
    index: &Index,
    m: &Mod,
    progress: &impl Fn(usize, usize),
) -> Result<HashMap<String, Vec<u8>>> {
    let mut out = HashMap::new();
    let mut pak: Option<Pak> = None;
    for (i, f) in m.files.iter().enumerate() {
        progress(i, m.files.len());
        game.resolve(&f.path)?;
        let payload = index::download(&f.url).await?;
        let data = match &f.base_sha256 {
            None => payload,
            Some(base_hash) => {
                if pak.is_none() {
                    pak = Some(Pak::open(&game.pak(), &index.game.pak_key)?);
                }
                let base = pak.as_mut().expect("pak opened").read(&f.path)?;
                if fsx::sha256(&base) != base_hash.to_ascii_lowercase() {
                    return Err(Error::Invalid(format!(
                        "{} was built for another game version ({})",
                        m.name,
                        m.game_builds.join(", ")
                    )));
                }
                pak::apply_patch(&base, &payload, &f.path)?
            }
        };
        if fsx::sha256(&data) != f.sha256.to_ascii_lowercase() {
            return Err(Error::Checksum(f.path.clone()));
        }
        out.insert(f.path.clone(), data);
    }
    progress(m.files.len(), m.files.len());
    Ok(out)
}

pub async fn install(
    game: &Game,
    index: &Index,
    id: &str,
    progress: impl Fn(usize, usize),
) -> Result<()> {
    ensure_idle(game)?;
    let order = resolve(&index.mods, id)?;
    for dep in &order[..order.len() - 1] {
        let wanted = index
            .get(dep)
            .ok_or_else(|| Error::UnknownMod(dep.clone()))?;
        let state = State::load(game)?;
        let outdated = state
            .mods
            .get(dep)
            .is_none_or(|i| index::version_newer(&wanted.version, &i.version));
        if outdated {
            install_files(game, index, dep, true, &progress).await?;
        }
    }
    install_files(game, index, id, false, &progress).await
}

pub fn resolve(mods: &[Mod], id: &str) -> Result<Vec<String>> {
    fn visit(mods: &[Mod], id: &str, stack: &mut Vec<String>, out: &mut Vec<String>) -> Result<()> {
        if out.iter().any(|x| x == id) {
            return Ok(());
        }
        if stack.iter().any(|x| x == id) {
            return Err(Error::Invalid(format!(
                "{id} depends on itself through {}",
                stack.join(" → ")
            )));
        }
        let m = mods
            .iter()
            .find(|m| m.id == id)
            .ok_or_else(|| Error::UnknownMod(id.into()))?;
        stack.push(id.to_string());
        for r in &m.requires {
            visit(mods, r, stack, out)?;
        }
        stack.pop();
        out.push(id.to_string());
        Ok(())
    }
    let mut out = Vec::new();
    visit(mods, id, &mut Vec::new(), &mut out)?;
    Ok(out)
}

async fn install_files(
    game: &Game,
    index: &Index,
    id: &str,
    auto: bool,
    progress: &impl Fn(usize, usize),
) -> Result<()> {
    if !crate::package::valid_id(id) {
        return Err(Error::Invalid(format!("invalid mod id {id:?}")));
    }
    let m = index
        .get(id)
        .ok_or_else(|| Error::UnknownMod(id.into()))?
        .clone();
    for f in &m.files {
        crate::package::check_path(&f.path)?;
    }
    let fresh = build(game, index, &m, progress).await?;
    let mut state = State::load(game)?;
    let previous: Vec<String> = state
        .mods
        .get(id)
        .map(|i| i.files.iter().map(|f| f.path.clone()).collect())
        .unwrap_or_default();
    let seq = match state.mods.get(id) {
        Some(i) => i.seq,
        None => {
            state.next_seq += 1;
            state.next_seq
        }
    };
    let auto = state.mods.get(id).map_or(auto, |i| i.auto && auto);
    let mut after = state.clone();
    after.mods.insert(
        id.to_string(),
        Installed {
            name: m.name.clone(),
            version: m.version.clone(),
            seq,
            requires: m.requires.clone(),
            auto,
            files: m
                .files
                .iter()
                .map(|f| InstalledFile {
                    path: f.path.clone(),
                    sha256: f.sha256.to_ascii_lowercase(),
                })
                .collect(),
        },
    );
    let stems: BTreeSet<String> = previous
        .iter()
        .chain(m.files.iter().map(|f| &f.path))
        .map(|p| stem(p))
        .collect();
    let mut src = Sources {
        game,
        fresh: &fresh,
        fresh_id: id,
        pak: None,
        pak_key: &index.game.pak_key,
    };
    let actions = plan(&after, &stems, &mut src)?;
    let dir = game.data_dir().join("mods").join(id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
    }
    for (rel, data) in &fresh {
        fsx::write_atomic(&cache_path(game, id, rel), data)?;
    }
    after.save(game)?;
    execute(game, &mut after, actions)
}

pub fn uninstall(game: &Game, pak_key: &str, id: &str) -> Result<()> {
    ensure_idle(game)?;
    let state = State::load(game)?;
    let needed_by: Vec<String> = state
        .mods
        .iter()
        .filter(|(other, m)| *other != id && m.requires.iter().any(|r| r == id))
        .map(|(_, m)| m.name.clone())
        .collect();
    if !needed_by.is_empty() {
        let name = state.mods.get(id).map_or(id, |m| m.name.as_str());
        return Err(Error::Invalid(format!(
            "{name} is needed by {}",
            needed_by.join(", ")
        )));
    }
    uninstall_files(game, pak_key, id)?;
    loop {
        let state = State::load(game)?;
        let orphan = state.mods.iter().find(|(dep, m)| {
            m.auto
                && !state
                    .mods
                    .values()
                    .any(|o| o.requires.iter().any(|r| r == *dep))
        });
        match orphan {
            Some((dep, _)) => uninstall_files(game, pak_key, &dep.clone())?,
            None => return Ok(()),
        }
    }
}

fn uninstall_files(game: &Game, pak_key: &str, id: &str) -> Result<()> {
    if !crate::package::valid_id(id) {
        return Err(Error::Invalid(format!("invalid mod id {id:?}")));
    }
    let mut state = State::load(game)?;
    let Some(m) = state.mods.remove(id) else {
        return Err(Error::UnknownMod(id.into()));
    };
    let stems: BTreeSet<String> = m.files.iter().map(|f| stem(&f.path)).collect();
    let empty = HashMap::new();
    let mut src = Sources {
        game,
        fresh: &empty,
        fresh_id: "",
        pak: None,
        pak_key,
    };
    let actions = plan(&state, &stems, &mut src)?;
    execute(game, &mut state, actions)?;
    let dir = game.data_dir().join("mods").join(id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
    }
    state.save(game)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{GameInfo, ModFile, Network};

    fn fake_game(dir: &std::path::Path) -> Game {
        let bin = dir.join("Playtime_Multiplayer/Binaries/Win64");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join(format!("{}.exe", game::EXE)), b"exe").unwrap();
        Game::from_dir(dir).unwrap()
    }

    fn demo_mod(id: &str, file: &std::path::Path, data: &[u8], rel: &str) -> Mod {
        std::fs::write(file, data).unwrap();
        Mod {
            id: id.into(),
            name: id.into(),
            version: "1.0".into(),
            authors: vec!["me".into()],
            summary: String::new(),
            description: String::new(),
            category: "fixes".into(),
            network: Network::Safe,
            game_builds: vec!["12576441".into()],
            color: None,
            homepage: None,
            changelog: vec![],
            requires: vec![],
            files: vec![ModFile {
                path: rel.into(),
                url: format!("file://{}", file.display()),
                sha256: fsx::sha256(data),
                size: data.len() as u64,
                base_sha256: None,
            }],
        }
    }

    fn index(mods: Vec<Mod>) -> Index {
        Index {
            schema: 1,
            game: GameInfo {
                builds: vec![],
                pak_key: String::new(),
            },
            mods,
        }
    }

    #[tokio::test]
    async fn install_backs_up_and_uninstall_restores() {
        let dir = tempfile::tempdir().unwrap();
        let game = fake_game(dir.path());
        let rel = "Playtime_Multiplayer/Content/Interface/UI_Test.txt";
        let target = dir.path().join(rel);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, b"users own loose mod").unwrap();
        let idx = index(vec![demo_mod(
            "demo",
            &dir.path().join("p.bin"),
            b"modded",
            rel,
        )]);

        install_files(&game, &idx, "demo", false, &|_, _| {})
            .await
            .unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"modded");
        assert!(matches!(
            State::load(&game).unwrap().health(&game, "demo"),
            Health::Ok
        ));

        uninstall_files(&game, "", "demo").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"users own loose mod");
        assert!(State::load(&game).unwrap().mods.is_empty());
    }

    #[tokio::test]
    async fn identical_files_share_and_different_ones_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let game = fake_game(dir.path());
        let rel = "Playtime_Multiplayer/Content/Data/Shared.txt";
        let idx = index(vec![
            demo_mod("a", &dir.path().join("a.bin"), b"same", rel),
            demo_mod("b", &dir.path().join("b.bin"), b"same", rel),
            demo_mod("c", &dir.path().join("c.bin"), b"other", rel),
        ]);
        install_files(&game, &idx, "a", false, &|_, _| {})
            .await
            .unwrap();
        install_files(&game, &idx, "b", false, &|_, _| {})
            .await
            .unwrap();
        assert!(install_files(&game, &idx, "c", false, &|_, _| {})
            .await
            .is_err());
        assert!(!State::load(&game).unwrap().mods.contains_key("c"));
        uninstall_files(&game, "", "a").unwrap();
        assert_eq!(std::fs::read(dir.path().join(rel)).unwrap(), b"same");
        uninstall_files(&game, "", "b").unwrap();
        assert!(!dir.path().join(rel).exists());
    }

    #[tokio::test]
    async fn rejects_paths_outside_the_game() {
        let dir = tempfile::tempdir().unwrap();
        let game = fake_game(dir.path());
        let idx = index(vec![demo_mod(
            "demo",
            &dir.path().join("p.bin"),
            b"x",
            "Playtime_Multiplayer/../../evil.txt",
        )]);
        assert!(install_files(&game, &idx, "demo", false, &|_, _| {})
            .await
            .is_err());
        assert!(!dir.path().parent().unwrap().join("evil.txt").exists());
    }

    #[tokio::test]
    async fn dependencies_come_and_go_automatically() {
        let dir = tempfile::tempdir().unwrap();
        let game = fake_game(dir.path());
        let mut lib = demo_mod(
            "lib",
            &dir.path().join("l.bin"),
            b"lib",
            "Playtime_Multiplayer/Content/Lib.txt",
        );
        lib.category = "library".into();
        let mut app = demo_mod(
            "app",
            &dir.path().join("a.bin"),
            b"app",
            "Playtime_Multiplayer/Content/App.txt",
        );
        app.requires = vec!["lib".into()];
        let idx = index(vec![lib, app]);
        install(&game, &idx, "app", |_, _| {}).await.unwrap();
        let state = State::load(&game).unwrap();
        assert!(state.mods["lib"].auto && !state.mods["app"].auto);
        assert!(uninstall(&game, "", "lib").is_err());
        uninstall(&game, "", "app").unwrap();
        assert!(State::load(&game).unwrap().mods.is_empty());
        assert!(!dir
            .path()
            .join("Playtime_Multiplayer/Content/Lib.txt")
            .exists());
    }

    #[test]
    fn dependency_cycles_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = demo_mod(
            "a",
            &dir.path().join("a.bin"),
            b"a",
            "Playtime_Multiplayer/Content/A.txt",
        );
        let mut b = demo_mod(
            "b",
            &dir.path().join("b.bin"),
            b"b",
            "Playtime_Multiplayer/Content/B.txt",
        );
        a.requires = vec!["b".into()];
        b.requires = vec!["a".into()];
        assert!(resolve(&[a, b], "a").is_err());
    }

    #[test]
    fn hostile_mod_lists_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let traversal = demo_mod(
            "../../escape",
            &dir.path().join("a.bin"),
            b"a",
            "Playtime_Multiplayer/Content/A.txt",
        );
        assert!(index(vec![traversal]).validate().is_err());
        let binary = demo_mod(
            "dll",
            &dir.path().join("b.bin"),
            b"b",
            "Playtime_Multiplayer/Binaries/Win64/dxgi.dll",
        );
        assert!(index(vec![binary]).validate().is_err());
        let mut needs = demo_mod(
            "needs",
            &dir.path().join("c.bin"),
            b"c",
            "Playtime_Multiplayer/Content/C.txt",
        );
        needs.requires = vec!["../up".into()];
        assert!(index(vec![needs]).validate().is_err());
        let ok = demo_mod(
            "fine",
            &dir.path().join("d.bin"),
            b"d",
            "Playtime_Multiplayer/Content/D.txt",
        );
        assert!(index(vec![ok]).validate().is_ok());
    }

    #[tokio::test]
    async fn install_refuses_paths_outside_content() {
        let dir = tempfile::tempdir().unwrap();
        let game = fake_game(dir.path());
        let idx = index(vec![demo_mod(
            "dll",
            &dir.path().join("p.bin"),
            b"MZ",
            "Playtime_Multiplayer/Binaries/Win64/dxgi.dll",
        )]);
        assert!(install_files(&game, &idx, "dll", false, &|_, _| {})
            .await
            .is_err());
        assert!(!dir
            .path()
            .join("Playtime_Multiplayer/Binaries/Win64/dxgi.dll")
            .exists());
    }
}
