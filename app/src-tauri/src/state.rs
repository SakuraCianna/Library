use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::error::AppError;
use crate::models::{
    can_request_session_permission, ChatMessage, ChatRole, ChatScope, KnowledgeBlockPreview,
    KnowledgeSpace, OcrSidecarRequest, OcrSidecarResult, PermissionMode, ScanSummary,
    TableInsightPreview, WorkbenchSnapshot,
};
use crate::ocr::{build_ocr_document, build_ocr_request, validate_ocr_inputs};
use crate::parser::parse_file;
use crate::runtime::ocr_config;
use crate::scanner::scan_folder;
use crate::storage::sqlite::SqliteStore;

pub struct AppState {
    store: Mutex<SqliteStore>,
    app_data_dir: PathBuf,
    active_space_id: Mutex<Option<String>>,
    active_scope: Mutex<ChatScope>,
    session_permission: Mutex<PermissionMode>,
    messages: Mutex<Vec<ChatMessage>>,
}

impl AppState {
    pub fn open(app_data_dir: PathBuf) -> Result<Self, AppError> {
        fs::create_dir_all(&app_data_dir)
            .map_err(|error| AppError::Filesystem(format!("无法创建应用数据目录：{}", error)))?;
        let db_path = app_data_dir.join("library.sqlite3");
        let store = SqliteStore::open(&db_path)
            .map_err(|error| AppError::Storage(format!("无法打开本地数据库：{error}")))?;

        Ok(Self::new_with_app_data_dir(store, app_data_dir))
    }

    #[cfg(test)]
    pub fn new(store: SqliteStore) -> Self {
        let app_data_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::new_with_app_data_dir(store, app_data_dir)
    }

    pub fn new_with_app_data_dir(store: SqliteStore, app_data_dir: PathBuf) -> Self {
        Self {
            store: Mutex::new(store),
            app_data_dir,
            active_space_id: Mutex::new(None),
            active_scope: Mutex::new(ChatScope::CurrentFolder),
            session_permission: Mutex::new(PermissionMode::Readonly),
            messages: Mutex::new(Vec::new()),
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
        let latest_block = match active_space.as_ref() {
            Some(space) => store
                .latest_knowledge_block(&space.id)
                .map_err(|error| AppError::Storage(error.to_string()))?,
            None => None,
        };
        let parse_jobs = match active_space.as_ref() {
            Some(space) => store
                .list_parse_jobs(&space.id)
                .map_err(|error| AppError::Storage(error.to_string()))?,
            None => Vec::new(),
        };
        let messages = self
            .messages
            .lock()
            .expect("messages mutex poisoned")
            .clone();

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
            parse_jobs,
            latest_block,
            messages,
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

    pub fn index_knowledge_space(&self, space_id: String) -> Result<WorkbenchSnapshot, AppError> {
        let root_path = {
            let store = self.store.lock().expect("sqlite store mutex poisoned");
            store
                .get_space_root(&space_id)
                .map_err(|error| AppError::Storage(error.to_string()))?
                .ok_or_else(|| AppError::Storage("找不到要索引的知识库".to_string()))?
                .root_path
        };
        let candidates = {
            let store = self.store.lock().expect("sqlite store mutex poisoned");
            store
                .list_parse_candidates(&space_id)
                .map_err(|error| AppError::Storage(format!("无法读取待解析文件：{error}")))?
        };
        let mut indexed_count = 0_u32;
        let mut failed_count = 0_u32;

        for candidate in candidates {
            match parse_file(Path::new(&root_path), &candidate) {
                Ok(document) => {
                    let mut store = self.store.lock().expect("sqlite store mutex poisoned");
                    store
                        .replace_file_knowledge_block(&space_id, &candidate.file_id, &document)
                        .map_err(|error| AppError::Storage(format!("无法保存解析结果：{error}")))?;
                    indexed_count += 1;
                }
                Err(_) => {
                    let store = self.store.lock().expect("sqlite store mutex poisoned");
                    store
                        .mark_file_parse_failed(&candidate.file_id)
                        .map_err(|error| {
                            AppError::Storage(format!("无法记录解析失败状态：{error}"))
                        })?;
                    failed_count += 1;
                }
            }
        }

        *self
            .active_space_id
            .lock()
            .expect("active space mutex poisoned") = Some(space_id);
        self.push_system_message(format!(
            "索引/摘要完成：成功 {} 个，失败 {} 个。",
            indexed_count, failed_count
        ));
        self.snapshot()
    }

    pub fn enqueue_ocr_parse_job(
        &self,
        space_id: String,
        file_id: String,
    ) -> Result<WorkbenchSnapshot, AppError> {
        let (root_path, candidate) = {
            let store = self.store.lock().expect("sqlite store mutex poisoned");
            let root_path = store
                .get_space_root(&space_id)
                .map_err(|error| AppError::Storage(error.to_string()))?
                .ok_or_else(|| AppError::Storage("找不到要排队 OCR 的知识库".to_string()))?
                .root_path;
            let candidate = store
                .get_file_parse_candidate(&space_id, &file_id)
                .map_err(|error| AppError::Storage(format!("无法读取待 OCR 文件：{error}")))?
                .ok_or_else(|| AppError::Storage("找不到要排队 OCR 的文件".to_string()))?;

            (root_path, candidate)
        };
        if !is_ocr_supported_extension(&candidate.extension) {
            return Err(AppError::Storage(
                "当前 OCR 队列仅支持 PDF 文件".to_string(),
            ));
        }
        let config = ocr_config(&self.app_data_dir);
        let input_path = Path::new(&root_path).join(&candidate.relative_path);
        validate_ocr_inputs(&input_path, &config.model_dir, &config.tier)?;
        let request = build_ocr_request(&input_path, &config.model_dir, &config.tier);

        {
            let store = self.store.lock().expect("sqlite store mutex poisoned");
            store
                .enqueue_parse_job(&space_id, &candidate.file_id, "ocr")
                .map_err(|error| AppError::Storage(format!("无法创建 OCR 解析任务：{error}")))?;
        }

        *self
            .active_space_id
            .lock()
            .expect("active space mutex poisoned") = Some(space_id);
        self.push_system_message(format!(
            "OCR 解析任务已排队：{}（{}）。",
            display_relative_file_name(&candidate.relative_path),
            request.tier
        ));
        self.snapshot()
    }

    pub fn cancel_parse_job(&self, job_id: String) -> Result<WorkbenchSnapshot, AppError> {
        let cancelled = {
            let store = self.store.lock().expect("sqlite store mutex poisoned");
            store
                .cancel_parse_job(&job_id)
                .map_err(|error| AppError::Storage(format!("无法取消解析任务：{error}")))?
        };
        if !cancelled {
            return Err(AppError::Storage("找不到可取消的排队任务".to_string()));
        }

        self.push_system_message("解析任务已取消。".to_string());
        self.snapshot()
    }

    pub fn run_next_ocr_parse_job(
        &self,
        space_id: String,
        resource_script_path: Option<PathBuf>,
    ) -> Result<WorkbenchSnapshot, AppError> {
        self.run_next_ocr_parse_job_with_runner(space_id, move |request| {
            crate::ocr::run_ocr_sidecar(request, resource_script_path.as_deref())
        })
    }

    pub fn run_next_ocr_parse_job_with_runner<F>(
        &self,
        space_id: String,
        runner: F,
    ) -> Result<WorkbenchSnapshot, AppError>
    where
        F: Fn(&OcrSidecarRequest) -> Result<OcrSidecarResult, AppError>,
    {
        let (root_path, candidate) = {
            let store = self.store.lock().expect("sqlite store mutex poisoned");
            let root_path = store
                .get_space_root(&space_id)
                .map_err(|error| AppError::Storage(error.to_string()))?
                .ok_or_else(|| AppError::Storage("找不到要执行 OCR 的知识库".to_string()))?
                .root_path;
            let candidate = store
                .next_queued_parse_job(&space_id, "ocr")
                .map_err(|error| AppError::Storage(format!("无法读取 OCR 队列：{error}")))?;

            (root_path, candidate)
        };
        let Some(candidate) = candidate else {
            self.push_system_message("没有待执行的 OCR 任务。".to_string());
            return self.snapshot();
        };
        if !is_ocr_supported_extension(&candidate.extension) {
            return Err(AppError::Storage(
                "当前 OCR 队列仅支持 PDF 文件".to_string(),
            ));
        }

        let started = {
            let store = self.store.lock().expect("sqlite store mutex poisoned");
            store
                .mark_parse_job_running(&candidate.job_id)
                .map_err(|error| AppError::Storage(format!("无法启动 OCR 任务：{error}")))?
        };
        if !started {
            return Err(AppError::Storage("OCR 任务不再处于待执行状态".to_string()));
        }

        let config = ocr_config(&self.app_data_dir);
        let input_path = Path::new(&root_path).join(&candidate.relative_path);
        let request = build_ocr_request(&input_path, &config.model_dir, &config.tier);
        let run_result = validate_ocr_inputs(&input_path, &config.model_dir, &config.tier)
            .and_then(|_| runner(&request))
            .and_then(|ocr_result| build_ocr_document(&candidate.relative_path, &ocr_result));

        match run_result {
            Ok(document) => {
                let storage_result = {
                    let mut store = self.store.lock().expect("sqlite store mutex poisoned");
                    store
                        .replace_file_knowledge_block(&space_id, &candidate.file_id, &document)
                        .map_err(|error| AppError::Storage(format!("无法保存 OCR 结果：{error}")))
                        .and_then(|_| {
                            store
                                .mark_parse_job_succeeded(&candidate.job_id)
                                .map_err(|error| {
                                    AppError::Storage(format!("无法标记 OCR 成功：{error}"))
                                })
                                .and_then(|updated| {
                                    updated.then_some(()).ok_or_else(|| {
                                        AppError::Storage(
                                            "OCR 任务状态已变化，无法标记成功".to_string(),
                                        )
                                    })
                                })
                        })
                };

                match storage_result {
                    Ok(_) => {
                        self.push_system_message(format!(
                            "OCR 解析完成：{}。",
                            display_relative_file_name(&candidate.relative_path)
                        ));
                    }
                    Err(error) => {
                        self.record_ocr_failure(&candidate.job_id, &candidate.file_id, &error)?;
                        self.push_system_message(format!(
                            "OCR 解析失败：{}。",
                            display_relative_file_name(&candidate.relative_path)
                        ));
                    }
                }
            }
            Err(error) => {
                self.record_ocr_failure(&candidate.job_id, &candidate.file_id, &error)?;
                self.push_system_message(format!(
                    "OCR 解析失败：{}。",
                    display_relative_file_name(&candidate.relative_path)
                ));
            }
        }

        *self
            .active_space_id
            .lock()
            .expect("active space mutex poisoned") = Some(space_id);
        self.snapshot()
    }

    fn record_ocr_failure(
        &self,
        job_id: &str,
        file_id: &str,
        error: &AppError,
    ) -> Result<(), AppError> {
        let message = error.to_string();
        let store = self.store.lock().expect("sqlite store mutex poisoned");
        store
            .mark_file_parse_failed(file_id)
            .map_err(|error| AppError::Storage(format!("无法记录 OCR 文件失败：{error}")))?;
        let failed = store
            .mark_parse_job_failed(job_id, &message)
            .map_err(|error| AppError::Storage(format!("无法记录 OCR 任务失败：{error}")))?;
        if !failed {
            return Err(AppError::Storage(
                "OCR 任务状态已变化，无法标记失败".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn ask_agent(
        &self,
        space_id: String,
        question: String,
    ) -> Result<WorkbenchSnapshot, AppError> {
        let question = question.trim().to_string();
        if question.is_empty() {
            return Err(AppError::Storage("请输入问题".to_string()));
        }

        let hits = {
            let store = self.store.lock().expect("sqlite store mutex poisoned");
            store
                .search_knowledge_blocks(&space_id, &question, 4)
                .map_err(|error| AppError::Storage(format!("检索知识块失败：{error}")))?
        };
        self.push_chat_message(ChatRole::User, question.clone());
        let answer = crate::agent::answer_question(&question, &hits).await;
        self.push_chat_message(ChatRole::Assistant, answer);
        *self
            .active_space_id
            .lock()
            .expect("active space mutex poisoned") = Some(space_id);

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

    fn push_system_message(&self, content: String) {
        self.push_chat_message(ChatRole::System, content);
    }

    fn push_chat_message(&self, role: ChatRole, content: String) {
        let mut messages = self.messages.lock().expect("messages mutex poisoned");
        messages.push(ChatMessage {
            id: format!("msg-{}", uuid::Uuid::new_v4()),
            role,
            content,
        });

        if messages.len() > 24 {
            let overflow = messages.len() - 24;
            messages.drain(0..overflow);
        }
    }
}

fn build_snapshot(
    spaces: Vec<KnowledgeSpace>,
    active_space_id: String,
    active_scope: ChatScope,
    session_permission: PermissionMode,
    files: Vec<crate::models::KnowledgeFile>,
    parse_jobs: Vec<crate::models::ParseJobSummary>,
    latest_block: Option<crate::models::KnowledgeBlockSearchHit>,
    messages: Vec<ChatMessage>,
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
        parse_jobs,
        block_preview: latest_block
            .map(|block| KnowledgeBlockPreview {
                id: block.id,
                title: block.title,
                excerpt: block.excerpt,
                source_file_name: block.source_file_name,
            })
            .unwrap_or_else(|| KnowledgeBlockPreview {
                id: "block-empty".to_string(),
                title: if has_files {
                    "知识块等待解析".to_string()
                } else {
                    "暂无知识块".to_string()
                },
                excerpt: if has_files {
                    "文件元数据已进入本地数据库，点击建索引/摘要后会生成可检索的知识块。"
                        .to_string()
                } else if has_spaces {
                    "点击扫描后，支持的文件会先进入本地元数据索引。".to_string()
                } else {
                    "请先添加一个真实文件夹作为知识库。".to_string()
                },
                source_file_name: first_file_name,
            }),
        table_preview: TableInsightPreview {
            id: "table-empty".to_string(),
            title: "表格理解等待接入".to_string(),
            description: "本阶段先完成文件扫描入库，表格结构理解将在后续解析阶段接入。".to_string(),
        },
        messages: if messages.is_empty() {
            vec![ChatMessage {
                id: "msg-system-ready".to_string(),
                role: ChatRole::System,
                content: if has_spaces {
                    "当前已使用本地 SQLite 读取真实知识库状态。".to_string()
                } else {
                    "请点击新建选择一个真实文件夹。".to_string()
                },
            }]
        } else {
            messages
        },
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

fn display_relative_file_name(relative_path: &str) -> String {
    relative_path
        .rsplit(['\\', '/'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(relative_path)
        .to_string()
}

fn is_ocr_supported_extension(extension: &str) -> bool {
    extension
        .trim_start_matches('.')
        .eq_ignore_ascii_case("pdf")
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, OnceLock};
    use std::{env, fs};

    use super::AppState;
    use crate::models::{ChatRole, OcrPageResult, OcrSidecarResult, PermissionMode};
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
    fn indexes_scanned_files_into_searchable_blocks() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        fs::write(
            temp_dir.path().join("Redis面试.md"),
            "Redis 缓存穿透是查询不存在的数据导致缓存和数据库都无法命中。",
        )
        .expect("write md");
        let state = AppState::new(SqliteStore::open_in_memory().expect("sqlite opens"));
        let created = state
            .create_knowledge_space(
                "面试".to_string(),
                temp_dir.path().to_string_lossy().to_string(),
                PermissionMode::Approval,
            )
            .expect("space created");
        state
            .scan_knowledge_space(created.active_space_id.clone())
            .expect("space scanned");

        let indexed = state
            .index_knowledge_space(created.active_space_id)
            .expect("space indexed");

        assert_eq!(indexed.files[0].status_label, "已索引");
        assert!(indexed.block_preview.excerpt.contains("缓存穿透"));
    }

    #[test]
    fn enqueues_pdf_ocr_job_when_models_are_ready() {
        let knowledge_dir = tempfile::tempdir().expect("knowledge dir");
        fs::write(knowledge_dir.path().join("scan.pdf"), "pdf").expect("write pdf");
        let app_data_dir = tempfile::tempdir().expect("app data dir");
        let model_dir = app_data_dir
            .path()
            .join("models")
            .join("ocr")
            .join("pp-ocrv6");
        create_test_ocr_model(&model_dir);
        let state = AppState::new_with_app_data_dir(
            SqliteStore::open_in_memory().expect("sqlite opens"),
            app_data_dir.path().to_path_buf(),
        );
        let created = state
            .create_knowledge_space(
                "OCR".to_string(),
                knowledge_dir.path().to_string_lossy().to_string(),
                PermissionMode::Approval,
            )
            .expect("space created");
        let scanned = state
            .scan_knowledge_space(created.active_space_id)
            .expect("space scanned");

        let queued = state
            .enqueue_ocr_parse_job(scanned.active_space_id, scanned.files[0].id.clone())
            .expect("ocr job queued");

        assert_eq!(queued.parse_jobs.len(), 1);
        assert_eq!(
            queued.parse_jobs[0].file_id.as_deref(),
            Some(scanned.files[0].id.as_str())
        );
    }

    #[test]
    fn rejects_non_pdf_ocr_jobs_before_queueing() {
        let knowledge_dir = tempfile::tempdir().expect("knowledge dir");
        fs::write(knowledge_dir.path().join("README.md"), "hello").expect("write md");
        let state = AppState::new(SqliteStore::open_in_memory().expect("sqlite opens"));
        let created = state
            .create_knowledge_space(
                "OCR".to_string(),
                knowledge_dir.path().to_string_lossy().to_string(),
                PermissionMode::Approval,
            )
            .expect("space created");
        let scanned = state
            .scan_knowledge_space(created.active_space_id)
            .expect("space scanned");

        let result =
            state.enqueue_ocr_parse_job(scanned.active_space_id, scanned.files[0].id.clone());

        assert!(result
            .expect_err("md file is rejected")
            .to_string()
            .contains("仅支持 PDF"));
    }

    #[test]
    fn runs_next_ocr_job_into_searchable_knowledge_block() {
        let knowledge_dir = tempfile::tempdir().expect("knowledge dir");
        fs::write(knowledge_dir.path().join("scan.pdf"), "pdf").expect("write pdf");
        let app_data_dir = tempfile::tempdir().expect("app data dir");
        let model_dir = app_data_dir
            .path()
            .join("models")
            .join("ocr")
            .join("pp-ocrv6");
        create_test_ocr_model(&model_dir);
        let state = AppState::new_with_app_data_dir(
            SqliteStore::open_in_memory().expect("sqlite opens"),
            app_data_dir.path().to_path_buf(),
        );
        let created = state
            .create_knowledge_space(
                "OCR".to_string(),
                knowledge_dir.path().to_string_lossy().to_string(),
                PermissionMode::Approval,
            )
            .expect("space created");
        let scanned = state
            .scan_knowledge_space(created.active_space_id)
            .expect("space scanned");
        state
            .enqueue_ocr_parse_job(scanned.active_space_id.clone(), scanned.files[0].id.clone())
            .expect("ocr job queued");

        let snapshot = state
            .run_next_ocr_parse_job_with_runner(scanned.active_space_id, |_request| {
                Ok(OcrSidecarResult {
                    text: "扫描版 PDF 的本地 OCR 文本".to_string(),
                    page_count: 1,
                    pages: vec![OcrPageResult {
                        page_index: 0,
                        text: "扫描版 PDF 的本地 OCR 文本".to_string(),
                        confidence: Some(0.93),
                    }],
                })
            })
            .expect("ocr job runs");

        assert_eq!(snapshot.files[0].status_label, "已索引");
        assert_eq!(snapshot.parse_jobs[0].status, "succeeded");
        assert!(snapshot.block_preview.excerpt.contains("OCR 文本"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ocr_knowledge_block_can_answer_agent_question() {
        let _env_lock = env_lock().lock().expect("env lock");
        let _api_key_guard = EnvVarGuard::set("DEEPSEEK_API_KEY", "");
        let knowledge_dir = tempfile::tempdir().expect("knowledge dir");
        fs::write(knowledge_dir.path().join("scan.pdf"), "pdf").expect("write pdf");
        let app_data_dir = tempfile::tempdir().expect("app data dir");
        let model_dir = app_data_dir
            .path()
            .join("models")
            .join("ocr")
            .join("pp-ocrv6");
        create_test_ocr_model(&model_dir);
        let state = AppState::new_with_app_data_dir(
            SqliteStore::open_in_memory().expect("sqlite opens"),
            app_data_dir.path().to_path_buf(),
        );
        let created = state
            .create_knowledge_space(
                "OCR".to_string(),
                knowledge_dir.path().to_string_lossy().to_string(),
                PermissionMode::Approval,
            )
            .expect("space created");
        let scanned = state
            .scan_knowledge_space(created.active_space_id)
            .expect("space scanned");
        state
            .enqueue_ocr_parse_job(scanned.active_space_id.clone(), scanned.files[0].id.clone())
            .expect("ocr job queued");
        let indexed = state
            .run_next_ocr_parse_job_with_runner(scanned.active_space_id, |_request| {
                Ok(OcrSidecarResult {
                    text: "扫描版 PDF 的本地 OCR 文本".to_string(),
                    page_count: 1,
                    pages: vec![OcrPageResult {
                        page_index: 0,
                        text: "扫描版 PDF 的本地 OCR 文本".to_string(),
                        confidence: Some(0.93),
                    }],
                })
            })
            .expect("ocr job runs");

        let answered = state
            .ask_agent(indexed.active_space_id, "扫描版".to_string())
            .await
            .expect("agent answers from local index");

        assert!(answered.messages.iter().any(|message| {
            message.role == ChatRole::Assistant && message.content.contains("本地 OCR 文本")
        }));
    }

    #[test]
    fn records_ocr_job_failure_without_indexing_file() {
        let knowledge_dir = tempfile::tempdir().expect("knowledge dir");
        fs::write(knowledge_dir.path().join("scan.pdf"), "pdf").expect("write pdf");
        let app_data_dir = tempfile::tempdir().expect("app data dir");
        let model_dir = app_data_dir
            .path()
            .join("models")
            .join("ocr")
            .join("pp-ocrv6");
        create_test_ocr_model(&model_dir);
        let state = AppState::new_with_app_data_dir(
            SqliteStore::open_in_memory().expect("sqlite opens"),
            app_data_dir.path().to_path_buf(),
        );
        let created = state
            .create_knowledge_space(
                "OCR".to_string(),
                knowledge_dir.path().to_string_lossy().to_string(),
                PermissionMode::Approval,
            )
            .expect("space created");
        let scanned = state
            .scan_knowledge_space(created.active_space_id)
            .expect("space scanned");
        state
            .enqueue_ocr_parse_job(scanned.active_space_id.clone(), scanned.files[0].id.clone())
            .expect("ocr job queued");

        let snapshot = state
            .run_next_ocr_parse_job_with_runner(scanned.active_space_id, |_request| {
                Err(crate::error::AppError::Filesystem(
                    "OCR_EMPTY_RESULT".to_string(),
                ))
            })
            .expect("failure is recorded in snapshot");

        assert_eq!(snapshot.files[0].status_label, "扫描失败");
        assert_eq!(snapshot.parse_jobs[0].status, "failed");
        assert_eq!(
            snapshot.parse_jobs[0].error_message.as_deref(),
            Some("文件系统错误：OCR_EMPTY_RESULT")
        );
    }

    fn create_test_ocr_model(model_dir: &std::path::Path) {
        for model_name in ["PP-OCRv6_medium_det", "PP-OCRv6_medium_rec"] {
            let path = model_dir.join(model_name);
            fs::create_dir_all(&path).expect("model dir");
            for file_name in ["inference.json", "inference.pdiparams", "inference.yml"] {
                fs::write(path.join(file_name), "model").expect("model file");
            }
        }
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = env::var_os(key);
            env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(previous) = self.previous.take() {
                env::set_var(self.key, previous);
            } else {
                env::remove_var(self.key);
            }
        }
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
