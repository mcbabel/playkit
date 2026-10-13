#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod config;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(commands::AppState::new(config::Config::load()))
        .invoke_handler(tauri::generate_handler![
            commands::status,
            commands::mods,
            commands::install,
            commands::uninstall,
            commands::set_enabled,
            commands::update_all,
            commands::setup,
            commands::remove_loader,
            commands::play,
            commands::set_game_dir,
            commands::set_index_url,
            commands::pack,
            commands::open_url,
            commands::reveal,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Playkit");
}
