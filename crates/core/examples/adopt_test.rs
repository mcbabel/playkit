use playkit_core::{game::Game, index, install};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let game = Game::from_dir(std::path::Path::new(&args[2])).unwrap();
    let idx = index::fetch(&args[1]).await.unwrap();
    let adopted = install::adopt(&game, &idx).await.unwrap();
    println!("recognised {} of {}:", adopted.len(), idx.mods.len());
    for a in &adopted {
        println!("  {a}");
    }
    let state = install::State::load(&game).unwrap();
    for m in &idx.mods {
        if !state.mods.contains_key(&m.id) {
            println!("  not recognised: {}", m.name);
        }
    }
    if let Some(id) = args.get(3) {
        install::uninstall(&game, &idx.game.pak_key, id).unwrap();
        let state = install::State::load(&game).unwrap();
        for id in state.mods.keys() {
            println!("  after uninstall {id}: {:?}", state.health(&game, id));
        }
    }
}
