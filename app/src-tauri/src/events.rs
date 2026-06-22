use serde::Serialize;
use tauri::{AppHandle, Emitter};

pub const WORKBENCH_UPDATED_EVENT: &str = "workbench-updated";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkbenchUpdatedPayload {
    space_id: Option<String>,
    reason: String,
}

pub fn emit_workbench_updated(app: &AppHandle, space_id: Option<&str>, reason: &str) {
    let payload = WorkbenchUpdatedPayload {
        space_id: space_id.map(ToOwned::to_owned),
        reason: reason.to_string(),
    };

    if let Err(error) = app.emit(WORKBENCH_UPDATED_EVENT, payload) {
        eprintln!("failed to emit workbench update event: {error}");
    }
}
