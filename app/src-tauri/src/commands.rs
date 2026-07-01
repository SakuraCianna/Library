use tauri::{path::BaseDirectory, Manager, State};

use crate::error::ErrorResponse;
use crate::events::emit_workbench_updated;
use crate::models::{
    AskAgentRequest, BackupExportResult, BackupRestorePreflight, BackupRestoreResult,
    CancelParseJobRequest, CreateKnowledgeSpaceRequest, DefaultPermissionRequest,
    EnqueueOcrJobRequest, ExportSpaceBackupRequest, IndexKnowledgeSpaceRequest,
    KnowledgeBlockContext, KnowledgeBlockContextRequest, OcrEnvironmentReport,
    OpenSourceFileRequest, PermissionRequest, PreflightSpaceBackupRestoreRequest,
    RestoreSpaceBackupRequest, RuntimeStatus, ScanKnowledgeSpaceRequest, StartOcrWorkerRequest,
    WorkbenchSnapshot, CreateConversationRequest, ListConversationsRequest, SwitchConversationRequest,
};
use crate::state::AppState;
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
pub fn get_workbench_snapshot(
    state: State<'_, AppState>,
) -> Result<WorkbenchSnapshot, ErrorResponse> {
    state.snapshot().map_err(Into::into)
}

#[tauri::command]
pub fn set_session_permission(
    state: State<'_, AppState>,
    request: PermissionRequest,
) -> Result<WorkbenchSnapshot, ErrorResponse> {
    state
        .request_session_permission(request.requested)
        .map_err(Into::into)
}

#[tauri::command]
pub fn create_knowledge_space(
    state: State<'_, AppState>,
    request: CreateKnowledgeSpaceRequest,
) -> Result<WorkbenchSnapshot, ErrorResponse> {
    state
        .create_knowledge_space(request.name, request.root_path, request.default_permission)
        .map_err(Into::into)
}

#[tauri::command]
pub fn scan_knowledge_space(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: ScanKnowledgeSpaceRequest,
) -> Result<WorkbenchSnapshot, ErrorResponse> {
    let space_id = request.space_id;
    let should_spawn = state
        .prepare_scan_knowledge_space(space_id.clone())
        .map_err(ErrorResponse::from)?;

    if should_spawn {
        let app_for_worker = app.clone();
        std::thread::spawn(move || {
            let worker_state = app_for_worker.state::<AppState>();
            let worker_space_id = space_id.clone();
            worker_state.run_scan_worker(space_id, |reason| {
                emit_workbench_updated(&app_for_worker, Some(&worker_space_id), reason);
            });
        });
    }

    state.snapshot().map_err(Into::into)
}

#[tauri::command]
pub fn index_knowledge_space(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: IndexKnowledgeSpaceRequest,
) -> Result<WorkbenchSnapshot, ErrorResponse> {
    let space_id = request.space_id;
    let should_spawn = state
        .prepare_document_indexing(space_id.clone())
        .map_err(ErrorResponse::from)?;

    if should_spawn {
        let resource_script = app
            .path()
            .resolve("sidecars/parser/parser_sidecar.py", BaseDirectory::Resource)
            .ok();
        let app_for_worker = app.clone();
        std::thread::spawn(move || {
            let worker_state = app_for_worker.state::<AppState>();
            let worker_space_id = space_id.clone();
            worker_state.run_document_worker(space_id, resource_script, |reason| {
                emit_workbench_updated(&app_for_worker, Some(&worker_space_id), reason);
            });
        });
    }

    state.snapshot().map_err(Into::into)
}

#[tauri::command]
pub fn enqueue_ocr_parse_job(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: EnqueueOcrJobRequest,
) -> Result<WorkbenchSnapshot, ErrorResponse> {
    let space_id = request.space_id.clone();
    state
        .enqueue_ocr_parse_job(request.space_id, request.file_id, request.source_locator)
        .inspect(|_| emit_workbench_updated(&app, Some(&space_id), "ocr-queued"))
        .map_err(Into::into)
}

#[tauri::command]
pub fn cancel_parse_job(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: CancelParseJobRequest,
) -> Result<WorkbenchSnapshot, ErrorResponse> {
    state
        .cancel_parse_job(request.job_id)
        .inspect(|_| emit_workbench_updated(&app, None, "parse-job-cancelled"))
        .map_err(Into::into)
}

#[tauri::command]
pub fn start_ocr_worker(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: StartOcrWorkerRequest,
) -> Result<WorkbenchSnapshot, ErrorResponse> {
    let resource_script = app
        .path()
        .resolve("sidecars/ocr/ocr_sidecar.py", BaseDirectory::Resource)
        .ok();
    let space_id = request.space_id;
    let should_spawn = state
        .begin_ocr_worker(space_id.clone())
        .map_err(ErrorResponse::from)?;

    if should_spawn {
        let app_for_worker = app.clone();
        std::thread::spawn(move || {
            let worker_state = app_for_worker.state::<AppState>();
            let worker_space_id = space_id.clone();
            worker_state.run_ocr_worker(space_id, resource_script, |reason| {
                emit_workbench_updated(&app_for_worker, Some(&worker_space_id), reason);
            });
        });
    }

    state.snapshot().map_err(Into::into)
}

#[tauri::command]
pub async fn ask_agent(
    state: State<'_, AppState>,
    request: AskAgentRequest,
) -> Result<WorkbenchSnapshot, ErrorResponse> {
    state
        .ask_agent(request.space_id, request.question)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub fn open_source_file(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: OpenSourceFileRequest,
) -> Result<(), ErrorResponse> {
    let source_path = state
        .resolve_source_file_path(&request.space_id, &request.source_locator)
        .map_err(ErrorResponse::from)?;

    app.opener()
        .open_path(source_path.to_string_lossy().to_string(), None::<&str>)
        .map_err(|error| ErrorResponse {
            message: format!("无法打开来源文件：{error}"),
        })
}

#[tauri::command]
pub fn get_knowledge_block_context(
    state: State<'_, AppState>,
    request: KnowledgeBlockContextRequest,
) -> Result<KnowledgeBlockContext, ErrorResponse> {
    state
        .knowledge_block_context(request.space_id, request.block_id)
        .map_err(Into::into)
}

#[tauri::command]
pub fn set_default_permission(
    state: State<'_, AppState>,
    request: DefaultPermissionRequest,
) -> Result<WorkbenchSnapshot, ErrorResponse> {
    state
        .update_default_permission(request.space_id, request.permission)
        .map_err(Into::into)
}

#[tauri::command]
pub fn export_space_backup(
    state: State<'_, AppState>,
    request: ExportSpaceBackupRequest,
) -> Result<BackupExportResult, ErrorResponse> {
    state
        .export_space_backup(request.space_id, request.file_name)
        .map_err(Into::into)
}

#[tauri::command]
pub fn preflight_space_backup_restore(
    state: State<'_, AppState>,
    request: PreflightSpaceBackupRestoreRequest,
) -> Result<BackupRestorePreflight, ErrorResponse> {
    state
        .preflight_space_backup_restore(request.path)
        .map_err(Into::into)
}

#[tauri::command]
pub fn restore_space_backup(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: RestoreSpaceBackupRequest,
) -> Result<BackupRestoreResult, ErrorResponse> {
    state
        .restore_space_backup(request.path, request.confirm_overwrite)
        .inspect(|result| emit_workbench_updated(&app, Some(&result.space_id), "backup-restored"))
        .map_err(Into::into)
}

#[tauri::command]
pub fn get_runtime_status(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<RuntimeStatus, ErrorResponse> {
    let app_data_dir = app.path().app_data_dir().map_err(|error| ErrorResponse {
        message: format!("无法读取应用数据目录：{error}"),
    })?;

    let mut status = crate::runtime::runtime_status(&app_data_dir);
    if let Some(config) = state.get_deepseek_config() {
        let is_configured = !config.api_key.trim().is_empty();
        let key_hint = if is_configured {
            let key = config.api_key.trim();
            if key.len() > 8 {
                format!("{}...{}", &key[..4], &key[key.len() - 4..])
            } else {
                "***".to_string()
            }
        } else {
            "".to_string()
        };
        
        status.deepseek = crate::models::DeepSeekRuntimeStatus {
            model: config.model,
            base_url: config.base_url,
            configured: is_configured,
            key_hint,
        };
    }

    Ok(status)
}

#[tauri::command]
pub fn check_ocr_environment(app: tauri::AppHandle) -> Result<OcrEnvironmentReport, ErrorResponse> {
    let app_data_dir = app.path().app_data_dir().map_err(|error| ErrorResponse {
        message: format!("无法读取应用数据目录：{error}"),
    })?;
    let resource_checker = app
        .path()
        .resolve(
            "sidecars/ocr/check_ocr_environment.py",
            BaseDirectory::Resource,
        )
        .ok();

    crate::ocr::check_ocr_environment(&app_data_dir, resource_checker.as_deref())
        .map_err(Into::into)
}

#[tauri::command]
pub fn get_agent_tone(state: State<'_, AppState>) -> Result<Option<String>, ErrorResponse> {
    state.get_agent_tone().map_err(Into::into)
}

#[tauri::command]
pub fn set_agent_tone(state: State<'_, AppState>, tone: String) -> Result<(), ErrorResponse> {
    state.set_agent_tone(&tone).map_err(Into::into)
}

#[tauri::command]
pub fn switch_conversation(
    state: State<'_, AppState>,
    request: SwitchConversationRequest,
) -> Result<WorkbenchSnapshot, ErrorResponse> {
    state.switch_conversation(request.conversation_id);
    state.snapshot().map_err(Into::into)
}

#[tauri::command]
pub fn create_conversation(
    state: State<'_, AppState>,
    request: CreateConversationRequest,
) -> Result<crate::models::Conversation, ErrorResponse> {
    state.create_conversation(&request.space_id, &request.title).map_err(|e| ErrorResponse {
        message: e.to_string(),
    })
}

#[tauri::command]
pub fn list_conversations(
    state: State<'_, AppState>,
    request: ListConversationsRequest,
) -> Result<Vec<crate::models::Conversation>, ErrorResponse> {
    state.list_conversations(&request.space_id).map_err(|e| ErrorResponse {
        message: format!("StorageError: {}", e),
    })
}

#[derive(serde::Serialize)]
pub struct UserSettings {
    pub deepseek_api_key: String,
    pub deepseek_model: String,
    pub deepseek_base_url: String,
}

#[tauri::command]
pub fn get_user_settings(
    state: State<'_, AppState>,
) -> Result<UserSettings, ErrorResponse> {
    let config = state.get_deepseek_config();
    if let Some(c) = config {
        Ok(UserSettings {
            deepseek_api_key: c.api_key,
            deepseek_model: c.model,
            deepseek_base_url: c.base_url,
        })
    } else {
        Ok(UserSettings {
            deepseek_api_key: "".to_string(),
            deepseek_model: "deepseek-v4-flash".to_string(),
            deepseek_base_url: "https://api.deepseek.com".to_string(),
        })
    }
}

#[derive(serde::Deserialize)]
pub struct UpdateUserSettingsPayload {
    pub deepseek_api_key: Option<String>,
    pub deepseek_model: Option<String>,
    pub deepseek_base_url: Option<String>,
}

#[tauri::command]
pub fn update_user_settings(
    state: State<'_, AppState>,
    settings: UpdateUserSettingsPayload,
) -> Result<(), ErrorResponse> {
    if let Some(key) = settings.deepseek_api_key {
        state.set_setting("DEEPSEEK_API_KEY", &key).map_err(|e| ErrorResponse { message: e.to_string() })?;
    }
    if let Some(model) = settings.deepseek_model {
        state.set_setting("DEEPSEEK_MODEL", &model).map_err(|e| ErrorResponse { message: e.to_string() })?;
    }
    if let Some(url) = settings.deepseek_base_url {
        state.set_setting("DEEPSEEK_BASE_URL", &url).map_err(|e| ErrorResponse { message: e.to_string() })?;
    }
    Ok(())
}
