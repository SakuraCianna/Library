use tauri::State;

use crate::error::ErrorResponse;
use crate::models::{
    CreateKnowledgeSpaceRequest, DefaultPermissionRequest, PermissionRequest,
    ScanKnowledgeSpaceRequest, WorkbenchSnapshot,
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
pub fn set_default_permission(
    state: State<'_, AppState>,
    request: DefaultPermissionRequest,
) -> Result<WorkbenchSnapshot, ErrorResponse> {
    state
        .update_default_permission(request.space_id, request.permission)
        .map_err(Into::into)
}
