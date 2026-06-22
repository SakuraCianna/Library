use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    Readonly,
    Approval,
    Full,
}

impl PermissionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Readonly => "readonly",
            Self::Approval => "approval",
            Self::Full => "full",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "readonly" => Some(Self::Readonly),
            "approval" => Some(Self::Approval),
            "full" => Some(Self::Full),
            _ => None,
        }
    }
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

impl ParseStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Indexed => "indexed",
            Self::Changed => "changed",
            Self::Queued => "queued",
            Self::Failed => "failed",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "indexed" => Some(Self::Indexed),
            "changed" => Some(Self::Changed),
            "queued" => Some(Self::Queued),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Indexed => "已索引",
            Self::Changed => "已变更",
            Self::Queued => "待解析",
            Self::Failed => "扫描失败",
        }
    }
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub deepseek: DeepSeekRuntimeStatus,
    pub ocr: OcrRuntimeStatus,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepSeekRuntimeStatus {
    pub configured: bool,
    pub model: String,
    pub base_url: String,
    pub key_hint: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OcrRuntimeStatus {
    pub configured: bool,
    pub tier: String,
    pub model_dir: String,
    pub missing_models: Vec<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateKnowledgeSpaceRequest {
    pub name: String,
    pub root_path: String,
    pub default_permission: PermissionMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanKnowledgeSpaceRequest {
    pub space_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultPermissionRequest {
    pub space_id: String,
    pub permission: PermissionMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedFile {
    pub relative_path: String,
    pub extension: String,
    pub size_bytes: i64,
    pub modified_at: String,
    pub content_hash: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanSummary {
    pub added_count: u32,
    pub changed_count: u32,
    pub deleted_count: u32,
    pub failed_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileParseCandidate {
    pub file_id: String,
    pub relative_path: String,
    pub extension: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDocument {
    pub title: String,
    pub body: String,
    pub summary: String,
    pub source_locator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeBlockSearchHit {
    pub id: String,
    pub title: String,
    pub excerpt: String,
    pub source_file_name: String,
    pub source_locator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexKnowledgeSpaceRequest {
    pub space_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskAgentRequest {
    pub space_id: String,
    pub question: String,
}

pub fn can_request_session_permission(
    folder_default: &PermissionMode,
    requested: &PermissionMode,
) -> bool {
    match folder_default {
        PermissionMode::Readonly => matches!(requested, PermissionMode::Readonly),
        PermissionMode::Approval => matches!(
            requested,
            PermissionMode::Readonly | PermissionMode::Approval
        ),
        PermissionMode::Full => true,
    }
}

#[cfg(test)]
mod tests {
    use super::{can_request_session_permission, PermissionMode};

    #[test]
    fn permission_session_matrix_matches_domain_boundary() {
        let cases = [
            (PermissionMode::Readonly, PermissionMode::Readonly, true),
            (PermissionMode::Readonly, PermissionMode::Approval, false),
            (PermissionMode::Readonly, PermissionMode::Full, false),
            (PermissionMode::Approval, PermissionMode::Readonly, true),
            (PermissionMode::Approval, PermissionMode::Approval, true),
            (PermissionMode::Approval, PermissionMode::Full, false),
            (PermissionMode::Full, PermissionMode::Readonly, true),
            (PermissionMode::Full, PermissionMode::Approval, true),
            (PermissionMode::Full, PermissionMode::Full, true),
        ];

        for (folder_default, requested, expected) in cases {
            assert_eq!(
                can_request_session_permission(&folder_default, &requested),
                expected,
                "folder_default={folder_default:?}, requested={requested:?}"
            );
        }
    }
}
