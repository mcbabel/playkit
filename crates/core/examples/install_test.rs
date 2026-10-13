use playkit_core::{fsx, game::Game, index, install};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (index_url, real_game, scratch) = (&args[1], &args[2], std::path::PathBuf::from(&args[3]));
    let bin = scratch.join("Playtime_Multiplayer/Binaries/Win64");
    let paks = scratch.join("Playtime_Multiplayer/Content/Paks");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&paks).unwrap();
    std::fs::write(bin.join("Playtime_Multiplayer-Win64-Shipping.exe"), b"").unwrap();
    let pak = "Playtime_Multiplayer-WindowsNoEditor.pak";
    #[cfg(unix)]
    let _ = std::os::unix::fs::symlink(
        std::path::Path::new(real_game)
            .join("Playtime_Multiplayer/Content/Paks")
            .join(pak),
        paks.join(pak),
    );
    let game = Game::from_dir(&scratch).unwrap();
    let idx = index::fetch(index_url).await.unwrap();
    for m in &idx.mods {
        install::install(&game, &idx, &m.id, |_, _| {})
            .await
            .map_err(|e| e.to_string())
            .unwrap();
        let state = install::State::load(&game).unwrap();
        println!(
            "installed {} -> health {:?}, sharing with {:?}",
            m.id,
            state.health(&game, &m.id),
            state.sharing(
                &m.id,
                &m.files.iter().map(|f| f.path.clone()).collect::<Vec<_>>()
            )
        );
    }
    let state = install::State::load(&game).unwrap();
    for (p, sha) in &state.written {
        println!("written {p} {}", &sha[..12]);
    }
    if let Some(out) = args.get(4) {
        for p in state.written.keys() {
            let src = game.resolve(p).unwrap();
            let dst = std::path::Path::new(out).join(src.file_name().unwrap());
            std::fs::copy(&src, dst).unwrap();
        }
    }
    if std::env::var("KEEP").is_ok() {
        return;
    }
    for m in idx.mods.iter().rev() {
        install::uninstall(&game, &idx.game.pak_key, &m.id).unwrap();
        let state = install::State::load(&game).unwrap();
        println!(
            "removed {} -> {} files still written",
            m.id,
            state.written.len()
        );
        for (p, sha) in &state.written {
            let ok = fsx::sha256_file(&game.resolve(p).unwrap()).unwrap() == *sha;
            println!("  {p} on disk matches: {ok}");
        }
    }
}
