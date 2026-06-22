mod agent;
mod commands;
mod error;
mod events;
mod models;
mod ocr;
mod parser;
mod runtime;
mod scanner;
mod state;
pub mod storage;
pub mod vector;

use state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            app.manage(AppState::open(app_data_dir)?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_workbench_snapshot,
            commands::set_session_permission,
            commands::create_knowledge_space,
            commands::scan_knowledge_space,
            commands::index_knowledge_space,
            commands::enqueue_ocr_parse_job,
            commands::cancel_parse_job,
            commands::start_ocr_worker,
            commands::ask_agent,
            commands::open_source_file,
            commands::get_knowledge_block_context,
            commands::set_default_permission,
            commands::export_space_backup,
            commands::get_runtime_status,
            commands::check_ocr_environment
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
