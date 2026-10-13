use crate::game::Game;
use crate::{fsx, Error, Result};
use serde::Serialize;
use std::path::PathBuf;

const KEYS: [&str; 3] = ["productid", "sandboxid", "deploymentid"];
const VALUE: &str = "123";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AntiCheat {
    Absent,
    Original,
    Patched,
}

fn settings(game: &Game) -> PathBuf {
    game.root.join("EasyAntiCheat").join("Settings.json")
}

fn backup(game: &Game) -> PathBuf {
    game.data_dir().join("backup/anticheat/Settings.json")
}

fn rewrite_line(line: &str) -> Option<String> {
    let t = line.trim_start();
    let key = KEYS.iter().find(|k| t.starts_with(&format!("\"{k}\"")))?;
    let after = line.len() - t.len() + key.len() + 2;
    let colon = after + line[after..].find(':')?;
    let open = colon + line[colon..].find('"')?;
    let close = open + 1 + line[open + 1..].find('"')?;
    Some(format!("{}{VALUE}{}", &line[..=open], &line[close..]))
}

pub fn rewrite(text: &str) -> String {
    text.split_inclusive('\n')
        .map(|l| rewrite_line(l).unwrap_or_else(|| l.to_string()))
        .collect()
}

pub fn status(game: &Game) -> AntiCheat {
    match std::fs::read_to_string(settings(game)) {
        Err(_) => AntiCheat::Absent,
        Ok(t) if rewrite(&t) == t => AntiCheat::Patched,
        Ok(_) => AntiCheat::Original,
    }
}

pub fn patch(game: &Game) -> Result<()> {
    let path = settings(game);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    let new = rewrite(&text);
    if new == text {
        return Ok(());
    }
    if game.is_running() {
        return Err(Error::GameRunning);
    }
    if !backup(game).exists() {
        fsx::copy_into(&path, &backup(game))?;
    }
    fsx::write_atomic(&path, new.as_bytes())
}

pub fn restore(game: &Game) -> Result<()> {
    let b = backup(game);
    if !b.is_file() {
        return Ok(());
    }
    if game.is_running() {
        return Err(Error::GameRunning);
    }
    fsx::copy_into(&b, &settings(game))?;
    std::fs::remove_file(&b).map_err(|e| Error::io(&b, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_only_ids_and_keeps_layout() {
        let src = "{\r\n\t\"title\"\t\t: \"ProjectPlaytime\",\r\n\t\"productid\"\t\t\t: \"a1b2\",\r\n\t\"sandboxid\" : \"c3\",\r\n\t\"deploymentid\":\"d4\"\r\n}\r\n";
        let out = rewrite(src);
        assert_eq!(
            out,
            "{\r\n\t\"title\"\t\t: \"ProjectPlaytime\",\r\n\t\"productid\"\t\t\t: \"123\",\r\n\t\"sandboxid\" : \"123\",\r\n\t\"deploymentid\":\"123\"\r\n}\r\n"
        );
        assert_eq!(rewrite(&out), out);
    }
}
