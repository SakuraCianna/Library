use tauri::State;

use crate::error::{AppError, ErrorResponse};
use crate::models::{can_temporarily_escalate, PermissionRequest, WorkbenchSnapshot};
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
    let snapshot = state.snapshot();
    let active_space = snapshot
        .spaces
        .iter()
        .find(|space| space.id == snapshot.active_space_id)
        .ok_or_else(|| AppError::Storage("找不到当前知识库".to_string()))?;

    if !can_temporarily_escalate(&active_space.default_permission, &request.requested) {
        return Err(
            AppError::PermissionDenied("当前文件夹默认权限不允许这样临时升权".to_string()).into(),
        );
    }

    state.set_session_permission(request.requested);
    Ok(state.snapshot())
}
