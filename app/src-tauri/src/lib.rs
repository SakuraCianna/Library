mod commands;
mod error;
mod models;
mod state;
pub mod storage;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::new_with_mock_data())
        .invoke_handler(tauri::generate_handler![
            commands::get_workbench_snapshot,
            commands::set_session_permission
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
