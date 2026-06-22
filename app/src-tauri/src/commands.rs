use tauri::{path::BaseDirectory, Manager, State};

use crate::error::ErrorResponse;
use crate::models::{
    AskAgentRequest, CancelParseJobRequest, CreateKnowledgeSpaceRequest, DefaultPermissionRequest,
    EnqueueOcrJobRequest, IndexKnowledgeSpaceRequest, PermissionRequest, RunOcrJobRequest,
    RuntimeStatus, ScanKnowledgeSpaceRequest, WorkbenchSnapshot,
};
use crate::state::AppState;

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
    state: State<'_, AppState>,
    request: ScanKnowledgeSpaceRequest,
) -> Result<WorkbenchSnapshot, ErrorResponse> {
    state
        .scan_knowledge_space(request.space_id)
        .map_err(Into::into)
}

#[tauri::command]
pub fn index_knowledge_space(
    state: State<'_, AppState>,
    request: IndexKnowledgeSpaceRequest,
) -> Result<WorkbenchSnapshot, ErrorResponse> {
    state
        .index_knowledge_space(request.space_id)
        .map_err(Into::into)
}

#[tauri::command]
pub fn enqueue_ocr_parse_job(
    state: State<'_, AppState>,
    request: EnqueueOcrJobRequest,
) -> Result<WorkbenchSnapshot, ErrorResponse> {
    state
        .enqueue_ocr_parse_job(request.space_id, request.file_id)
        .map_err(Into::into)
}

#[tauri::command]
pub fn cancel_parse_job(
    state: State<'_, AppState>,
    request: CancelParseJobRequest,
) -> Result<WorkbenchSnapshot, ErrorResponse> {
    state.cancel_parse_job(request.job_id).map_err(Into::into)
}

#[tauri::command]
pub fn run_next_ocr_parse_job(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: RunOcrJobRequest,
) -> Result<WorkbenchSnapshot, ErrorResponse> {
    let resource_script = app
        .path()
        .resolve("sidecars/ocr/ocr_sidecar.py", BaseDirectory::Resource)
        .ok();

    state
        .run_next_ocr_parse_job(request.space_id, resource_script)
        .map_err(Into::into)
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
pub fn set_default_permission(
    state: State<'_, AppState>,
    request: DefaultPermissionRequest,
) -> Result<WorkbenchSnapshot, ErrorResponse> {
    state
        .update_default_permission(request.space_id, request.permission)
        .map_err(Into::into)
}

#[tauri::command]
pub fn get_runtime_status(app: tauri::AppHandle) -> Result<RuntimeStatus, ErrorResponse> {
    let app_data_dir = app.path().app_data_dir().map_err(|error| ErrorResponse {
        message: format!("无法读取应用数据目录：{error}"),
    })?;

    Ok(crate::runtime::runtime_status(&app_data_dir))
}
