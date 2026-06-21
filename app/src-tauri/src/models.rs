use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    Readonly,
    Approval,
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatScope {
    CurrentFile,
    CurrentFolder,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParseStatus {
    Indexed,
    Changed,
    Queued,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeSpace {
    pub id: String,
    pub name: String,
    pub path: String,
    pub default_permission: PermissionMode,
    pub changed_file_count: u32,
    pub ocr_queue_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeFile {
    pub id: String,
    pub name: String,
    pub extension: String,
    pub status: ParseStatus,
    pub status_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeBlockPreview {
    pub id: String,
    pub title: String,
    pub excerpt: String,
    pub source_file_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableInsightPreview {
    pub id: String,
    pub title: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub role: ChatRole,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingAction {
    pub id: String,
    pub label: String,
    pub requires_approval: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbenchSnapshot {
    pub spaces: Vec<KnowledgeSpace>,
    pub active_space_id: String,
    pub active_scope: ChatScope,
    pub session_permission: PermissionMode,
    pub files: Vec<KnowledgeFile>,
    pub block_preview: KnowledgeBlockPreview,
    pub table_preview: TableInsightPreview,
    pub messages: Vec<ChatMessage>,
    pub pending_action: Option<PendingAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequest {
    pub requested: PermissionMode,
}

pub fn can_temporarily_escalate(
    folder_default: &PermissionMode,
    requested: &PermissionMode,
) -> bool {
    matches!(
        (folder_default, requested),
        (PermissionMode::Readonly, PermissionMode::Approval)
            | (PermissionMode::Approval, PermissionMode::Approval)
            | (PermissionMode::Full, PermissionMode::Approval)
            | (PermissionMode::Full, PermissionMode::Full)
    )
}

#[cfg(test)]
mod tests {
    use super::{can_temporarily_escalate, PermissionMode};

    #[test]
    fn permission_escalation_matrix_matches_domain_boundary() {
        let cases = [
            (PermissionMode::Readonly, PermissionMode::Readonly, false),
            (PermissionMode::Readonly, PermissionMode::Approval, true),
            (PermissionMode::Readonly, PermissionMode::Full, false),
            (PermissionMode::Approval, PermissionMode::Readonly, false),
            (PermissionMode::Approval, PermissionMode::Approval, true),
            (PermissionMode::Approval, PermissionMode::Full, false),
            (PermissionMode::Full, PermissionMode::Readonly, false),
            (PermissionMode::Full, PermissionMode::Approval, true),
            (PermissionMode::Full, PermissionMode::Full, true),
        ];

        for (folder_default, requested, expected) in cases {
            assert_eq!(
                can_temporarily_escalate(&folder_default, &requested),
                expected,
                "folder_default={folder_default:?}, requested={requested:?}"
            );
        }
    }
}
