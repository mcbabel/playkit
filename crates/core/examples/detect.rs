use playkit_core::{fsx, game::Game, loader, pak::Pak, platform};

fn main() {
    let p = platform::detect();
    println!("platform: {p:?}");
    let g = Game::detect().expect("game");
    println!("game: {} build {:?}", g.root.display(), g.build_id);
    println!("steam: {:?}", g.steam);
    println!(
        "loader: {:?}",
        loader::status(
            &g,
            &p,
            &loader::Bundled {
                bytes: b"",
                known_old: &[]
            }
        )
    );
    println!("game running (this folder): {}", g.is_running());
    if let Some(key) = std::env::args().nth(1) {
        let mut pak = Pak::open(&g.pak(), &key).expect("pak");
        let f = "Playtime_Multiplayer/Content/Gameplay/Puzzles/Triggers/BP_Puzzle_Piano.uasset";
        let data = pak.read(f).expect("read");
        println!("{f}: {} bytes sha {}", data.len(), fsx::sha256(&data));
    }
}
