use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::error::AppError;
use crate::models::{
    can_request_session_permission, ChatMessage, ChatRole, ChatScope, KnowledgeBlockPreview,
    KnowledgeSpace, PermissionMode, ScanSummary, TableInsightPreview, WorkbenchSnapshot,
};
use crate::scanner::scan_folder;
use crate::storage::sqlite::SqliteStore;

pub struct AppState {
    store: Mutex<SqliteStore>,
    active_space_id: Mutex<Option<String>>,
    active_scope: Mutex<ChatScope>,
    session_permission: Mutex<PermissionMode>,
}

impl AppState {
    pub fn open(app_data_dir: PathBuf) -> Result<Self, AppError> {
        fs::create_dir_all(&app_data_dir)
            .map_err(|error| AppError::Filesystem(format!("无法创建应用数据目录：{}", error)))?;
        let db_path = app_data_dir.join("library.sqlite3");
        let store = SqliteStore::open(&db_path)
            .map_err(|error| AppError::Storage(format!("无法打开本地数据库：{error}")))?;

        Ok(Self::new(store))
    }

    pub fn new(store: SqliteStore) -> Self {
        Self {
            store: Mutex::new(store),
            active_space_id: Mutex::new(None),
            active_scope: Mutex::new(ChatScope::CurrentFolder),
            session_permission: Mutex::new(PermissionMode::Readonly),
        }
    }

    pub fn snapshot(&self) -> Result<WorkbenchSnapshot, AppError> {
        let store = self.store.lock().expect("sqlite store mutex poisoned");
        let spaces = store
            .list_knowledge_spaces()
            .map_err(|error| AppError::Storage(error.to_string()))?;
        let active_space_id = self.resolve_active_space_id(&spaces);
        let active_space = spaces
            .iter()
            .find(|space| space.id == active_space_id)
            .cloned();
        let files = match active_space.as_ref() {
            Some(space) => store
                .list_files(&space.id)
                .map_err(|error| AppError::Storage(error.to_string()))?,
            None => Vec::new(),
        };

        let session_permission = self.resolve_session_permission(active_space.as_ref());
        Ok(build_snapshot(
            spaces,
            active_space_id,
            self.active_scope
                .lock()
                .expect("active scope mutex poisoned")
                .clone(),
            session_permission,
            files,
        ))
    }

    pub fn create_knowledge_space(
        &self,
        name: String,
        root_path: String,
        default_permission: PermissionMode,
    ) -> Result<WorkbenchSnapshot, AppError> {
        let root = PathBuf::from(root_path.trim());
        validate_folder_path(&root)?;

        let root_path = root
            .canonicalize()
            .map_err(|error| AppError::Filesystem(format!("无法规范化文件夹路径：{error}")))?
            .to_string_lossy()
            .to_string();
        let store = self.store.lock().expect("sqlite store mutex poisoned");
        let space_id = store
            .create_knowledge_space(name.trim(), &root_path, default_permission.clone())
            .map_err(|error| AppError::Storage(format!("无法创建知识库：{error}")))?;

        *self
            .active_space_id
            .lock()
            .expect("active space mutex poisoned") = Some(space_id);
        *self
            .session_permission
            .lock()
            .expect("session permission mutex poisoned") = default_permission;
        drop(store);

        self.snapshot()
    }

    pub fn scan_knowledge_space(&self, space_id: String) -> Result<WorkbenchSnapshot, AppError> {
        let root_path = {
            let store = self.store.lock().expect("sqlite store mutex poisoned");
            store
                .get_space_root(&space_id)
                .map_err(|error| AppError::Storage(error.to_string()))?
                .ok_or_else(|| AppError::Storage("找不到要扫描的知识库".to_string()))?
                .root_path
        };
        let scanned_files = scan_folder(Path::new(&root_path)).map_err(|error| {
            AppError::Filesystem(format!("无法扫描文件夹 {root_path}：{error}"))
        })?;

        let mut store = self.store.lock().expect("sqlite store mutex poisoned");
        let _summary: ScanSummary = store
            .apply_scan_results(&space_id, &scanned_files)
            .map_err(|error| AppError::Storage(format!("无法保存扫描结果：{error}")))?;
        *self
            .active_space_id
            .lock()
            .expect("active space mutex poisoned") = Some(space_id);
        drop(store);

        self.snapshot()
    }

    pub fn update_default_permission(
        &self,
        space_id: String,
        permission: PermissionMode,
    ) -> Result<WorkbenchSnapshot, AppError> {
        let store = self.store.lock().expect("sqlite store mutex poisoned");
        let updated = store
            .update_knowledge_space_permission(&space_id, permission.clone())
            .map_err(|error| AppError::Storage(format!("无法更新默认权限：{error}")))?;
        if !updated {
            return Err(AppError::Storage("找不到要更新的知识库".to_string()));
        }

        let mut session_permission = self
            .session_permission
            .lock()
            .expect("session permission mutex poisoned");
        if !can_request_session_permission(&permission, &session_permission) {
            *session_permission = permission;
        }
        drop(session_permission);
        drop(store);

        self.snapshot()
    }

    pub fn request_session_permission(
        &self,
        requested: PermissionMode,
    ) -> Result<WorkbenchSnapshot, AppError> {
        let snapshot = self.snapshot()?;
        let active_space = snapshot
            .spaces
            .iter()
            .find(|space| space.id == snapshot.active_space_id)
            .ok_or_else(|| AppError::Storage("找不到当前知识库".to_string()))?;

        if !can_request_session_permission(&active_space.default_permission, &requested) {
            return Err(AppError::PermissionDenied(
                "当前文件夹默认权限不允许这样临时升权".to_string(),
            ));
        }

        *self
            .session_permission
            .lock()
            .expect("session permission mutex poisoned") = requested;
        self.snapshot()
    }

    fn resolve_active_space_id(&self, spaces: &[KnowledgeSpace]) -> String {
        let mut active_space_id = self
            .active_space_id
            .lock()
            .expect("active space mutex poisoned");
        let current = active_space_id
            .as_ref()
            .filter(|id| spaces.iter().any(|space| space.id == **id))
            .cloned();

        if let Some(space_id) = current {
            return space_id;
        }

        let fallback = spaces.first().map(|space| space.id.clone());
        *active_space_id = fallback.clone();
        fallback.unwrap_or_default()
    }

    fn resolve_session_permission(&self, active_space: Option<&KnowledgeSpace>) -> PermissionMode {
        let mut session_permission = self
            .session_permission
            .lock()
            .expect("session permission mutex poisoned");
        let Some(space) = active_space else {
            *session_permission = PermissionMode::Readonly;
            return PermissionMode::Readonly;
        };

        if !can_request_session_permission(&space.default_permission, &session_permission) {
            *session_permission = space.default_permission.clone();
        }

        session_permission.clone()
    }
}

fn build_snapshot(
    spaces: Vec<KnowledgeSpace>,
    active_space_id: String,
    active_scope: ChatScope,
    session_permission: PermissionMode,
    files: Vec<crate::models::KnowledgeFile>,
) -> WorkbenchSnapshot {
    let has_spaces = !spaces.is_empty();
    let has_files = !files.is_empty();
    let first_file_name = files
        .first()
        .map(|file| file.name.clone())
        .unwrap_or_else(|| "暂无来源文件".to_string());

    WorkbenchSnapshot {
        spaces,
        active_space_id,
        active_scope,
        session_permission,
        files,
        block_preview: KnowledgeBlockPreview {
            id: "block-empty".to_string(),
            title: if has_files {
                "知识块等待解析".to_string()
            } else {
                "暂无知识块".to_string()
            },
            excerpt: if has_files {
                "文件元数据已进入本地数据库，后续解析阶段会生成可检索的知识块。".to_string()
            } else if has_spaces {
                "点击扫描后，支持的文件会先进入本地元数据索引。".to_string()
            } else {
                "请先添加一个真实文件夹作为知识库。".to_string()
            },
            source_file_name: first_file_name,
        },
        table_preview: TableInsightPreview {
            id: "table-empty".to_string(),
            title: "表格理解等待接入".to_string(),
            description: "本阶段先完成文件扫描入库，表格结构理解将在后续解析阶段接入。".to_string(),
        },
        messages: vec![ChatMessage {
            id: "msg-system-ready".to_string(),
            role: ChatRole::System,
            content: if has_spaces {
                "当前已使用本地 SQLite 读取真实知识库状态。".to_string()
            } else {
                "请点击新建选择一个真实文件夹。".to_string()
            },
        }],
        pending_action: None,
    }
}

fn validate_folder_path(path: &Path) -> Result<(), AppError> {
    if path.as_os_str().is_empty() {
        return Err(AppError::Filesystem("请选择有效文件夹".to_string()));
    }

    if !path.exists() {
        return Err(AppError::Filesystem("文件夹不存在".to_string()));
    }

    if !path.is_dir() {
        return Err(AppError::Filesystem("请选择文件夹而不是文件".to_string()));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::AppState;
    use crate::models::PermissionMode;
    use crate::storage::sqlite::SqliteStore;

    #[test]
    fn snapshot_starts_empty_without_mock_spaces() {
        let state = AppState::new(SqliteStore::open_in_memory().expect("sqlite opens"));
        let snapshot = state.snapshot().expect("snapshot builds");

        assert!(snapshot.spaces.is_empty());
        assert!(snapshot.files.is_empty());
        assert_eq!(snapshot.active_space_id, "");
        assert_eq!(snapshot.session_permission, PermissionMode::Readonly);
        assert!(snapshot.pending_action.is_none());
    }

    #[test]
    fn creates_scans_and_updates_real_space_state() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        fs::write(temp_dir.path().join("README.md"), "hello").expect("write md");
        fs::write(temp_dir.path().join("image.png"), "skip").expect("write unsupported");
        let state = AppState::new(SqliteStore::open_in_memory().expect("sqlite opens"));
        let created = state
            .create_knowledge_space(
                "测试知识库".to_string(),
                temp_dir.path().to_string_lossy().to_string(),
                PermissionMode::Approval,
            )
            .expect("space created");

        assert_eq!(created.spaces.len(), 1);
        assert!(created.files.is_empty());
        assert!(created.pending_action.is_none());

        let scanned = state
            .scan_knowledge_space(created.active_space_id)
            .expect("space scanned");

        assert_eq!(scanned.files.len(), 1);
        assert_eq!(scanned.files[0].name, "README.md");
        assert_eq!(scanned.files[0].status_label, "待解析");
    }

    #[test]
    fn canonicalizes_folder_path_before_insert() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let state = AppState::new(SqliteStore::open_in_memory().expect("sqlite opens"));
        state
            .create_knowledge_space(
                "测试知识库".to_string(),
                temp_dir.path().to_string_lossy().to_string(),
                PermissionMode::Approval,
            )
            .expect("space created");

        let duplicate = state.create_knowledge_space(
            "重复知识库".to_string(),
            temp_dir.path().join(".").to_string_lossy().to_string(),
            PermissionMode::Approval,
        );

        assert!(duplicate.is_err());
    }

    #[test]
    fn default_permission_change_can_limit_session_permission() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let state = AppState::new(SqliteStore::open_in_memory().expect("sqlite opens"));
        let created = state
            .create_knowledge_space(
                "完全访问空间".to_string(),
                temp_dir.path().to_string_lossy().to_string(),
                PermissionMode::Full,
            )
            .expect("space created");
        state
            .request_session_permission(PermissionMode::Full)
            .expect("full session allowed");

        let updated = state
            .update_default_permission(created.active_space_id, PermissionMode::Readonly)
            .expect("permission updated");

        assert_eq!(
            updated.spaces[0].default_permission,
            PermissionMode::Readonly
        );
        assert_eq!(updated.session_permission, PermissionMode::Readonly);
    }
}
