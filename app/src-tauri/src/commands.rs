use tauri::State;

use crate::error::ErrorResponse;
use crate::models::{PermissionRequest, WorkbenchSnapshot};
use crate::state::AppState;

#[tauri::command]
pub fn get_workbench_snapshot(state: State<'_, AppState>) -> WorkbenchSnapshot {
    state.snapshot()
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
