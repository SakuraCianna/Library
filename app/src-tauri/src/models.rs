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
    pub scan_queue_count: u32,
    pub document_queue_count: u32,
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
    #[serde(default)]
    pub source_locator: String,
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
pub struct ChatMessageSource {
    pub id: String,
    pub title: String,
    pub excerpt: String,
    pub source_file_name: String,
    pub source_locator: String,
    pub source_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub role: ChatRole,
    pub content: String,
    #[serde(default)]
    pub sources: Vec<ChatMessageSource>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OcrEnvironmentReport {
    pub ok: bool,
    pub checks: Vec<OcrEnvironmentCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OcrEnvironmentCheck {
    pub name: String,
    pub ok: bool,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbenchSnapshot {
    pub spaces: Vec<KnowledgeSpace>,
    pub active_space_id: String,
    pub active_scope: ChatScope,
    pub session_permission: PermissionMode,
    pub files: Vec<KnowledgeFile>,
    pub parse_jobs: Vec<ParseJobSummary>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSpaceBackupRequest {
    pub space_id: String,
    pub file_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupExportResult {
    pub path: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub exported_at: String,
    pub file_count: u32,
    pub knowledge_block_count: u32,
    pub parse_job_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightSpaceBackupRestoreRequest {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreSpaceBackupRequest {
    pub path: String,
    pub confirm_overwrite: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupRestorePreflight {
    pub path: String,
    pub file_name: String,
    pub format: String,
    pub schema_version: u32,
    pub exported_at: String,
    pub space_id: String,
    pub space_name: String,
    pub root_path: String,
    pub default_permission: PermissionMode,
    pub file_count: u32,
    pub knowledge_block_count: u32,
    pub parse_job_count: u32,
    pub trash_entry_count: u32,
    pub will_overwrite: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupRestoreResult {
    pub path: String,
    pub file_name: String,
    pub format: String,
    pub schema_version: u32,
    pub exported_at: String,
    pub space_id: String,
    pub space_name: String,
    pub root_path: String,
    pub default_permission: PermissionMode,
    pub file_count: u32,
    pub knowledge_block_count: u32,
    pub parse_job_count: u32,
    pub trash_entry_count: u32,
    pub will_overwrite: bool,
    pub restored_at: String,
    pub overwritten: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupExport {
    pub format: String,
    pub schema_version: u32,
    pub exported_at: String,
    pub space: BackupExportSpace,
    pub workspace: BackupExportWorkspace,
    pub files: Vec<BackupExportFile>,
    pub markdown_notes: Vec<BackupExportMarkdownNote>,
    pub knowledge_blocks: Vec<BackupExportKnowledgeBlock>,
    pub parse_jobs: Vec<BackupExportParseJob>,
    pub trash_entries: Vec<BackupExportTrashEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupExportSpace {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub default_permission: PermissionMode,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupExportWorkspace {
    pub active_space_id: String,
    pub default_permission: PermissionMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupExportFile {
    pub id: String,
    pub relative_path: String,
    pub extension: String,
    pub content_hash: Option<String>,
    pub size_bytes: i64,
    pub modified_at: Option<String>,
    pub parse_status: String,
    pub last_scanned_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupExportMarkdownNote {
    pub id: String,
    pub file_id: Option<String>,
    pub relative_path: String,
    pub user_editable: bool,
    pub last_generated_hash: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupExportKnowledgeBlock {
    pub id: String,
    pub file_id: Option<String>,
    pub note_id: Option<String>,
    pub title: String,
    pub body: String,
    pub source_kind: String,
    pub source_locator: String,
    pub searchable: bool,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupExportParseJob {
    pub id: String,
    pub file_id: Option<String>,
    #[serde(default)]
    pub source_locator: Option<String>,
    pub job_type: String,
    pub status: String,
    pub error_message: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub progress_current: u32,
    pub progress_total: u32,
    pub phase: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupExportTrashEntry {
    pub id: String,
    pub entity_kind: String,
    pub entity_id: String,
    pub display_name: String,
    pub original_locator: String,
    pub deleted_at: String,
    pub restored_at: Option<String>,
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
pub struct ParseJobCandidate {
    pub job_id: String,
    pub file_id: String,
    pub relative_path: String,
    pub extension: String,
    pub source_locator: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ParsedDocument {
    pub title: String,
    pub body: String,
    pub summary: String,
    pub source_locator: String,
    #[serde(default)]
    pub segments: Vec<ParsedDocumentSegment>,
    pub table_insights: Vec<ParsedTableInsight>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ParsedDocumentSegment {
    pub title: String,
    pub body: String,
    pub source_locator: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<ParsedEvidenceMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ParsedEvidenceMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_number: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_number: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub char_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_percent: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ParsedTableInsight {
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
    pub source_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeBlockContext {
    pub current_index: u32,
    pub total_count: u32,
    pub blocks: Vec<KnowledgeBlockSearchHit>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenSourceFileRequest {
    pub space_id: String,
    pub source_locator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeBlockContextRequest {
    pub space_id: String,
    pub block_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueOcrJobRequest {
    pub space_id: String,
    pub file_id: String,
    #[serde(default)]
    pub source_locator: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartOcrWorkerRequest {
    pub space_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelParseJobRequest {
    pub job_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OcrSidecarRequest {
    pub file_path: String,
    pub model_dir: String,
    pub tier: String,
    pub max_pdf_pages: u32,
    pub max_image_pixels: u64,
    pub progress: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temp_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OcrPageResult {
    pub page_index: u32,
    pub text: String,
    pub confidence: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub char_count: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OcrSidecarResult {
    pub text: String,
    pub page_count: u32,
    pub pages: Vec<OcrPageResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ParseJobSummary {
    pub id: String,
    pub file_id: Option<String>,
    pub file_name: String,
    pub source_locator: Option<String>,
    pub job_type: String,
    pub status: String,
    pub error_message: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub progress_current: u32,
    pub progress_total: u32,
    pub phase: String,
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
