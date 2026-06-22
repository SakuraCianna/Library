use tauri::{path::BaseDirectory, Manager, State};

use crate::error::ErrorResponse;
use crate::events::emit_workbench_updated;
use crate::models::{
    AskAgentRequest, CancelParseJobRequest, CreateKnowledgeSpaceRequest, DefaultPermissionRequest,
    EnqueueOcrJobRequest, IndexKnowledgeSpaceRequest, PermissionRequest, RuntimeStatus,
    ScanKnowledgeSpaceRequest, StartOcrWorkerRequest, WorkbenchSnapshot,
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
        let app_for_worker = app.clone();
        std::thread::spawn(move || {
            let worker_state = app_for_worker.state::<AppState>();
            let worker_space_id = space_id.clone();
            worker_state.run_document_worker(space_id, |reason| {
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
        .enqueue_ocr_parse_job(request.space_id, request.file_id)
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
