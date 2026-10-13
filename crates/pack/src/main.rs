use clap::{Parser, Subcommand};
use playkit_core::{game::Game, package};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(about = "Pack Project Playtime mods for the Playkit registry")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Turn a mod folder (mod.toml + Playtime_Multiplayer/...) into patches inside the registry
    Pack {
        src: PathBuf,
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        game: Option<PathBuf>,
        #[arg(long, env = "PLAYKIT_PAK_KEY")]
        key: Option<String>,
    },
    /// Validate every mod and write index.json
    Index {
        repo: PathBuf,
        #[arg(long)]
        base_url: String,
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

fn run() -> Result<(), String> {
    match Cli::parse().cmd {
        Cmd::Pack {
            src,
            repo,
            game,
            key,
        } => {
            let game = match game {
                Some(dir) => Game::from_dir(&dir),
                None => Game::detect(),
            }
            .map_err(|e| e.to_string())?;
            let key = match key {
                Some(k) => k,
                None => {
                    let reg = std::fs::read_to_string(repo.join("registry.toml"))
                        .map_err(|e| e.to_string())?;
                    let reg: package::Registry = toml_from(&reg)?;
                    reg.game.pak_key
                }
            };
            let r = package::pack(&src, &game.pak(), &key, &repo).map_err(|e| e.to_string())?;
            for f in &r.files {
                let kind = if f.base_sha256.is_some() {
                    "patch"
                } else {
                    "file "
                };
                println!("{kind} {} ({} bytes)", f.path, f.size);
            }
            for s in &r.skipped_unchanged {
                println!("same  {s} (unchanged, left out)");
            }
            println!("packed {} into {}", r.id, r.out.display());
        }
        Cmd::Index {
            repo,
            base_url,
            out,
        } => {
            let idx = package::build_index(&repo, &base_url).map_err(|e| e.to_string())?;
            let json = serde_json::to_string_pretty(&idx).map_err(|e| e.to_string())?;
            let out = out.unwrap_or_else(|| repo.join("index.json"));
            std::fs::write(&out, json + "\n").map_err(|e| e.to_string())?;
            println!("{} mods -> {}", idx.mods.len(), out.display());
        }
    }
    Ok(())
}

fn toml_from<T: serde::de::DeserializeOwned>(s: &str) -> Result<T, String> {
    playkit_core::package::parse_toml(s).map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
