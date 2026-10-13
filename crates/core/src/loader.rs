use crate::game::Game;
use crate::platform::{Os, Platform};
use crate::{fsx, steam, Error, Result, APP_ID};
use serde::Serialize;

pub const DLL: &str = "dxgi.dll";
const OVERRIDE: &str = "dxgi=n,b";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum LaunchOption {
    NotNeeded,
    Ok,
    Missing { current: String, suggested: String },
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DllState {
    Missing,
    Current,
    Outdated,
    Foreign,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub dll: DllState,
    pub launch_option: LaunchOption,
}

impl Status {
    pub fn ready(&self) -> bool {
        matches!(self.dll, DllState::Current | DllState::Foreign)
            && matches!(
                self.launch_option,
                LaunchOption::Ok | LaunchOption::NotNeeded
            )
    }
}

pub struct Bundled<'a> {
    pub bytes: &'a [u8],
}

const MARKER: &[u8] = b"playkit loader ";

pub fn loader_version(dll: &[u8]) -> Option<semver::Version> {
    let start = dll.windows(MARKER.len()).position(|w| w == MARKER)? + MARKER.len();
    let rest = &dll[start..dll.len().min(start + 32)];
    let end = rest
        .iter()
        .position(|c| !(c.is_ascii_alphanumeric() || *c == b'.' || *c == b'-'))
        .unwrap_or(rest.len());
    semver::Version::parse(std::str::from_utf8(&rest[..end]).ok()?).ok()
}

pub fn status(game: &Game, platform: &Platform, bundled: &Bundled) -> Status {
    let path = game.binaries().join(DLL);
    let ours = loader_version(bundled.bytes);
    let dll = match std::fs::read(&path) {
        Err(_) => DllState::Missing,
        Ok(d) => match (loader_version(&d), &ours) {
            (Some(v), Some(o)) if v < *o => DllState::Outdated,
            (Some(_), _) => DllState::Current,
            (None, _) => DllState::Foreign,
        },
    };
    Status {
        dll,
        launch_option: launch_option_state(game, platform),
    }
}

fn launch_option_state(game: &Game, platform: &Platform) -> LaunchOption {
    if platform.os != Os::Linux {
        return LaunchOption::NotNeeded;
    }
    let Some(s) = &game.steam else {
        return LaunchOption::Unknown;
    };
    let Some(acc) = s.active_account() else {
        return LaunchOption::Unknown;
    };
    match s.launch_options(acc, APP_ID) {
        Ok(cur) if has_override(&cur) => LaunchOption::Ok,
        Ok(cur) => LaunchOption::Missing {
            suggested: merge_launch_options(&cur),
            current: cur,
        },
        Err(_) => LaunchOption::Unknown,
    }
}

fn backup(game: &Game) -> std::path::PathBuf {
    game.data_dir().join("backup/loader").join(DLL)
}

pub fn install_dll(game: &Game, bundled: &Bundled) -> Result<()> {
    if game.is_running() {
        return Err(Error::GameRunning);
    }
    let path = game.binaries().join(DLL);
    if let Ok(current) = std::fs::read(&path) {
        match (loader_version(&current), loader_version(bundled.bytes)) {
            (Some(v), Some(o)) if v >= o => return Ok(()),
            (Some(_), _) => {}
            (None, _) if !backup(game).exists() => fsx::copy_into(&path, &backup(game))?,
            (None, _) => {}
        }
    }
    fsx::write_atomic(&path, bundled.bytes)
}

pub fn remove_dll(game: &Game) -> Result<()> {
    if game.is_running() {
        return Err(Error::GameRunning);
    }
    let path = game.binaries().join(DLL);
    let b = backup(game);
    if b.is_file() {
        fsx::copy_into(&b, &path)?;
        std::fs::remove_file(&b).map_err(|e| Error::io(&b, e))?;
    } else if path.is_file() {
        std::fs::remove_file(&path).map_err(|e| Error::io(&path, e))?;
    }
    Ok(())
}

pub fn apply_launch_option(game: &Game) -> Result<()> {
    let s = game.steam.as_ref().ok_or(Error::GameNotFound)?;
    let acc = s
        .active_account()
        .ok_or_else(|| Error::Invalid("no Steam account found".into()))?;
    let cur = s.launch_options(acc, APP_ID)?;
    if has_override(&cur) {
        return Ok(());
    }
    if steam::is_running() {
        return Err(Error::SteamRunning);
    }
    s.set_launch_options(acc, APP_ID, &merge_launch_options(&cur))
}

pub fn has_override(opts: &str) -> bool {
    overrides_value(opts).is_some_and(|v| {
        v.split(';').any(|entry| {
            let (dlls, mode) = entry.split_once('=').unwrap_or((entry, ""));
            dlls.split(',')
                .any(|d| d.trim().eq_ignore_ascii_case("dxgi"))
                && mode.trim_start().starts_with('n')
        })
    })
}

fn overrides_value(opts: &str) -> Option<String> {
    let start = opts.find("WINEDLLOVERRIDES=")? + "WINEDLLOVERRIDES=".len();
    let rest = &opts[start..];
    if let Some(q) = rest.strip_prefix('"') {
        Some(q.split('"').next().unwrap_or_default().to_string())
    } else {
        Some(
            rest.split_whitespace()
                .next()
                .unwrap_or_default()
                .to_string(),
        )
    }
}

pub fn merge_launch_options(cur: &str) -> String {
    let cur = cur.trim();
    if has_override(cur) {
        return cur.to_string();
    }
    if let Some(v) = overrides_value(cur) {
        let old = if cur.contains("WINEDLLOVERRIDES=\"") {
            format!("WINEDLLOVERRIDES=\"{v}\"")
        } else {
            format!("WINEDLLOVERRIDES={v}")
        };
        let joined = if v.is_empty() {
            OVERRIDE.to_string()
        } else {
            format!("{OVERRIDE};{v}")
        };
        return cur.replacen(&old, &format!("WINEDLLOVERRIDES=\"{joined}\""), 1);
    }
    let var = format!("WINEDLLOVERRIDES=\"{OVERRIDE}\"");
    if cur.is_empty() {
        format!("{var} %command%")
    } else if cur.contains("%command%") {
        format!("{var} {cur}")
    } else {
        format!("{var} %command% {cur}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_any_build_of_the_loader() {
        let mut dll = b"MZ\0\0 some code playkit loader 0.2.1\n more".to_vec();
        assert_eq!(loader_version(&dll), Some(semver::Version::new(0, 2, 1)));
        dll.truncate(10);
        assert_eq!(loader_version(&dll), None);
        assert_eq!(
            loader_version(b"playkit loader 1.0.0"),
            Some(semver::Version::new(1, 0, 0))
        );
    }

    #[test]
    fn detects_existing_override() {
        assert!(has_override(r#"WINEDLLOVERRIDES="dxgi=n,b" %command%"#));
        assert!(has_override(
            r#"WINEDLLOVERRIDES="dxgi,dwmapi=n,b" UMU_NO_RUNTIME=1 "/x/launch.sh" %command%"#
        ));
        assert!(has_override("WINEDLLOVERRIDES=d3d11=b;dxgi=n %command%"));
        assert!(!has_override(r#"WINEDLLOVERRIDES="d3d11=n,b" %command%"#));
        assert!(!has_override("%command% -dx12"));
    }

    #[test]
    fn merges_without_losing_user_options() {
        assert_eq!(
            merge_launch_options(""),
            r#"WINEDLLOVERRIDES="dxgi=n,b" %command%"#
        );
        assert_eq!(
            merge_launch_options("%command% -dx12"),
            r#"WINEDLLOVERRIDES="dxgi=n,b" %command% -dx12"#
        );
        assert_eq!(
            merge_launch_options("-dx12"),
            r#"WINEDLLOVERRIDES="dxgi=n,b" %command% -dx12"#
        );
        assert_eq!(
            merge_launch_options(r#"WINEDLLOVERRIDES="d3d11=n,b" gamemoderun %command%"#),
            r#"WINEDLLOVERRIDES="dxgi=n,b;d3d11=n,b" gamemoderun %command%"#
        );
        let mine = r#"WINEDLLOVERRIDES="dxgi,dwmapi=n,b" UMU_NO_RUNTIME=1 %command%"#;
        assert_eq!(merge_launch_options(mine), mine);
    }
}
