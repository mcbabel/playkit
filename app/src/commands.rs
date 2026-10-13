use crate::config::Config;
use playkit_core::game::Game;
use playkit_core::index::{self, Index, Mod};
use playkit_core::install::{self, Health, State};
use playkit_core::loader::{self, Bundled};
use playkit_core::platform::{self, Platform};
use playkit_core::{anticheat, package, steam, APP_ID};
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State as Tauri};
use tauri_plugin_opener::OpenerExt;

const LOADER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/dxgi.dll"));
const REGISTRY: &str = "https://github.com/MCbabel/playkit-mods";

fn bundled() -> Bundled<'static> {
    Bundled { bytes: LOADER }
}

type Res<T> = Result<T, String>;

fn err(e: impl ToString) -> String {
    e.to_string()
}

pub struct AppState {
    config: Mutex<Config>,
    index: tokio::sync::Mutex<Option<Index>>,
    busy: tokio::sync::Mutex<()>,
    adopted: Mutex<bool>,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        AppState {
            config: Mutex::new(config),
            index: tokio::sync::Mutex::new(None),
            busy: tokio::sync::Mutex::new(()),
            adopted: Mutex::new(false),
        }
    }

    fn game(&self) -> Res<Game> {
        let dir = self.config.lock().map_err(err)?.game_dir.clone();
        match dir {
            Some(d) => Game::from_dir(&d).map_err(err),
            None => Game::detect().map_err(err),
        }
    }

    fn index_url(&self) -> String {
        self.config
            .lock()
            .ok()
            .and_then(|c| c.index_url.clone())
            .unwrap_or_else(|| index::DEFAULT_URL.to_string())
    }

    async fn index(&self, refresh: bool) -> Res<Index> {
        let mut cached = self.index.lock().await;
        if refresh || cached.is_none() {
            *cached = Some(index::fetch(&self.index_url()).await.map_err(err)?);
        }
        Ok(cached.clone().expect("index loaded"))
    }
}

#[derive(Serialize)]
pub struct GameView {
    root: PathBuf,
    build_id: Option<String>,
    build_name: Option<String>,
    custom_dir: bool,
}

#[derive(Serialize)]
pub struct Status {
    app_version: &'static str,
    platform: Platform,
    game: Option<GameView>,
    game_error: Option<String>,
    loader: Option<loader::Status>,
    ready: bool,
    steam_running: bool,
    game_running: bool,
    index_url: String,
}

#[tauri::command]
pub async fn status(state: Tauri<'_, AppState>) -> Res<Status> {
    let platform = platform::detect();
    let game = state.game();
    let build_name = match (&game, state.index.lock().await.as_ref()) {
        (Ok(g), Some(idx)) => g
            .build_id
            .as_deref()
            .and_then(|b| idx.build_name(b))
            .map(str::to_string),
        _ => None,
    };
    let custom_dir = state.config.lock().map_err(err)?.game_dir.is_some();
    let loader = game
        .as_ref()
        .ok()
        .map(|g| loader::status(g, &platform, &bundled()));
    Ok(Status {
        app_version: env!("CARGO_PKG_VERSION"),
        ready: loader.as_ref().is_some_and(|l| l.ready()),
        steam_running: steam::is_running(),
        game_running: game.as_ref().is_ok_and(|g| g.is_running()),
        game: game.as_ref().ok().map(|g| GameView {
            root: g.root.clone(),
            build_id: g.build_id.clone(),
            build_name,
            custom_dir,
        }),
        game_error: game.err(),
        loader,
        platform,
        index_url: state.index_url(),
    })
}

#[derive(Serialize)]
pub struct ModView {
    #[serde(flatten)]
    info: Mod,
    installed: Option<String>,
    update: bool,
    compatible: bool,
    broken: Vec<String>,
    shares_with: Vec<String>,
    needs: Vec<String>,
    needed_by: Vec<String>,
    auto: bool,
    disabled: bool,
}

#[derive(Serialize)]
pub struct ModList {
    mods: Vec<ModView>,
    local_only: Vec<LocalMod>,
}

#[derive(Serialize)]
pub struct LocalMod {
    id: String,
    name: String,
    version: String,
}

#[tauri::command]
pub async fn mods(state: Tauri<'_, AppState>, refresh: bool) -> Res<ModList> {
    let idx = state.index(refresh).await?;
    let game = state.game().ok();
    if let Some(g) = &game {
        let first = !std::mem::replace(&mut *state.adopted.lock().map_err(err)?, true);
        if refresh || first {
            let _busy = state.busy.lock().await;
            let _ = install::adopt(g, &idx).await;
        }
    }
    let installed = game
        .as_ref()
        .map(|g| State::load(g).unwrap_or_default())
        .unwrap_or_default();
    let build = game.as_ref().and_then(|g| g.build_id.clone());
    let mods = idx
        .mods
        .iter()
        .map(|m| {
            let mine = installed.mods.get(&m.id);
            let broken = match (&game, mine) {
                (Some(g), Some(_)) => match installed.health(g, &m.id) {
                    Health::Ok => vec![],
                    Health::Changed { files } => files,
                },
                _ => vec![],
            };
            let paths: Vec<String> = m.files.iter().map(|f| f.path.clone()).collect();
            let shares_with = installed
                .sharing(&m.id, &paths)
                .iter()
                .map(|other| {
                    installed
                        .mods
                        .get(other)
                        .map_or(other.clone(), |o| o.name.clone())
                })
                .collect();
            ModView {
                installed: mine.map(|i| i.version.clone()),
                update: mine.is_some_and(|i| index::version_newer(&m.version, &i.version)),
                compatible: build.as_ref().is_none_or(|b| m.game_builds.contains(b)),
                broken,
                shares_with,
                needs: m
                    .requires
                    .iter()
                    .map(|r| idx.get(r).map_or(r.clone(), |x| x.name.clone()))
                    .collect(),
                needed_by: installed
                    .mods
                    .values()
                    .filter(|o| o.requires.iter().any(|r| r == &m.id))
                    .map(|o| o.name.clone())
                    .collect(),
                auto: mine.is_some_and(|i| i.auto),
                disabled: mine.is_some_and(|i| i.disabled),
                info: m.clone(),
            }
        })
        .collect();
    let local_only = installed
        .mods
        .iter()
        .filter(|(id, _)| idx.get(id).is_none())
        .map(|(id, m)| LocalMod {
            id: id.clone(),
            name: m.name.clone(),
            version: m.version.clone(),
        })
        .collect();
    Ok(ModList { mods, local_only })
}

#[derive(Clone, Serialize)]
struct Progress<'a> {
    id: &'a str,
    done: usize,
    total: usize,
}

async fn install_one(app: &AppHandle, state: &AppState, idx: &Index, id: &str) -> Res<()> {
    let game = state.game()?;
    install::install(&game, idx, id, |done, total| {
        let _ = app.emit("progress", Progress { id, done, total });
    })
    .await
    .map_err(err)
}

#[tauri::command]
pub async fn install(app: AppHandle, state: Tauri<'_, AppState>, id: String) -> Res<()> {
    let _busy = state.busy.lock().await;
    let idx = state.index(false).await?;
    install_one(&app, &state, &idx, &id).await
}

#[tauri::command]
pub async fn uninstall(state: Tauri<'_, AppState>, id: String) -> Res<()> {
    let _busy = state.busy.lock().await;
    let key = state
        .index(false)
        .await
        .map(|i| i.game.pak_key)
        .unwrap_or_default();
    install::uninstall(&state.game()?, &key, &id).map_err(err)
}

#[tauri::command]
pub async fn set_enabled(state: Tauri<'_, AppState>, id: String, enabled: bool) -> Res<()> {
    let _busy = state.busy.lock().await;
    let key = state
        .index(false)
        .await
        .map(|i| i.game.pak_key)
        .unwrap_or_default();
    install::set_enabled(&state.game()?, &key, &id, enabled).map_err(err)
}

#[tauri::command]
pub async fn update_all(app: AppHandle, state: Tauri<'_, AppState>) -> Res<Vec<String>> {
    let _busy = state.busy.lock().await;
    let idx = state.index(true).await?;
    let game = state.game()?;
    let installed = State::load(&game).map_err(err)?;
    let mut done = Vec::new();
    for m in &idx.mods {
        if installed
            .mods
            .get(&m.id)
            .is_some_and(|i| index::version_newer(&m.version, &i.version))
        {
            install_one(&app, &state, &idx, &m.id).await?;
            done.push(m.name.clone());
        }
    }
    Ok(done)
}

#[derive(Serialize)]
pub struct SetupResult {
    launch_option_error: Option<String>,
}

#[tauri::command]
pub async fn setup(state: Tauri<'_, AppState>) -> Res<SetupResult> {
    let game = state.game()?;
    loader::install_dll(&game, &bundled()).map_err(err)?;
    anticheat::patch(&game).map_err(err)?;
    let launch_option_error = match platform::detect().os {
        platform::Os::Linux => loader::apply_launch_option(&game).err().map(err),
        _ => None,
    };
    Ok(SetupResult {
        launch_option_error,
    })
}

#[tauri::command]
pub async fn remove_loader(state: Tauri<'_, AppState>) -> Res<()> {
    let game = state.game()?;
    loader::remove_dll(&game).map_err(err)?;
    anticheat::restore(&game).map_err(err)
}

#[tauri::command]
pub async fn play(app: AppHandle) -> Res<()> {
    app.opener()
        .open_url(steam::run_url(APP_ID), None::<&str>)
        .map_err(err)
}

#[tauri::command]
pub async fn open_url(app: AppHandle, url: String) -> Res<()> {
    if !url.starts_with("https://") {
        return Err("only https links".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(err)
}

#[tauri::command]
pub async fn reveal(app: AppHandle, path: PathBuf) -> Res<()> {
    app.opener().reveal_item_in_dir(path).map_err(err)
}

#[tauri::command]
pub async fn set_game_dir(state: Tauri<'_, AppState>, path: Option<PathBuf>) -> Res<()> {
    if let Some(p) = &path {
        Game::from_dir(p).map_err(err)?;
    }
    let mut c = state.config.lock().map_err(err)?;
    c.game_dir = path;
    c.save()
}

#[tauri::command]
pub async fn set_index_url(state: Tauri<'_, AppState>, url: Option<String>) -> Res<()> {
    {
        let mut c = state.config.lock().map_err(err)?;
        c.index_url = url.filter(|u| !u.trim().is_empty());
        c.save()?;
    }
    *state.index.lock().await = None;
    Ok(())
}

#[derive(Serialize)]
pub struct PackResult {
    id: String,
    files: Vec<package::PackedFile>,
    unchanged: Vec<String>,
    zip: PathBuf,
    manifest: String,
    pull_request_url: String,
    issue_url: String,
}

#[tauri::command]
pub async fn pack(state: Tauri<'_, AppState>, src: PathBuf) -> Res<PackResult> {
    let game = state.game()?;
    let idx = state.index(false).await?;
    let work = Config::data_dir().join("submissions");
    let manifest = std::fs::read_to_string(src.join("mod.toml"))
        .map_err(|_| "mod.toml is missing in that folder".to_string())?;
    let staging = work.join("staging");
    let _ = std::fs::remove_dir_all(&staging);
    let report = package::pack(&src, &game.pak(), &idx.game.pak_key, &staging).map_err(err)?;
    let zip = work.join(format!("{}.zip", report.id));
    zip_dir(&report.out, &zip, &format!("mods/{}", report.id))?;
    Ok(PackResult {
        pull_request_url: format!("{REGISTRY}/blob/main/CONTRIBUTING.md"),
        issue_url: format!(
            "{REGISTRY}/issues/new?template=submit-mod.yml&title=Add+mod:+{}",
            report.id
        ),
        id: report.id,
        files: report.files,
        unchanged: report.skipped_unchanged,
        zip,
        manifest,
    })
}

fn zip_dir(dir: &Path, out: &Path, prefix: &str) -> Res<()> {
    let file = std::fs::File::create(out).map_err(err)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).map_err(err)?.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let rel = p
                .strip_prefix(dir)
                .map_err(err)?
                .to_string_lossy()
                .replace('\\', "/");
            zip.start_file(format!("{prefix}/{rel}"), opts)
                .map_err(err)?;
            zip.write_all(&std::fs::read(&p).map_err(err)?)
                .map_err(err)?;
        }
    }
    zip.finish().map_err(err)?;
    Ok(())
}
