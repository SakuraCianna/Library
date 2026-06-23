use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use time::OffsetDateTime;

use crate::error::AppError;
use crate::models::{
    can_request_session_permission, BackupExport, BackupExportResult, BackupRestorePreflight,
    BackupRestoreResult, ChatMessage, ChatMessageSource, ChatRole, ChatScope,
    KnowledgeBlockContext, KnowledgeBlockPreview, KnowledgeBlockSearchHit, KnowledgeSpace,
    OcrSidecarRequest, OcrSidecarResult, ParseJobCandidate, PermissionMode, ScanSummary,
    ScannedFile, TableInsightPreview, WorkbenchSnapshot,
};
use crate::ocr::{
    build_ocr_document, prepare_docx_embedded_image_ocr_request, prepare_file_ocr_request,
    PreparedOcrRequest,
};
use crate::parser::parse_file_with_sidecar;
use crate::runtime::ocr_config;
use crate::scanner::{scan_folder_with_progress, ScanProgress};
use crate::storage::sqlite::{ParseJobWriteOutcome, ScanJobWriteOutcome, SqliteStore};

pub struct AppState {
    store: Mutex<SqliteStore>,
    app_data_dir: PathBuf,
    active_space_id: Mutex<Option<String>>,
    active_scope: Mutex<ChatScope>,
    session_permission: Mutex<PermissionMode>,
    messages: Mutex<Vec<ChatMessage>>,
    active_scan_workers: Mutex<HashSet<String>>,
    active_document_workers: Mutex<HashSet<String>>,
    active_ocr_workers: Mutex<HashSet<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ScanJobOutcome {
    NoQueuedJob,
    Succeeded {
        summary: ScanSummary,
        queued_document_count: u32,
        queued_ocr_count: u32,
    },
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DocumentJobOutcome {
    NoQueuedJob,
    Succeeded(String),
    Failed(String),
    Cancelled(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OcrJobOutcome {
    NoQueuedJob,
    Succeeded(String),
    Failed(String),
    Cancelled(String),
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

    fn prepare_ocr_request_for_candidate(
        root_path: &Path,
        relative_path: &str,
        extension: &str,
        source_locator: Option<&str>,
        model_dir: &Path,
        tier: &str,
    ) -> Result<PreparedOcrRequest, AppError> {
        let input_path = root_path.join(relative_path);
        if let Some(source_locator) = source_locator {
            Self::validate_embedded_image_source_locator(relative_path, extension, source_locator)?;
            return prepare_docx_embedded_image_ocr_request(
                &input_path,
                source_locator,
                model_dir,
                tier,
            );
        }

        if !is_ocr_supported_extension(extension) {
            return Err(AppError::Storage(
                "当前 OCR 队列仅支持 PDF、图片文件或 DOCX 文档图片".to_string(),
            ));
        }

        prepare_file_ocr_request(&input_path, relative_path, model_dir, tier)
    }

    fn validate_embedded_image_source_locator(
        relative_path: &str,
        extension: &str,
        source_locator: &str,
    ) -> Result<(), AppError> {
        if extension.trim_start_matches('.').to_lowercase() != "docx" {
            return Err(AppError::Storage(
                "只有 DOCX 文档图片支持内嵌图片 OCR".to_string(),
            ));
        }
        if Self::embedded_image_number_from_locator(source_locator).is_none() {
            return Err(AppError::Storage(
                "DOCX 文档图片 OCR 需要 #image-N 来源定位".to_string(),
            ));
        }

        let source_path = source_locator_to_relative_path(source_locator)?;
        let expected = PathBuf::from(relative_path.replace('/', std::path::MAIN_SEPARATOR_STR));
        if source_path != expected {
            return Err(AppError::PermissionDenied(
                "OCR 来源定位不属于当前文件".to_string(),
            ));
        }

        Ok(())
    }

    fn display_ocr_job_name(candidate: &ParseJobCandidate) -> String {
        if let Some(source_locator) = candidate.source_locator.as_deref() {
            if let Some(image_number) = Self::embedded_image_number_from_locator(source_locator) {
                return format!(
                    "{} · 文档图片 {}",
                    display_relative_file_name(&candidate.relative_path),
                    image_number
                );
            }
        }

        display_relative_file_name(&candidate.relative_path)
    }

    fn embedded_image_number_from_locator(source_locator: &str) -> Option<u32> {
        source_locator.split('#').skip(1).find_map(|fragment| {
            let number = fragment.strip_prefix("image-")?;
            if number.is_empty() || !number.chars().all(|character| character.is_ascii_digit()) {
                return None;
            }
            number.parse::<u32>().ok()
        })
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
            active_scan_workers: Mutex::new(HashSet::new()),
            active_document_workers: Mutex::new(HashSet::new()),
            active_ocr_workers: Mutex::new(HashSet::new()),
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
        let latest_table = match active_space.as_ref() {
            Some(space) => store
                .latest_table_insight(&space.id)
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
            latest_table,
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

    pub fn resolve_source_file_path(
        &self,
        space_id: &str,
        source_locator: &str,
    ) -> Result<PathBuf, AppError> {
        let relative_path = source_locator_to_relative_path(source_locator)?;
        let root_path = {
            let store = self.store.lock().expect("sqlite store mutex poisoned");
            store
                .get_space_root(space_id)
                .map_err(|error| AppError::Storage(error.to_string()))?
                .ok_or_else(|| AppError::Storage("找不到来源所属知识库".to_string()))?
                .root_path
        };
        let root = PathBuf::from(root_path)
            .canonicalize()
            .map_err(|error| AppError::Filesystem(format!("无法规范化知识库目录：{error}")))?;
        let source_path = root.join(relative_path);
        let source_path = source_path
            .canonicalize()
            .map_err(|error| AppError::Filesystem(format!("无法读取来源文件：{error}")))?;

        if !source_path.starts_with(&root) {
            return Err(AppError::PermissionDenied(
                "来源文件不在当前知识库目录内".to_string(),
            ));
        }

        if !source_path.is_file() {
            return Err(AppError::Filesystem("来源定位不是可打开文件".to_string()));
        }

        Ok(source_path)
    }

    pub fn knowledge_block_context(
        &self,
        space_id: String,
        block_id: String,
    ) -> Result<KnowledgeBlockContext, AppError> {
        let store = self.store.lock().expect("sqlite store mutex poisoned");
        store
            .knowledge_block_context(&space_id, &block_id)
            .map_err(|error| AppError::Storage(format!("无法读取来源上下文：{error}")))?
            .ok_or_else(|| AppError::Storage("找不到来源知识块".to_string()))
    }

    pub fn prepare_scan_knowledge_space(&self, space_id: String) -> Result<bool, AppError> {
        let inserted = {
            let store = self.store.lock().expect("sqlite store mutex poisoned");
            store
                .get_space_root(&space_id)
                .map_err(|error| AppError::Storage(error.to_string()))?
                .ok_or_else(|| AppError::Storage("找不到要扫描的知识库".to_string()))?;
            store
                .enqueue_space_parse_job_with_status(&space_id, "scan")
                .map_err(|error| AppError::Storage(format!("无法创建扫描任务：{error}")))?
                .inserted
        };

        if inserted {
            self.push_system_message("文件夹扫描任务已排队。".to_string());
        }

        self.begin_scan_worker(space_id)
    }

    #[cfg(test)]
    pub fn scan_knowledge_space(&self, space_id: String) -> Result<WorkbenchSnapshot, AppError> {
        let _ = self.prepare_scan_knowledge_space(space_id.clone())?;
        let outcome = self.run_one_scan_job_with_scanner(&space_id, |root_path, _job_id| {
            crate::scanner::scan_folder(root_path).map_err(scan_filesystem_error)
        })?;
        self.finish_scan_worker(&space_id);

        match outcome {
            ScanJobOutcome::Succeeded { .. } | ScanJobOutcome::NoQueuedJob => self.snapshot(),
            ScanJobOutcome::Cancelled => Err(AppError::Storage("扫描任务已取消".to_string())),
            ScanJobOutcome::Failed => Err(AppError::Storage("扫描任务失败".to_string())),
        }
    }

    pub fn begin_scan_worker(&self, space_id: String) -> Result<bool, AppError> {
        let worker_space_id = space_id.clone();
        let has_queued_job = {
            let store = self.store.lock().expect("sqlite store mutex poisoned");
            store
                .get_space_root(&space_id)
                .map_err(|error| AppError::Storage(error.to_string()))?
                .ok_or_else(|| AppError::Storage("找不到要扫描的知识库".to_string()))?;
            store
                .has_queued_parse_job(&space_id, "scan")
                .map_err(|error| AppError::Storage(format!("无法读取扫描队列：{error}")))?
        };

        *self
            .active_space_id
            .lock()
            .expect("active space mutex poisoned") = Some(space_id);

        if !has_queued_job {
            self.push_system_message("没有待执行的扫描任务。".to_string());
            return Ok(false);
        }

        let mut workers = self
            .active_scan_workers
            .lock()
            .expect("scan worker mutex poisoned");
        if !workers.insert(worker_space_id) {
            drop(workers);
            self.push_system_message("扫描后台任务已在运行。".to_string());
            return Ok(false);
        }
        drop(workers);

        self.push_system_message("扫描后台任务已启动。".to_string());
        Ok(true)
    }

    pub fn run_scan_worker<F>(&self, space_id: String, mut notify: F)
    where
        F: FnMut(&str),
    {
        loop {
            let outcome = self.run_one_scan_job_with_scanner(&space_id, |root_path, job_id| {
                scan_folder_with_progress(root_path, |progress| {
                    if self.is_parse_job_cancelled(job_id).unwrap_or(false) {
                        return false;
                    }

                    if self.update_scan_progress(job_id, progress).is_ok() {
                        notify("scan-progress");
                    }
                    true
                })
                .map_err(scan_filesystem_error)
            });

            match outcome {
                Ok(ScanJobOutcome::NoQueuedJob) => {
                    self.push_system_message("扫描后台队列已清空。".to_string());
                    notify("scan-drained");
                    break;
                }
                Ok(outcome) => {
                    self.push_scan_outcome_message(&outcome);
                    notify("scan-state-changed");
                }
                Err(error) => {
                    self.push_system_message(format!("扫描后台任务中断：{error}"));
                    notify("scan-interrupted");
                    break;
                }
            }
        }

        self.finish_scan_worker(&space_id);
        notify("scan-worker-finished");
    }

    #[cfg(test)]
    fn run_next_scan_job_with_scanner<F>(
        &self,
        space_id: String,
        scanner: F,
    ) -> Result<ScanJobOutcome, AppError>
    where
        F: FnOnce(&Path, &str) -> Result<Vec<ScannedFile>, AppError>,
    {
        let outcome = self.run_one_scan_job_with_scanner(&space_id, scanner)?;
        *self
            .active_space_id
            .lock()
            .expect("active space mutex poisoned") = Some(space_id);
        Ok(outcome)
    }

    fn run_one_scan_job_with_scanner<F>(
        &self,
        space_id: &str,
        scanner: F,
    ) -> Result<ScanJobOutcome, AppError>
    where
        F: FnOnce(&Path, &str) -> Result<Vec<ScannedFile>, AppError>,
    {
        let (root_path, job_id) = {
            let mut store = self.store.lock().expect("sqlite store mutex poisoned");
            let root_path = store
                .get_space_root(space_id)
                .map_err(|error| AppError::Storage(error.to_string()))?
                .ok_or_else(|| AppError::Storage("找不到要扫描的知识库".to_string()))?
                .root_path;
            let job_id = store
                .claim_next_queued_space_parse_job(space_id, "scan")
                .map_err(|error| AppError::Storage(format!("无法领取扫描任务：{error}")))?;

            (root_path, job_id)
        };
        let Some(job_id) = job_id else {
            return Ok(ScanJobOutcome::NoQueuedJob);
        };

        self.update_parse_progress(&job_id, "正在遍历文件夹", 0, 0)?;
        let root_path = Path::new(&root_path);
        let run_result = scanner(root_path, &job_id);

        match run_result {
            Ok(scanned_files) => {
                let outcome = {
                    let mut store = self.store.lock().expect("sqlite store mutex poisoned");
                    match store
                        .complete_scan_job_if_running(space_id, &job_id, &scanned_files)
                        .map_err(|error| AppError::Storage(format!("无法保存扫描结果：{error}")))?
                    {
                        ScanJobWriteOutcome::Updated {
                            summary,
                            queued_document_count,
                            queued_ocr_count,
                        } => ScanJobOutcome::Succeeded {
                            summary,
                            queued_document_count,
                            queued_ocr_count,
                        },
                        ScanJobWriteOutcome::Cancelled => ScanJobOutcome::Cancelled,
                        ScanJobWriteOutcome::NotRunning => {
                            return Err(AppError::Storage(
                                "扫描任务状态已变化，无法标记成功".to_string(),
                            ));
                        }
                    }
                };

                Ok(outcome)
            }
            Err(error) => {
                if self.is_parse_job_cancelled(&job_id)? {
                    return Ok(ScanJobOutcome::Cancelled);
                }

                if self.record_space_parse_failure(&job_id, &error)? {
                    Ok(ScanJobOutcome::Failed)
                } else {
                    Ok(ScanJobOutcome::Cancelled)
                }
            }
        }
    }

    pub fn prepare_document_indexing(&self, space_id: String) -> Result<bool, AppError> {
        let queued_count = {
            let store = self.store.lock().expect("sqlite store mutex poisoned");
            store
                .get_space_root(&space_id)
                .map_err(|error| AppError::Storage(error.to_string()))?
                .ok_or_else(|| AppError::Storage("找不到要索引的知识库".to_string()))?;
            store
                .enqueue_document_parse_jobs(&space_id)
                .map_err(|error| AppError::Storage(format!("无法创建文档解析任务：{error}")))?
        };

        if queued_count > 0 {
            self.push_system_message(format!("已排队 {} 个文档解析任务。", queued_count));
        }

        self.begin_document_worker(space_id)
    }

    pub fn begin_document_worker(&self, space_id: String) -> Result<bool, AppError> {
        let worker_space_id = space_id.clone();
        let has_queued_job = {
            let store = self.store.lock().expect("sqlite store mutex poisoned");
            store
                .get_space_root(&space_id)
                .map_err(|error| AppError::Storage(error.to_string()))?
                .ok_or_else(|| AppError::Storage("找不到要建索引/摘要的知识库".to_string()))?;
            store
                .has_queued_parse_job(&space_id, "document")
                .map_err(|error| AppError::Storage(format!("无法读取文档解析队列：{error}")))?
        };

        *self
            .active_space_id
            .lock()
            .expect("active space mutex poisoned") = Some(space_id);

        if !has_queued_job {
            self.push_system_message("没有待建索引/摘要的文件。".to_string());
            return Ok(false);
        }

        let mut workers = self
            .active_document_workers
            .lock()
            .expect("document worker mutex poisoned");
        if !workers.insert(worker_space_id) {
            drop(workers);
            self.push_system_message("文档解析后台任务已在运行。".to_string());
            return Ok(false);
        }
        drop(workers);

        self.push_system_message("文档解析后台任务已启动。".to_string());
        Ok(true)
    }

    pub fn run_document_worker<F>(
        &self,
        space_id: String,
        resource_script_path: Option<PathBuf>,
        mut notify: F,
    ) where
        F: FnMut(&str),
    {
        loop {
            let outcome = self.run_one_document_parse_job_with_parser_notifying(
                &space_id,
                |root_path, candidate| {
                    let file_candidate = crate::models::FileParseCandidate {
                        file_id: candidate.file_id.clone(),
                        relative_path: candidate.relative_path.clone(),
                        extension: candidate.extension.clone(),
                    };
                    parse_file_with_sidecar(
                        root_path,
                        &file_candidate,
                        resource_script_path.as_deref(),
                    )
                },
                &mut notify,
            );

            match outcome {
                Ok(DocumentJobOutcome::NoQueuedJob) => {
                    self.push_system_message("文档解析后台队列已清空。".to_string());
                    notify("document-drained");
                    break;
                }
                Ok(outcome) => {
                    self.push_document_outcome_message(&outcome);
                    notify("document-state-changed");
                }
                Err(error) => {
                    self.push_system_message(format!("文档解析后台任务中断：{error}"));
                    notify("document-interrupted");
                    break;
                }
            }
        }

        self.finish_document_worker(&space_id);
        notify("document-worker-finished");
    }

    #[cfg(test)]
    fn run_next_document_parse_job_with_parser<F>(
        &self,
        space_id: String,
        parser: F,
    ) -> Result<DocumentJobOutcome, AppError>
    where
        F: FnOnce(&Path, &ParseJobCandidate) -> Result<crate::models::ParsedDocument, AppError>,
    {
        let outcome = self.run_one_document_parse_job_with_parser(&space_id, parser)?;
        *self
            .active_space_id
            .lock()
            .expect("active space mutex poisoned") = Some(space_id);
        Ok(outcome)
    }

    #[cfg(test)]
    fn run_one_document_parse_job_with_parser<F>(
        &self,
        space_id: &str,
        parser: F,
    ) -> Result<DocumentJobOutcome, AppError>
    where
        F: FnOnce(&Path, &ParseJobCandidate) -> Result<crate::models::ParsedDocument, AppError>,
    {
        self.run_one_document_parse_job_with_parser_notifying(space_id, parser, |_| {})
    }

    fn run_one_document_parse_job_with_parser_notifying<F, N>(
        &self,
        space_id: &str,
        parser: F,
        mut notify: N,
    ) -> Result<DocumentJobOutcome, AppError>
    where
        F: FnOnce(&Path, &ParseJobCandidate) -> Result<crate::models::ParsedDocument, AppError>,
        N: FnMut(&str),
    {
        let (root_path, candidate) = {
            let mut store = self.store.lock().expect("sqlite store mutex poisoned");
            let root_path = store
                .get_space_root(space_id)
                .map_err(|error| AppError::Storage(error.to_string()))?
                .ok_or_else(|| AppError::Storage("找不到要建索引/摘要的知识库".to_string()))?
                .root_path;
            let candidate = store
                .claim_next_queued_parse_job(space_id, "document")
                .map_err(|error| AppError::Storage(format!("无法领取文档解析任务：{error}")))?;

            (root_path, candidate)
        };
        let Some(candidate) = candidate else {
            return Ok(DocumentJobOutcome::NoQueuedJob);
        };
        let file_name = display_relative_file_name(&candidate.relative_path);

        let root_path = Path::new(&root_path);
        let run_result = (|| {
            self.update_parse_progress(&candidate.job_id, "正在解析文档", 0, 2)?;
            notify("document-progress");
            let document = parser(root_path, &candidate)?;
            self.update_parse_progress(&candidate.job_id, "正在写入索引", 2, 2)?;
            notify("document-progress");
            Ok(document)
        })();

        match run_result {
            Ok(document) => {
                let outcome = {
                    let mut store = self.store.lock().expect("sqlite store mutex poisoned");
                    match store
                        .complete_parse_job_if_running(
                            space_id,
                            &candidate.file_id,
                            &candidate.job_id,
                            &document,
                        )
                        .map_err(|error| {
                            AppError::Storage(format!("无法保存文档解析结果：{error}"))
                        })? {
                        ParseJobWriteOutcome::Updated => DocumentJobOutcome::Succeeded(file_name),
                        ParseJobWriteOutcome::Cancelled => DocumentJobOutcome::Cancelled(file_name),
                        ParseJobWriteOutcome::NotRunning => {
                            return Err(AppError::Storage(
                                "文档解析任务状态已变化，无法标记成功".to_string(),
                            ));
                        }
                    }
                };

                Ok(outcome)
            }
            Err(error) => {
                if self.is_parse_job_cancelled(&candidate.job_id)? {
                    return Ok(DocumentJobOutcome::Cancelled(file_name));
                }

                if self.record_parse_failure(&candidate.job_id, &candidate.file_id, &error)? {
                    Ok(DocumentJobOutcome::Failed(file_name))
                } else {
                    Ok(DocumentJobOutcome::Cancelled(file_name))
                }
            }
        }
    }

    pub fn enqueue_ocr_parse_job(
        &self,
        space_id: String,
        file_id: String,
        source_locator: Option<String>,
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
        let source_locator = source_locator.and_then(|locator| {
            let trimmed = locator.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        });
        let config = ocr_config(&self.app_data_dir);
        let prepared = Self::prepare_ocr_request_for_candidate(
            Path::new(&root_path),
            &candidate.relative_path,
            &candidate.extension,
            source_locator.as_deref(),
            &config.model_dir,
            &config.tier,
        )?;

        {
            let store = self.store.lock().expect("sqlite store mutex poisoned");
            store
                .enqueue_parse_job_for_source_with_status(
                    &space_id,
                    &candidate.file_id,
                    "ocr",
                    source_locator.as_deref(),
                )
                .map_err(|error| AppError::Storage(format!("无法创建 OCR 解析任务：{error}")))?;
        }

        *self
            .active_space_id
            .lock()
            .expect("active space mutex poisoned") = Some(space_id);
        self.push_system_message(format!(
            "OCR 解析任务已排队：{}（{}）。",
            display_relative_file_name(&candidate.relative_path),
            prepared.request().tier
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
            return Err(AppError::Storage("找不到可取消的解析任务".to_string()));
        }

        self.push_system_message("解析任务已取消。".to_string());
        self.snapshot()
    }

    pub fn begin_ocr_worker(&self, space_id: String) -> Result<bool, AppError> {
        {
            let store = self.store.lock().expect("sqlite store mutex poisoned");
            store
                .get_space_root(&space_id)
                .map_err(|error| AppError::Storage(error.to_string()))?
                .ok_or_else(|| AppError::Storage("找不到要执行 OCR 的知识库".to_string()))?;
        }

        *self
            .active_space_id
            .lock()
            .expect("active space mutex poisoned") = Some(space_id.clone());

        let mut workers = self
            .active_ocr_workers
            .lock()
            .expect("ocr worker mutex poisoned");
        if !workers.insert(space_id) {
            drop(workers);
            self.push_system_message("OCR 后台任务已在运行。".to_string());
            return Ok(false);
        }
        drop(workers);

        self.push_system_message("OCR 后台任务已启动。".to_string());
        Ok(true)
    }

    pub fn run_ocr_worker<F>(
        &self,
        space_id: String,
        resource_script_path: Option<PathBuf>,
        mut notify: F,
    ) where
        F: FnMut(&str),
    {
        loop {
            let outcome = self.run_one_ocr_parse_job_with_runner_notifying(
                &space_id,
                |candidate, request, on_progress| {
                    let job_id = candidate.job_id.clone();
                    crate::ocr::run_ocr_sidecar_cancellable_with_progress(
                        request,
                        resource_script_path.as_deref(),
                        || self.is_parse_job_cancelled(&job_id).unwrap_or(false),
                        on_progress,
                    )
                },
                &mut notify,
            );

            match outcome {
                Ok(OcrJobOutcome::NoQueuedJob) => {
                    self.push_system_message("OCR 后台队列已清空。".to_string());
                    notify("ocr-drained");
                    break;
                }
                Ok(outcome) => {
                    self.push_ocr_outcome_message(&outcome);
                    notify("ocr-state-changed");
                }
                Err(error) => {
                    self.push_system_message(format!("OCR 后台任务中断：{error}"));
                    notify("ocr-interrupted");
                    break;
                }
            }
        }

        self.finish_ocr_worker(&space_id);
        notify("ocr-worker-finished");
    }

    #[cfg(test)]
    fn run_next_ocr_parse_job_with_runner<F>(
        &self,
        space_id: String,
        runner: F,
    ) -> Result<OcrJobOutcome, AppError>
    where
        F: FnOnce(
            &ParseJobCandidate,
            &OcrSidecarRequest,
            &mut dyn FnMut(crate::ocr::OcrProgressUpdate),
        ) -> Result<OcrSidecarResult, AppError>,
    {
        let outcome = self.run_one_ocr_parse_job_with_runner(&space_id, runner)?;
        *self
            .active_space_id
            .lock()
            .expect("active space mutex poisoned") = Some(space_id);
        Ok(outcome)
    }

    #[cfg(test)]
    fn run_one_ocr_parse_job_with_runner<F>(
        &self,
        space_id: &str,
        runner: F,
    ) -> Result<OcrJobOutcome, AppError>
    where
        F: FnOnce(
            &ParseJobCandidate,
            &OcrSidecarRequest,
            &mut dyn FnMut(crate::ocr::OcrProgressUpdate),
        ) -> Result<OcrSidecarResult, AppError>,
    {
        self.run_one_ocr_parse_job_with_runner_notifying(space_id, runner, |_| {})
    }

    fn run_one_ocr_parse_job_with_runner_notifying<F, N>(
        &self,
        space_id: &str,
        runner: F,
        mut notify: N,
    ) -> Result<OcrJobOutcome, AppError>
    where
        F: FnOnce(
            &ParseJobCandidate,
            &OcrSidecarRequest,
            &mut dyn FnMut(crate::ocr::OcrProgressUpdate),
        ) -> Result<OcrSidecarResult, AppError>,
        N: FnMut(&str),
    {
        let (root_path, candidate) = {
            let mut store = self.store.lock().expect("sqlite store mutex poisoned");
            let root_path = store
                .get_space_root(&space_id)
                .map_err(|error| AppError::Storage(error.to_string()))?
                .ok_or_else(|| AppError::Storage("找不到要执行 OCR 的知识库".to_string()))?
                .root_path;
            let candidate = store
                .claim_next_queued_parse_job(&space_id, "ocr")
                .map_err(|error| AppError::Storage(format!("无法领取 OCR 任务：{error}")))?;

            (root_path, candidate)
        };
        let Some(candidate) = candidate else {
            return Ok(OcrJobOutcome::NoQueuedJob);
        };
        let file_name = Self::display_ocr_job_name(&candidate);
        let config = ocr_config(&self.app_data_dir);
        let root_path = Path::new(&root_path);
        let run_result = (|| {
            self.update_parse_progress(&candidate.job_id, "正在验证 OCR 输入", 0, 1)?;
            notify("ocr-progress");
            let prepared = Self::prepare_ocr_request_for_candidate(
                root_path,
                &candidate.relative_path,
                &candidate.extension,
                candidate.source_locator.as_deref(),
                &config.model_dir,
                &config.tier,
            )?;
            self.update_parse_progress(&candidate.job_id, "正在执行本地 OCR", 0, 1)?;
            notify("ocr-progress");
            let mut on_progress = |progress: crate::ocr::OcrProgressUpdate| {
                if self
                    .update_parse_progress(
                        &candidate.job_id,
                        &progress.phase,
                        progress.current,
                        progress.total,
                    )
                    .is_ok()
                {
                    notify("ocr-progress");
                }
            };
            let ocr_result = runner(&candidate, prepared.request(), &mut on_progress)?;
            self.update_parse_progress(&candidate.job_id, "正在写入索引", 1, 1)?;
            notify("ocr-progress");
            build_ocr_document(prepared.source_locator(), &ocr_result)
        })();

        match run_result {
            Ok(document) => {
                let outcome = {
                    let mut store = self.store.lock().expect("sqlite store mutex poisoned");
                    match store
                        .complete_parse_job_if_running(
                            space_id,
                            &candidate.file_id,
                            &candidate.job_id,
                            &document,
                        )
                        .map_err(|error| AppError::Storage(format!("无法保存 OCR 结果：{error}")))?
                    {
                        ParseJobWriteOutcome::Updated => OcrJobOutcome::Succeeded(file_name),
                        ParseJobWriteOutcome::Cancelled => OcrJobOutcome::Cancelled(file_name),
                        ParseJobWriteOutcome::NotRunning => {
                            return Err(AppError::Storage(
                                "OCR 任务状态已变化，无法标记成功".to_string(),
                            ));
                        }
                    }
                };

                Ok(outcome)
            }
            Err(error) => {
                if self.is_parse_job_cancelled(&candidate.job_id)? {
                    return Ok(OcrJobOutcome::Cancelled(file_name));
                }

                if self.record_parse_failure(&candidate.job_id, &candidate.file_id, &error)? {
                    Ok(OcrJobOutcome::Failed(file_name))
                } else {
                    Ok(OcrJobOutcome::Cancelled(file_name))
                }
            }
        }
    }

    fn update_parse_progress(
        &self,
        job_id: &str,
        phase: &str,
        progress_current: u32,
        progress_total: u32,
    ) -> Result<(), AppError> {
        let store = self.store.lock().expect("sqlite store mutex poisoned");
        store
            .update_parse_job_progress(job_id, phase, progress_current, progress_total)
            .map_err(|error| AppError::Storage(format!("无法更新解析进度：{error}")))?
            .then_some(())
            .ok_or_else(|| AppError::Storage("解析任务状态已变化，无法更新进度".to_string()))
    }

    fn update_scan_progress(&self, job_id: &str, progress: &ScanProgress) -> Result<(), AppError> {
        let current_path = display_relative_file_name(&progress.current_path);
        self.update_parse_progress(
            job_id,
            &format!("正在扫描 {current_path}"),
            progress.scanned_files,
            0,
        )
    }

    fn is_parse_job_cancelled(&self, job_id: &str) -> Result<bool, AppError> {
        let store = self.store.lock().expect("sqlite store mutex poisoned");
        let status = store
            .parse_job_status(job_id)
            .map_err(|error| AppError::Storage(format!("无法读取解析任务状态：{error}")))?;
        Ok(status.as_deref() == Some("cancelled"))
    }

    fn push_document_outcome_message(&self, outcome: &DocumentJobOutcome) {
        match outcome {
            DocumentJobOutcome::NoQueuedJob => {
                self.push_system_message("没有待执行的文档解析任务。".to_string());
            }
            DocumentJobOutcome::Succeeded(file_name) => {
                self.push_system_message(format!("文档解析完成：{file_name}。"));
            }
            DocumentJobOutcome::Failed(file_name) => {
                self.push_system_message(format!("文档解析失败：{file_name}。"));
            }
            DocumentJobOutcome::Cancelled(file_name) => {
                self.push_system_message(format!("文档解析已取消：{file_name}。"));
            }
        }
    }

    fn push_scan_outcome_message(&self, outcome: &ScanJobOutcome) {
        match outcome {
            ScanJobOutcome::NoQueuedJob => {
                self.push_system_message("没有待执行的扫描任务。".to_string());
            }
            ScanJobOutcome::Succeeded {
                summary,
                queued_document_count,
                queued_ocr_count,
            } => {
                self.push_system_message(format!(
                    "扫描完成：新增 {} 个，变更 {} 个，删除 {} 个；已排队 {} 个文档解析任务、{} 个图片 OCR 任务。",
                    summary.added_count,
                    summary.changed_count,
                    summary.deleted_count,
                    queued_document_count,
                    queued_ocr_count
                ));
            }
            ScanJobOutcome::Failed => {
                self.push_system_message("扫描失败。".to_string());
            }
            ScanJobOutcome::Cancelled => {
                self.push_system_message("扫描已取消。".to_string());
            }
        }
    }

    fn push_ocr_outcome_message(&self, outcome: &OcrJobOutcome) {
        match outcome {
            OcrJobOutcome::NoQueuedJob => {
                self.push_system_message("没有待执行的 OCR 任务。".to_string());
            }
            OcrJobOutcome::Succeeded(file_name) => {
                self.push_system_message(format!("OCR 解析完成：{file_name}。"));
            }
            OcrJobOutcome::Failed(file_name) => {
                self.push_system_message(format!("OCR 解析失败：{file_name}。"));
            }
            OcrJobOutcome::Cancelled(file_name) => {
                self.push_system_message(format!("OCR 解析已取消：{file_name}。"));
            }
        }
    }

    fn finish_ocr_worker(&self, space_id: &str) {
        self.active_ocr_workers
            .lock()
            .expect("ocr worker mutex poisoned")
            .remove(space_id);
    }

    fn finish_scan_worker(&self, space_id: &str) {
        self.active_scan_workers
            .lock()
            .expect("scan worker mutex poisoned")
            .remove(space_id);
    }

    fn finish_document_worker(&self, space_id: &str) {
        self.active_document_workers
            .lock()
            .expect("document worker mutex poisoned")
            .remove(space_id);
    }

    fn record_space_parse_failure(&self, job_id: &str, error: &AppError) -> Result<bool, AppError> {
        let message = error.to_string();
        let mut store = self.store.lock().expect("sqlite store mutex poisoned");
        match store
            .fail_space_parse_job_if_running(job_id, &message)
            .map_err(|error| AppError::Storage(format!("无法记录扫描任务失败：{error}")))?
        {
            ParseJobWriteOutcome::Updated => Ok(true),
            ParseJobWriteOutcome::Cancelled => Ok(false),
            ParseJobWriteOutcome::NotRunning => Err(AppError::Storage(
                "扫描任务状态已变化，无法标记失败".to_string(),
            )),
        }
    }

    fn record_parse_failure(
        &self,
        job_id: &str,
        file_id: &str,
        error: &AppError,
    ) -> Result<bool, AppError> {
        let message = error.to_string();
        let mut store = self.store.lock().expect("sqlite store mutex poisoned");
        match store
            .fail_parse_job_if_running(file_id, job_id, &message)
            .map_err(|error| AppError::Storage(format!("无法记录解析任务失败：{error}")))?
        {
            ParseJobWriteOutcome::Updated => Ok(true),
            ParseJobWriteOutcome::Cancelled => Ok(false),
            ParseJobWriteOutcome::NotRunning => Err(AppError::Storage(
                "解析任务状态已变化，无法标记失败".to_string(),
            )),
        }
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
        self.push_chat_message_with_sources(
            ChatRole::Assistant,
            answer,
            chat_sources_from_hits(&hits),
        );
        *self
            .active_space_id
            .lock()
            .expect("active space mutex poisoned") = Some(space_id);

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

    pub fn export_space_backup(
        &self,
        space_id: String,
        file_name: Option<String>,
    ) -> Result<BackupExportResult, AppError> {
        let backup = {
            let store = self.store.lock().expect("sqlite store mutex poisoned");
            match store.export_space_backup(&space_id) {
                Ok(backup) => backup,
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    return Err(AppError::Storage("找不到要导出的知识库".to_string()));
                }
                Err(error) => {
                    return Err(AppError::Storage(format!("无法读取备份数据：{error}")));
                }
            }
        };
        let file_name = resolve_backup_file_name(file_name, &backup.space.id)?;
        let backup_dir = self.app_data_dir.join("backups");
        fs::create_dir_all(&backup_dir)
            .map_err(|error| AppError::Filesystem(format!("无法创建备份目录：{error}")))?;
        let backup_path = backup_dir.join(&file_name);
        let body = serde_json::to_vec_pretty(&backup)
            .map_err(|error| AppError::Storage(format!("无法生成备份 JSON：{error}")))?;
        let mut backup_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&backup_path)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    AppError::Filesystem("备份文件已存在，请换一个文件名".to_string())
                } else {
                    AppError::Filesystem(format!("无法写入备份文件：{error}"))
                }
            })?;
        backup_file
            .write_all(&body)
            .map_err(|error| AppError::Filesystem(format!("无法写入备份文件：{error}")))?;
        let size_bytes = fs::metadata(&backup_path)
            .map(|metadata| metadata.len())
            .unwrap_or(body.len() as u64);

        *self
            .active_space_id
            .lock()
            .expect("active space mutex poisoned") = Some(space_id);
        self.push_system_message(format!("备份已导出：{file_name}。"));

        Ok(BackupExportResult {
            path: backup_path.to_string_lossy().to_string(),
            file_name,
            size_bytes,
            exported_at: backup.exported_at,
            file_count: backup.files.len() as u32,
            knowledge_block_count: backup.knowledge_blocks.len() as u32,
            parse_job_count: backup.parse_jobs.len() as u32,
        })
    }

    pub fn preflight_space_backup_restore(
        &self,
        path: String,
    ) -> Result<BackupRestorePreflight, AppError> {
        let (backup, backup_path) = read_backup_restore_file(&path)?;
        validate_backup_restore_payload(&backup)?;
        let will_overwrite = {
            let store = self.store.lock().expect("sqlite store mutex poisoned");
            ensure_backup_root_path_available(&store, &backup)?;
            store
                .has_knowledge_space(&backup.space.id)
                .map_err(|error| AppError::Storage(format!("无法检查备份覆盖状态：{error}")))?
        };

        Ok(build_backup_restore_preflight(
            &backup,
            &backup_path,
            will_overwrite,
        ))
    }

    pub fn restore_space_backup(
        &self,
        path: String,
        confirm_overwrite: bool,
    ) -> Result<BackupRestoreResult, AppError> {
        if !confirm_overwrite {
            return Err(AppError::PermissionDenied(
                "恢复备份需要确认，因为会覆盖本地应用状态".to_string(),
            ));
        }

        let (backup, backup_path) = read_backup_restore_file(&path)?;
        validate_backup_restore_payload(&backup)?;
        let preflight = {
            let mut store = self.store.lock().expect("sqlite store mutex poisoned");
            ensure_backup_root_path_available(&store, &backup)?;
            let will_overwrite = store
                .has_knowledge_space(&backup.space.id)
                .map_err(|error| AppError::Storage(format!("无法检查备份覆盖状态：{error}")))?;
            let preflight = build_backup_restore_preflight(&backup, &backup_path, will_overwrite);
            store
                .restore_space_backup(&backup)
                .map_err(|error| AppError::Storage(format!("无法恢复备份：{error}")))?;
            preflight
        };

        *self
            .active_space_id
            .lock()
            .expect("active space mutex poisoned") = Some(backup.workspace.active_space_id.clone());
        *self
            .session_permission
            .lock()
            .expect("session permission mutex poisoned") =
            backup.workspace.default_permission.clone();
        self.push_system_message(format!("备份已恢复：{}。", preflight.file_name));

        Ok(BackupRestoreResult {
            path: preflight.path,
            file_name: preflight.file_name,
            format: preflight.format,
            schema_version: preflight.schema_version,
            exported_at: preflight.exported_at,
            space_id: preflight.space_id,
            space_name: preflight.space_name,
            root_path: preflight.root_path,
            default_permission: preflight.default_permission,
            file_count: preflight.file_count,
            knowledge_block_count: preflight.knowledge_block_count,
            parse_job_count: preflight.parse_job_count,
            trash_entry_count: preflight.trash_entry_count,
            will_overwrite: preflight.will_overwrite,
            restored_at: OffsetDateTime::now_utc().to_string(),
            overwritten: preflight.will_overwrite,
        })
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
        self.push_chat_message_with_sources(role, content, Vec::new());
    }

    fn push_chat_message_with_sources(
        &self,
        role: ChatRole,
        content: String,
        sources: Vec<ChatMessageSource>,
    ) {
        let mut messages = self.messages.lock().expect("messages mutex poisoned");
        messages.push(ChatMessage {
            id: format!("msg-{}", uuid::Uuid::new_v4()),
            role,
            content,
            sources,
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
    latest_table: Option<TableInsightPreview>,
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
                source_locator: block.source_locator,
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
                source_file_name: first_file_name.clone(),
                source_locator: if has_files {
                    first_file_name
                } else {
                    "暂无来源定位".to_string()
                },
            }),
        table_preview: latest_table.unwrap_or_else(|| TableInsightPreview {
            id: "table-empty".to_string(),
            title: "表格理解等待接入".to_string(),
            description: "本阶段先完成文件扫描入库，表格结构洞察会在解析 xlsx 后显示。".to_string(),
        }),
        messages: if messages.is_empty() {
            vec![ChatMessage {
                id: "msg-system-ready".to_string(),
                role: ChatRole::System,
                content: if has_spaces {
                    "当前已使用本地 SQLite 读取真实知识库状态。".to_string()
                } else {
                    "请点击新建选择一个真实文件夹。".to_string()
                },
                sources: Vec::new(),
            }]
        } else {
            messages
        },
        pending_action: None,
    }
}

const SUPPORTED_BACKUP_FORMAT: &str = "library.backup.v1";
const SUPPORTED_BACKUP_SCHEMA_VERSION: u32 = 1;

fn read_backup_restore_file(path: &str) -> Result<(BackupExport, PathBuf), AppError> {
    let trimmed_path = path.trim();
    if trimmed_path.is_empty() {
        return Err(AppError::Filesystem("请选择有效备份文件".to_string()));
    }

    let backup_path = PathBuf::from(trimmed_path);
    validate_backup_file_path(&backup_path)?;
    let body = fs::read_to_string(&backup_path)
        .map_err(|error| AppError::Filesystem(format!("无法读取备份文件：{error}")))?;
    let backup = serde_json::from_str::<BackupExport>(&body)
        .map_err(|error| AppError::Storage(format!("无法解析备份 JSON：{error}")))?;

    Ok((backup, backup_path))
}

fn validate_backup_file_path(path: &Path) -> Result<(), AppError> {
    if !path.exists() {
        return Err(AppError::Filesystem("备份文件不存在".to_string()));
    }

    if !path.is_file() {
        return Err(AppError::Filesystem(
            "请选择备份文件而不是文件夹".to_string(),
        ));
    }

    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    if !extension.eq_ignore_ascii_case("json") {
        return Err(AppError::PermissionDenied(
            "备份文件必须使用 .json 后缀".to_string(),
        ));
    }

    Ok(())
}

fn validate_backup_restore_payload(backup: &BackupExport) -> Result<(), AppError> {
    if backup.format != SUPPORTED_BACKUP_FORMAT
        || backup.schema_version != SUPPORTED_BACKUP_SCHEMA_VERSION
    {
        return Err(AppError::Storage(format!(
            "不支持的备份版本：{} v{}",
            backup.format, backup.schema_version
        )));
    }

    validate_required_backup_field(&backup.space.id, "space.id")?;
    validate_required_backup_field(&backup.space.name, "space.name")?;
    validate_required_backup_field(&backup.space.root_path, "space.rootPath")?;
    if backup.workspace.active_space_id != backup.space.id {
        return Err(AppError::Storage(
            "备份结构不完整：workspace.activeSpaceId 与 space.id 不一致".to_string(),
        ));
    }

    let mut file_ids = HashSet::new();
    for file in &backup.files {
        validate_required_backup_field(&file.id, "file.id")?;
        validate_backup_relative_locator(&file.relative_path)?;
        validate_backup_parse_status(&file.parse_status)?;
        if !file_ids.insert(file.id.as_str()) {
            return Err(AppError::Storage(
                "备份结构不完整：文件 ID 重复".to_string(),
            ));
        }
    }

    let mut note_ids = HashSet::new();
    for note in &backup.markdown_notes {
        validate_required_backup_field(&note.id, "markdownNote.id")?;
        validate_backup_relative_locator(&note.relative_path)?;
        if let Some(file_id) = &note.file_id {
            validate_backup_reference(&file_ids, file_id, "markdownNote.fileId")?;
        }
        if !note_ids.insert(note.id.as_str()) {
            return Err(AppError::Storage(
                "备份结构不完整：Markdown 笔记 ID 重复".to_string(),
            ));
        }
    }

    let mut block_ids = HashSet::new();
    for block in &backup.knowledge_blocks {
        validate_required_backup_field(&block.id, "knowledgeBlock.id")?;
        validate_backup_relative_locator(&block.source_locator)?;
        if let Some(file_id) = &block.file_id {
            validate_backup_reference(&file_ids, file_id, "knowledgeBlock.fileId")?;
        }
        if let Some(note_id) = &block.note_id {
            validate_backup_reference(&note_ids, note_id, "knowledgeBlock.noteId")?;
        }
        if !matches!(
            block.source_kind.as_str(),
            "original_file" | "markdown_note" | "table"
        ) {
            return Err(AppError::Storage(
                "备份结构不完整：知识块来源类型无效".to_string(),
            ));
        }
        if !block_ids.insert(block.id.as_str()) {
            return Err(AppError::Storage(
                "备份结构不完整：知识块 ID 重复".to_string(),
            ));
        }
    }

    let mut job_ids = HashSet::new();
    for job in &backup.parse_jobs {
        validate_required_backup_field(&job.id, "parseJob.id")?;
        validate_backup_job_status(&job.status)?;
        if let Some(source_locator) = &job.source_locator {
            validate_backup_relative_locator(source_locator)?;
        }
        if let Some(file_id) = &job.file_id {
            validate_backup_reference(&file_ids, file_id, "parseJob.fileId")?;
        }
        if !job_ids.insert(job.id.as_str()) {
            return Err(AppError::Storage(
                "备份结构不完整：解析任务 ID 重复".to_string(),
            ));
        }
    }

    let mut trash_ids = HashSet::new();
    for entry in &backup.trash_entries {
        validate_required_backup_field(&entry.id, "trashEntry.id")?;
        validate_backup_relative_locator(&entry.original_locator)?;
        match entry.entity_kind.as_str() {
            "file" => {
                validate_backup_reference(&file_ids, &entry.entity_id, "trashEntry.entityId")?
            }
            "markdown_note" => {
                validate_backup_reference(&note_ids, &entry.entity_id, "trashEntry.entityId")?
            }
            "knowledge_block" => {
                validate_backup_reference(&block_ids, &entry.entity_id, "trashEntry.entityId")?
            }
            _ => {
                return Err(AppError::Storage(
                    "备份结构不完整：回收站实体类型无效".to_string(),
                ));
            }
        }
        if !trash_ids.insert(entry.id.as_str()) {
            return Err(AppError::Storage(
                "备份结构不完整：回收站记录 ID 重复".to_string(),
            ));
        }
    }

    Ok(())
}

fn ensure_backup_root_path_available(
    store: &SqliteStore,
    backup: &BackupExport,
) -> Result<(), AppError> {
    let existing_space_id = store
        .active_space_id_for_root_path(&backup.space.root_path)
        .map_err(|error| AppError::Storage(format!("无法检查备份根路径冲突：{error}")))?;

    if existing_space_id
        .as_deref()
        .is_some_and(|space_id| space_id != backup.space.id)
    {
        return Err(AppError::Storage(
            "备份根路径已被其他知识库使用，无法恢复".to_string(),
        ));
    }

    Ok(())
}

fn validate_required_backup_field(value: &str, field_name: &str) -> Result<(), AppError> {
    if value.trim().is_empty() {
        return Err(AppError::Storage(format!(
            "备份结构不完整：{field_name} 不能为空"
        )));
    }

    Ok(())
}

fn validate_backup_reference(
    ids: &HashSet<&str>,
    referenced_id: &str,
    field_name: &str,
) -> Result<(), AppError> {
    if !ids.contains(referenced_id) {
        return Err(AppError::Storage(format!(
            "备份结构不完整：{field_name} 引用了不存在的对象"
        )));
    }

    Ok(())
}

fn validate_backup_relative_locator(locator: &str) -> Result<(), AppError> {
    source_locator_to_relative_path(locator)
        .map_err(|_| AppError::PermissionDenied("备份内容包含越界路径，已拒绝恢复".to_string()))?;

    Ok(())
}

fn validate_backup_parse_status(status: &str) -> Result<(), AppError> {
    if !matches!(status, "indexed" | "changed" | "queued" | "failed") {
        return Err(AppError::Storage(
            "备份结构不完整：文件解析状态无效".to_string(),
        ));
    }

    Ok(())
}

fn validate_backup_job_status(status: &str) -> Result<(), AppError> {
    if !matches!(
        status,
        "queued" | "running" | "succeeded" | "failed" | "cancelled"
    ) {
        return Err(AppError::Storage(
            "备份结构不完整：解析任务状态无效".to_string(),
        ));
    }

    Ok(())
}

fn build_backup_restore_preflight(
    backup: &BackupExport,
    path: &Path,
    will_overwrite: bool,
) -> BackupRestorePreflight {
    BackupRestorePreflight {
        path: path.to_string_lossy().to_string(),
        file_name: path
            .file_name()
            .and_then(|file_name| file_name.to_str())
            .unwrap_or("backup.json")
            .to_string(),
        format: backup.format.clone(),
        schema_version: backup.schema_version,
        exported_at: backup.exported_at.clone(),
        space_id: backup.space.id.clone(),
        space_name: backup.space.name.clone(),
        root_path: backup.space.root_path.clone(),
        default_permission: backup.space.default_permission.clone(),
        file_count: backup.files.len() as u32,
        knowledge_block_count: backup.knowledge_blocks.len() as u32,
        parse_job_count: backup.parse_jobs.len() as u32,
        trash_entry_count: backup.trash_entries.len() as u32,
        will_overwrite,
    }
}

fn chat_sources_from_hits(hits: &[KnowledgeBlockSearchHit]) -> Vec<ChatMessageSource> {
    hits.iter()
        .map(|hit| ChatMessageSource {
            id: hit.id.clone(),
            title: hit.title.clone(),
            excerpt: hit.excerpt.clone(),
            source_file_name: hit.source_file_name.clone(),
            source_locator: hit.source_locator.clone(),
            source_kind: hit.source_kind.clone(),
        })
        .collect()
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

fn resolve_backup_file_name(
    requested_file_name: Option<String>,
    space_id: &str,
) -> Result<String, AppError> {
    let requested = requested_file_name
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| default_backup_file_name(space_id));
    let mut components = Path::new(&requested).components();
    let Some(Component::Normal(file_name)) = components.next() else {
        return Err(AppError::PermissionDenied(
            "备份文件名不能包含路径或上级目录".to_string(),
        ));
    };

    if components.next().is_some() {
        return Err(AppError::PermissionDenied(
            "备份文件名不能包含路径或上级目录".to_string(),
        ));
    }

    let file_name = file_name.to_string_lossy().trim().to_string();
    if file_name.is_empty() || file_name == "." || file_name == ".." {
        return Err(AppError::PermissionDenied(
            "备份文件名不能包含路径或上级目录".to_string(),
        ));
    }

    if file_name.chars().any(is_windows_file_name_forbidden) {
        return Err(AppError::PermissionDenied(
            "备份文件名包含 Windows 不支持的字符".to_string(),
        ));
    }

    let extension = Path::new(&file_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    if !extension.eq_ignore_ascii_case("json") {
        return Err(AppError::PermissionDenied(
            "备份文件名必须使用 .json 后缀".to_string(),
        ));
    }

    Ok(file_name)
}

fn default_backup_file_name(space_id: &str) -> String {
    let timestamp = OffsetDateTime::now_utc().unix_timestamp_nanos();
    let space_token = sanitize_backup_file_token(space_id);
    format!("library-backup-{space_token}-{timestamp}.json")
}

fn sanitize_backup_file_token(value: &str) -> String {
    let token = value
        .chars()
        .filter_map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                Some(character)
            } else {
                None
            }
        })
        .take(32)
        .collect::<String>();

    if token.is_empty() {
        "space".to_string()
    } else {
        token
    }
}

fn is_windows_file_name_forbidden(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
        )
}

fn scan_filesystem_error(error: std::io::Error) -> AppError {
    AppError::Filesystem(format!("无法扫描文件夹：{error}"))
}

fn source_locator_to_relative_path(source_locator: &str) -> Result<PathBuf, AppError> {
    let locator = strip_known_source_fragment(source_locator);

    if locator.is_empty() || locator == "暂无来源定位" {
        return Err(AppError::Filesystem(
            "来源定位为空，无法打开文件".to_string(),
        ));
    }

    let mut safe_path = PathBuf::new();
    for component in Path::new(locator).components() {
        match component {
            Component::Normal(part) => safe_path.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(AppError::PermissionDenied(
                    "来源定位必须是知识库内的相对路径".to_string(),
                ));
            }
        }
    }

    if safe_path.as_os_str().is_empty() {
        return Err(AppError::Filesystem(
            "来源定位为空，无法打开文件".to_string(),
        ));
    }

    Ok(safe_path)
}

fn strip_known_source_fragment(source_locator: &str) -> &str {
    let mut source_path = source_locator.trim();

    while let Some((path, fragment)) = source_path.rsplit_once('#') {
        if !is_known_source_fragment(fragment) {
            break;
        }
        source_path = path.trim();
    }

    source_path
}

fn is_known_source_fragment(fragment: &str) -> bool {
    fragment == "ocr"
        || numbered_fragment(fragment, "page-")
        || numbered_fragment(fragment, "ocr-page-")
        || numbered_fragment(fragment, "image-")
        || numbered_fragment(fragment, "block-")
        || numbered_fragment(fragment, "ocr-block-")
        || numbered_fragment(fragment, "sheet-")
}

fn numbered_fragment(fragment: &str, prefix: &str) -> bool {
    fragment
        .strip_prefix(prefix)
        .map(|value| !value.is_empty() && value.chars().all(|character| character.is_ascii_digit()))
        .unwrap_or(false)
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
    matches!(
        extension.trim_start_matches('.').to_lowercase().as_str(),
        "pdf" | "png" | "jpg" | "jpeg" | "bmp" | "tif" | "tiff" | "webp"
    )
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};
    use std::{env, fs};

    use super::AppState;
    use crate::models::{
        ChatRole, OcrPageResult, OcrSidecarResult, ParsedTableInsight, PermissionMode, ScannedFile,
    };
    use crate::parser::parse_file;
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
        fs::write(temp_dir.path().join("image.png"), "image").expect("write image");
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

        assert_eq!(scanned.files.len(), 2);
        assert!(scanned
            .files
            .iter()
            .any(|file| file.name == "README.md" && file.status_label == "待解析"));
        assert!(scanned
            .files
            .iter()
            .any(|file| file.name == "image.png" && file.status_label == "待解析"));
        assert!(scanned
            .parse_jobs
            .iter()
            .any(|job| job.job_type == "scan" && job.status == "succeeded"));
        assert!(scanned
            .parse_jobs
            .iter()
            .any(|job| job.job_type == "document" && job.status == "queued"));
        assert!(scanned.parse_jobs.iter().any(|job| job.job_type == "ocr"
            && job.status == "queued"
            && job.file_name == "image.png"));
        assert_eq!(scanned.spaces[0].document_queue_count, 1);
        assert_eq!(scanned.spaces[0].ocr_queue_count, 1);
    }

    #[test]
    fn export_space_backup_writes_json_under_app_data_backups() {
        let knowledge_dir = tempfile::tempdir().expect("knowledge dir");
        fs::write(knowledge_dir.path().join("README.md"), "hello").expect("write md");
        let app_data_dir = tempfile::tempdir().expect("app data dir");
        let state = AppState::new_with_app_data_dir(
            SqliteStore::open_in_memory().expect("sqlite opens"),
            app_data_dir.path().to_path_buf(),
        );
        let created = state
            .create_knowledge_space(
                "备份空间".to_string(),
                knowledge_dir.path().to_string_lossy().to_string(),
                PermissionMode::Approval,
            )
            .expect("space created");
        state
            .scan_knowledge_space(created.active_space_id.clone())
            .expect("space scanned");

        let exported = state
            .export_space_backup(
                created.active_space_id.clone(),
                Some("library-backup.json".to_string()),
            )
            .expect("backup exported");
        let backup_path = app_data_dir
            .path()
            .join("backups")
            .join("library-backup.json");
        let body = fs::read_to_string(&backup_path).expect("backup file is readable");
        let json: serde_json::Value = serde_json::from_str(&body).expect("backup is json");

        assert_eq!(exported.file_name, "library-backup.json");
        assert_eq!(exported.path, backup_path.to_string_lossy());
        assert!(exported.size_bytes > 0);
        assert_eq!(json["format"], "library.backup.v1");
        assert_eq!(json["space"]["id"], created.active_space_id);
        assert_eq!(json["files"][0]["relativePath"], "README.md");
        assert!(!body.contains("DEEPSEEK_API_KEY"));
        assert!(!body.contains(".env"));
        assert!(!body.contains("models\\ocr"));
    }

    #[test]
    fn export_space_backup_rejects_path_traversal_file_name() {
        let knowledge_dir = tempfile::tempdir().expect("knowledge dir");
        let app_data_dir = tempfile::tempdir().expect("app data dir");
        let state = AppState::new_with_app_data_dir(
            SqliteStore::open_in_memory().expect("sqlite opens"),
            app_data_dir.path().to_path_buf(),
        );
        let created = state
            .create_knowledge_space(
                "备份空间".to_string(),
                knowledge_dir.path().to_string_lossy().to_string(),
                PermissionMode::Approval,
            )
            .expect("space created");

        let result =
            state.export_space_backup(created.active_space_id, Some("..\\secret.json".to_string()));

        assert!(matches!(
            result,
            Err(crate::error::AppError::PermissionDenied(_))
        ));
        assert!(!app_data_dir.path().join("secret.json").exists());
    }

    #[test]
    fn export_space_backup_rejects_existing_backup_file_name() {
        let knowledge_dir = tempfile::tempdir().expect("knowledge dir");
        let app_data_dir = tempfile::tempdir().expect("app data dir");
        let state = AppState::new_with_app_data_dir(
            SqliteStore::open_in_memory().expect("sqlite opens"),
            app_data_dir.path().to_path_buf(),
        );
        let created = state
            .create_knowledge_space(
                "备份空间".to_string(),
                knowledge_dir.path().to_string_lossy().to_string(),
                PermissionMode::Approval,
            )
            .expect("space created");
        fs::create_dir_all(app_data_dir.path().join("backups")).expect("backup dir");
        let backup_path = app_data_dir
            .path()
            .join("backups")
            .join("library-backup.json");
        fs::write(&backup_path, "existing backup").expect("existing backup");

        let result = state.export_space_backup(
            created.active_space_id,
            Some("library-backup.json".to_string()),
        );

        assert!(matches!(
            result,
            Err(crate::error::AppError::Filesystem(message)) if message.contains("备份文件已存在")
        ));
        assert_eq!(
            fs::read_to_string(&backup_path).expect("existing backup is readable"),
            "existing backup"
        );
    }

    #[test]
    fn export_space_backup_rejects_missing_workspace() {
        let app_data_dir = tempfile::tempdir().expect("app data dir");
        let state = AppState::new_with_app_data_dir(
            SqliteStore::open_in_memory().expect("sqlite opens"),
            app_data_dir.path().to_path_buf(),
        );

        let result = state.export_space_backup("missing-space".to_string(), None);

        assert!(matches!(
            result,
            Err(crate::error::AppError::Storage(message)) if message.contains("找不到要导出的知识库")
        ));
    }

    #[test]
    fn restore_space_backup_preflight_reads_summary_without_touching_state() {
        let app_data_dir = tempfile::tempdir().expect("app data dir");
        let state = AppState::new_with_app_data_dir(
            SqliteStore::open_in_memory().expect("sqlite opens"),
            app_data_dir.path().to_path_buf(),
        );
        write_backup_fixture(app_data_dir.path(), "library-backup.json");

        let preflight = state
            .preflight_space_backup_restore(
                app_data_dir
                    .path()
                    .join("backups")
                    .join("library-backup.json")
                    .to_string_lossy()
                    .to_string(),
            )
            .expect("restore preflight succeeds");
        let snapshot = state.snapshot().expect("snapshot builds");

        assert_eq!(preflight.file_name, "library-backup.json");
        assert_eq!(preflight.space_id, "backup-space");
        assert_eq!(preflight.space_name, "备份空间");
        assert_eq!(preflight.file_count, 1);
        assert_eq!(preflight.knowledge_block_count, 1);
        assert!(!preflight.will_overwrite);
        assert!(snapshot.spaces.is_empty());
    }

    #[test]
    fn restore_space_backup_preflight_rejects_path_traversal_inside_backup() {
        let app_data_dir = tempfile::tempdir().expect("app data dir");
        let state = AppState::new_with_app_data_dir(
            SqliteStore::open_in_memory().expect("sqlite opens"),
            app_data_dir.path().to_path_buf(),
        );
        let backup_path = write_backup_fixture(app_data_dir.path(), "library-backup.json");
        let mut json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&backup_path).expect("backup readable"))
                .expect("backup json parses");
        json["files"][0]["relativePath"] = serde_json::Value::String("..\\secret.md".to_string());
        fs::write(
            &backup_path,
            serde_json::to_string_pretty(&json).expect("backup serializes"),
        )
        .expect("backup overwritten");

        let result =
            state.preflight_space_backup_restore(backup_path.to_string_lossy().to_string());

        assert!(matches!(
            result,
            Err(crate::error::AppError::PermissionDenied(message))
                if message.contains("备份内容包含越界路径")
        ));
        assert!(state.snapshot().expect("snapshot builds").spaces.is_empty());
    }

    #[test]
    fn restore_space_backup_preflight_rejects_incompatible_version() {
        let app_data_dir = tempfile::tempdir().expect("app data dir");
        let state = AppState::new_with_app_data_dir(
            SqliteStore::open_in_memory().expect("sqlite opens"),
            app_data_dir.path().to_path_buf(),
        );
        let backup_path = write_backup_fixture(app_data_dir.path(), "library-backup.json");
        let mut json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&backup_path).expect("backup readable"))
                .expect("backup json parses");
        json["schemaVersion"] = serde_json::Value::from(999);
        fs::write(
            &backup_path,
            serde_json::to_string_pretty(&json).expect("backup serializes"),
        )
        .expect("backup overwritten");

        let result =
            state.preflight_space_backup_restore(backup_path.to_string_lossy().to_string());

        assert!(matches!(
            result,
            Err(crate::error::AppError::Storage(message))
                if message.contains("不支持的备份版本")
        ));
    }

    #[test]
    fn restore_space_backup_preflight_rejects_invalid_parse_status() {
        let app_data_dir = tempfile::tempdir().expect("app data dir");
        let state = AppState::new_with_app_data_dir(
            SqliteStore::open_in_memory().expect("sqlite opens"),
            app_data_dir.path().to_path_buf(),
        );
        let backup_path = write_backup_fixture(app_data_dir.path(), "library-backup.json");
        let mut json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&backup_path).expect("backup readable"))
                .expect("backup json parses");
        json["files"][0]["parseStatus"] = serde_json::Value::String("done".to_string());
        fs::write(
            &backup_path,
            serde_json::to_string_pretty(&json).expect("backup serializes"),
        )
        .expect("backup overwritten");

        let result =
            state.preflight_space_backup_restore(backup_path.to_string_lossy().to_string());

        assert!(matches!(
            result,
            Err(crate::error::AppError::Storage(message))
                if message.contains("文件解析状态无效")
        ));
    }

    #[test]
    fn restore_space_backup_preflight_rejects_missing_backup_reference() {
        let app_data_dir = tempfile::tempdir().expect("app data dir");
        let state = AppState::new_with_app_data_dir(
            SqliteStore::open_in_memory().expect("sqlite opens"),
            app_data_dir.path().to_path_buf(),
        );
        let backup_path = write_backup_fixture(app_data_dir.path(), "library-backup.json");
        let mut json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&backup_path).expect("backup readable"))
                .expect("backup json parses");
        json["knowledgeBlocks"][0]["fileId"] =
            serde_json::Value::String("missing-file".to_string());
        fs::write(
            &backup_path,
            serde_json::to_string_pretty(&json).expect("backup serializes"),
        )
        .expect("backup overwritten");

        let result =
            state.preflight_space_backup_restore(backup_path.to_string_lossy().to_string());

        assert!(matches!(
            result,
            Err(crate::error::AppError::Storage(message))
                if message.contains("引用了不存在的对象")
        ));
    }

    #[test]
    fn restore_space_backup_preflight_rejects_parse_job_source_locator_path_traversal() {
        let app_data_dir = tempfile::tempdir().expect("app data dir");
        let state = AppState::new_with_app_data_dir(
            SqliteStore::open_in_memory().expect("sqlite opens"),
            app_data_dir.path().to_path_buf(),
        );
        let backup_path = write_backup_fixture(app_data_dir.path(), "library-backup.json");
        let mut json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&backup_path).expect("backup readable"))
                .expect("backup json parses");
        json["parseJobs"] = serde_json::json!([
            {
                "id": "job-traversal",
                "fileId": "file-redis",
                "sourceLocator": "..\\secret.docx#image-001",
                "jobType": "ocr",
                "status": "queued",
                "errorMessage": null,
                "startedAt": null,
                "finishedAt": null,
                "progressCurrent": 0,
                "progressTotal": 1,
                "phase": "等待执行",
                "createdAt": "2026-06-23T00:00:00Z",
                "updatedAt": "2026-06-23T00:00:00Z"
            }
        ]);
        fs::write(
            &backup_path,
            serde_json::to_string_pretty(&json).expect("backup serializes"),
        )
        .expect("backup overwritten");

        let result =
            state.preflight_space_backup_restore(backup_path.to_string_lossy().to_string());

        assert!(matches!(
            result,
            Err(crate::error::AppError::PermissionDenied(message))
                if message.contains("越界路径")
        ));
    }
    #[test]
    fn restore_space_backup_preflight_rejects_invalid_parse_job_status() {
        let app_data_dir = tempfile::tempdir().expect("app data dir");
        let state = AppState::new_with_app_data_dir(
            SqliteStore::open_in_memory().expect("sqlite opens"),
            app_data_dir.path().to_path_buf(),
        );
        let backup_path = write_backup_fixture(app_data_dir.path(), "library-backup.json");
        let mut json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&backup_path).expect("backup readable"))
                .expect("backup json parses");
        json["parseJobs"] = serde_json::json!([
            {
                "id": "job-invalid",
                "fileId": "file-redis",
                "jobType": "document",
                "status": "done",
                "errorMessage": null,
                "startedAt": null,
                "finishedAt": null,
                "progressCurrent": 0,
                "progressTotal": 0,
                "phase": "等待执行",
                "createdAt": "2026-06-23T00:00:00Z",
                "updatedAt": "2026-06-23T00:00:00Z"
            }
        ]);
        fs::write(
            &backup_path,
            serde_json::to_string_pretty(&json).expect("backup serializes"),
        )
        .expect("backup overwritten");

        let result =
            state.preflight_space_backup_restore(backup_path.to_string_lossy().to_string());

        assert!(matches!(
            result,
            Err(crate::error::AppError::Storage(message))
                if message.contains("解析任务状态无效")
        ));
    }

    #[test]
    fn restore_space_backup_preflight_rejects_duplicate_parse_jobs() {
        let app_data_dir = tempfile::tempdir().expect("app data dir");
        let state = AppState::new_with_app_data_dir(
            SqliteStore::open_in_memory().expect("sqlite opens"),
            app_data_dir.path().to_path_buf(),
        );
        let backup_path = write_backup_fixture(app_data_dir.path(), "library-backup.json");
        let mut json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&backup_path).expect("backup readable"))
                .expect("backup json parses");
        json["parseJobs"] = serde_json::json!([
            {
                "id": "job-duplicate",
                "fileId": "file-redis",
                "jobType": "document",
                "status": "queued",
                "errorMessage": null,
                "startedAt": null,
                "finishedAt": null,
                "progressCurrent": 0,
                "progressTotal": 0,
                "phase": "等待执行",
                "createdAt": "2026-06-23T00:00:00Z",
                "updatedAt": "2026-06-23T00:00:00Z"
            },
            {
                "id": "job-duplicate",
                "fileId": "file-redis",
                "jobType": "document",
                "status": "queued",
                "errorMessage": null,
                "startedAt": null,
                "finishedAt": null,
                "progressCurrent": 0,
                "progressTotal": 0,
                "phase": "等待执行",
                "createdAt": "2026-06-23T00:00:00Z",
                "updatedAt": "2026-06-23T00:00:00Z"
            }
        ]);
        fs::write(
            &backup_path,
            serde_json::to_string_pretty(&json).expect("backup serializes"),
        )
        .expect("backup overwritten");

        let result =
            state.preflight_space_backup_restore(backup_path.to_string_lossy().to_string());

        assert!(matches!(
            result,
            Err(crate::error::AppError::Storage(message))
                if message.contains("解析任务 ID 重复")
        ));
    }

    #[test]
    fn restore_space_backup_preflight_rejects_duplicate_trash_entries() {
        let app_data_dir = tempfile::tempdir().expect("app data dir");
        let state = AppState::new_with_app_data_dir(
            SqliteStore::open_in_memory().expect("sqlite opens"),
            app_data_dir.path().to_path_buf(),
        );
        let backup_path = write_backup_fixture(app_data_dir.path(), "library-backup.json");
        let mut json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&backup_path).expect("backup readable"))
                .expect("backup json parses");
        json["trashEntries"] = serde_json::json!([
            {
                "id": "trash-duplicate",
                "entityKind": "file",
                "entityId": "file-redis",
                "displayName": "Redis面试.md",
                "originalLocator": "Redis面试.md",
                "deletedAt": "2026-06-23T00:00:00Z",
                "restoredAt": null
            },
            {
                "id": "trash-duplicate",
                "entityKind": "file",
                "entityId": "file-redis",
                "displayName": "Redis面试副本.md",
                "originalLocator": "Redis面试.md",
                "deletedAt": "2026-06-23T00:00:00Z",
                "restoredAt": null
            }
        ]);
        fs::write(
            &backup_path,
            serde_json::to_string_pretty(&json).expect("backup serializes"),
        )
        .expect("backup overwritten");

        let result =
            state.preflight_space_backup_restore(backup_path.to_string_lossy().to_string());

        assert!(matches!(
            result,
            Err(crate::error::AppError::Storage(message))
                if message.contains("回收站记录 ID 重复")
        ));
    }

    #[test]
    fn restore_space_backup_preflight_rejects_root_path_conflict() {
        let app_data_dir = tempfile::tempdir().expect("app data dir");
        let store = SqliteStore::open_in_memory().expect("sqlite opens");
        store
            .create_knowledge_space("已有空间", "D:\\知识库\\备份空间", PermissionMode::Approval)
            .expect("conflicting space is inserted");
        let state = AppState::new_with_app_data_dir(store, app_data_dir.path().to_path_buf());
        let backup_path = write_backup_fixture(app_data_dir.path(), "library-backup.json");

        let result =
            state.preflight_space_backup_restore(backup_path.to_string_lossy().to_string());

        assert!(matches!(
            result,
            Err(crate::error::AppError::Storage(message))
                if message.contains("备份根路径已被其他知识库使用")
        ));
    }

    #[test]
    fn restore_space_backup_requires_explicit_confirmation_before_writing() {
        let app_data_dir = tempfile::tempdir().expect("app data dir");
        let state = AppState::new_with_app_data_dir(
            SqliteStore::open_in_memory().expect("sqlite opens"),
            app_data_dir.path().to_path_buf(),
        );
        let backup_path = write_backup_fixture(app_data_dir.path(), "library-backup.json");

        let result = state.restore_space_backup(backup_path.to_string_lossy().to_string(), false);

        assert!(matches!(
            result,
            Err(crate::error::AppError::PermissionDenied(message))
                if message.contains("需要确认")
        ));
        assert!(state.snapshot().expect("snapshot builds").spaces.is_empty());
    }

    #[test]
    fn resolves_source_locator_inside_space_root() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let docs_dir = temp_dir.path().join("docs");
        fs::create_dir_all(&docs_dir).expect("create docs dir");
        let source_file = docs_dir.join("Redis.md");
        fs::write(&source_file, "redis").expect("write source file");
        let state = AppState::new(SqliteStore::open_in_memory().expect("sqlite opens"));
        let created = state
            .create_knowledge_space(
                "测试知识库".to_string(),
                temp_dir.path().to_string_lossy().to_string(),
                PermissionMode::Approval,
            )
            .expect("space created");

        let resolved = state
            .resolve_source_file_path(&created.active_space_id, "docs/Redis.md#block-001")
            .expect("source file resolves");

        assert_eq!(
            resolved,
            source_file.canonicalize().expect("canonical file")
        );
    }

    #[test]
    fn resolves_ocr_source_locator_suffix_to_original_file() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let source_file = temp_dir.path().join("scan.pdf");
        fs::write(&source_file, "pdf").expect("write pdf");
        let state = AppState::new(SqliteStore::open_in_memory().expect("sqlite opens"));
        let created = state
            .create_knowledge_space(
                "OCR".to_string(),
                temp_dir.path().to_string_lossy().to_string(),
                PermissionMode::Approval,
            )
            .expect("space created");

        let resolved = state
            .resolve_source_file_path(&created.active_space_id, "scan.pdf#ocr-block-001")
            .expect("ocr source file resolves");

        assert_eq!(resolved, source_file.canonicalize().expect("canonical pdf"));

        let legacy_resolved = state
            .resolve_source_file_path(&created.active_space_id, "scan.pdf#ocr")
            .expect("legacy ocr source file resolves");
        assert_eq!(
            legacy_resolved,
            source_file.canonicalize().expect("canonical pdf")
        );

        let page_resolved = state
            .resolve_source_file_path(&created.active_space_id, "scan.pdf#ocr-page-002#block-001")
            .expect("ocr page source file resolves");
        assert_eq!(
            page_resolved,
            source_file.canonicalize().expect("canonical pdf")
        );

        let parsed_page_resolved = state
            .resolve_source_file_path(&created.active_space_id, "scan.pdf#page-001")
            .expect("pdf page source file resolves");
        assert_eq!(
            parsed_page_resolved,
            source_file.canonicalize().expect("canonical pdf")
        );
    }

    #[test]
    fn resolves_xlsx_sheet_source_locator_to_original_file() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let source_file = temp_dir.path().join("经营报表.xlsx");
        fs::write(&source_file, "xlsx").expect("write xlsx");
        let state = AppState::new(SqliteStore::open_in_memory().expect("sqlite opens"));
        let created = state
            .create_knowledge_space(
                "报表".to_string(),
                temp_dir.path().to_string_lossy().to_string(),
                PermissionMode::Approval,
            )
            .expect("space created");

        let resolved = state
            .resolve_source_file_path(&created.active_space_id, "经营报表.xlsx#sheet-001")
            .expect("xlsx source file resolves");

        assert_eq!(
            resolved,
            source_file.canonicalize().expect("canonical xlsx")
        );
    }

    #[test]
    fn resolves_docx_embedded_image_source_locator_to_original_file() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let source_file = temp_dir.path().join("架构说明.docx");
        fs::write(&source_file, "docx").expect("write docx");
        let state = AppState::new(SqliteStore::open_in_memory().expect("sqlite opens"));
        let created = state
            .create_knowledge_space(
                "文档".to_string(),
                temp_dir.path().to_string_lossy().to_string(),
                PermissionMode::Approval,
            )
            .expect("space created");

        let resolved = state
            .resolve_source_file_path(&created.active_space_id, "架构说明.docx#image-001")
            .expect("docx image source file resolves");

        assert_eq!(
            resolved,
            source_file.canonicalize().expect("canonical docx")
        );
    }

    #[test]
    fn rejects_source_locator_path_traversal() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let state = AppState::new(SqliteStore::open_in_memory().expect("sqlite opens"));
        let created = state
            .create_knowledge_space(
                "测试知识库".to_string(),
                temp_dir.path().to_string_lossy().to_string(),
                PermissionMode::Approval,
            )
            .expect("space created");

        let resolved = state.resolve_source_file_path(&created.active_space_id, "..\\secret.txt");

        assert!(matches!(
            resolved,
            Err(crate::error::AppError::PermissionDenied(_))
        ));
    }

    #[test]
    fn rejects_absolute_source_locator() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let source_file = temp_dir.path().join("README.md");
        fs::write(&source_file, "hello").expect("write source file");
        let state = AppState::new(SqliteStore::open_in_memory().expect("sqlite opens"));
        let created = state
            .create_knowledge_space(
                "测试知识库".to_string(),
                temp_dir.path().to_string_lossy().to_string(),
                PermissionMode::Approval,
            )
            .expect("space created");

        let resolved = state.resolve_source_file_path(
            &created.active_space_id,
            &source_file
                .canonicalize()
                .expect("canonical file")
                .to_string_lossy(),
        );

        assert!(matches!(
            resolved,
            Err(crate::error::AppError::PermissionDenied(_))
        ));
    }

    #[test]
    fn scan_worker_writes_files_and_enqueues_document_jobs() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        fs::write(temp_dir.path().join("README.md"), "hello").expect("write md");
        let state = AppState::new(SqliteStore::open_in_memory().expect("sqlite opens"));
        let created = state
            .create_knowledge_space(
                "测试知识库".to_string(),
                temp_dir.path().to_string_lossy().to_string(),
                PermissionMode::Approval,
            )
            .expect("space created");
        let space_id = created.active_space_id;

        assert!(state
            .prepare_scan_knowledge_space(space_id.clone())
            .expect("scan job starts"));
        state
            .run_next_scan_job_with_scanner(space_id.clone(), |root_path, _job_id| {
                crate::scanner::scan_folder(root_path).map_err(|error| {
                    crate::error::AppError::Filesystem(format!("scan failed: {error}"))
                })
            })
            .expect("scan job runs");
        state.finish_scan_worker(&space_id);
        let snapshot = state.snapshot().expect("snapshot builds");

        assert_eq!(snapshot.files.len(), 1);
        assert!(snapshot
            .parse_jobs
            .iter()
            .any(|job| job.job_type == "scan" && job.status == "succeeded"));
        assert!(snapshot
            .parse_jobs
            .iter()
            .any(|job| job.job_type == "document" && job.status == "queued"));
    }

    #[test]
    fn cancelled_running_scan_job_does_not_write_scanned_files() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        fs::write(temp_dir.path().join("README.md"), "hello").expect("write md");
        let state = AppState::new(SqliteStore::open_in_memory().expect("sqlite opens"));
        let created = state
            .create_knowledge_space(
                "测试知识库".to_string(),
                temp_dir.path().to_string_lossy().to_string(),
                PermissionMode::Approval,
            )
            .expect("space created");
        let space_id = created.active_space_id;
        state
            .prepare_scan_knowledge_space(space_id.clone())
            .expect("scan job starts");

        state
            .run_next_scan_job_with_scanner(space_id.clone(), |_root_path, job_id| {
                state
                    .cancel_parse_job(job_id.to_string())
                    .expect("running scan cancels");
                Ok(vec![ScannedFile {
                    relative_path: "README.md".to_string(),
                    extension: "md".to_string(),
                    size_bytes: 5,
                    modified_at: "2026-06-22T00:00:00Z".to_string(),
                    content_hash: "hash-readme".to_string(),
                }])
            })
            .expect("cancelled scan returns without failing command");
        state.finish_scan_worker(&space_id);
        let snapshot = state.snapshot().expect("snapshot builds");

        assert!(snapshot.files.is_empty());
        assert!(snapshot
            .parse_jobs
            .iter()
            .any(|job| job.job_type == "scan" && job.status == "cancelled"));
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
        let scanned = state
            .scan_knowledge_space(created.active_space_id.clone())
            .expect("space scanned");

        state
            .run_next_document_parse_job_with_parser(
                scanned.active_space_id,
                |root_path, candidate| {
                    let file_candidate = crate::models::FileParseCandidate {
                        file_id: candidate.file_id.clone(),
                        relative_path: candidate.relative_path.clone(),
                        extension: candidate.extension.clone(),
                    };
                    parse_file(root_path, &file_candidate)
                },
            )
            .expect("document job runs");
        let indexed = state.snapshot().expect("snapshot builds");

        assert_eq!(indexed.files[0].status_label, "已索引");
        assert_eq!(indexed.parse_jobs[0].status, "succeeded");
        assert!(indexed.block_preview.excerpt.contains("缓存穿透"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn indexes_xlsx_table_insight_into_snapshot_preview_and_agent_sources() {
        let _env_lock = env_lock().lock().expect("env lock");
        let _api_key_guard = EnvVarGuard::set("DEEPSEEK_API_KEY", "test-local-key");
        let _base_url_guard = EnvVarGuard::set("DEEPSEEK_BASE_URL", "http://127.0.0.1:9");
        let temp_dir = tempfile::tempdir().expect("temp dir");
        fs::write(temp_dir.path().join("经营报表.xlsx"), "xlsx").expect("write xlsx");
        let state = AppState::new(SqliteStore::open_in_memory().expect("sqlite opens"));
        let created = state
            .create_knowledge_space(
                "报表".to_string(),
                temp_dir.path().to_string_lossy().to_string(),
                PermissionMode::Approval,
            )
            .expect("space created");
        let scanned = state
            .scan_knowledge_space(created.active_space_id.clone())
            .expect("space scanned");

        state
            .run_next_document_parse_job_with_parser(scanned.active_space_id, |_root, candidate| {
                Ok(crate::models::ParsedDocument {
                    title: candidate.relative_path.clone(),
                    body: "Excel 工作簿原始文本".to_string(),
                    summary: "Excel 工作簿原始文本".to_string(),
                    source_locator: candidate.relative_path.clone(),
                    segments: Vec::new(),
                    table_insights: vec![ParsedTableInsight {
                        title: "经营报表.xlsx · 工作表 1".to_string(),
                        body: "经营报表.xlsx · 工作表 1 结构：3 行，3 列 表头：月份、营收、成本 样例 1：2026-06 | 120 | 70".to_string(),
                        summary: "工作表 1：3 行、3 列；表头：月份、营收、成本".to_string(),
                        source_locator: "经营报表.xlsx#sheet-001".to_string(),
                    }],
                })
            })
            .expect("document job runs");
        let indexed = state.snapshot().expect("snapshot builds");

        assert_eq!(indexed.files[0].status_label, "已索引");
        assert_eq!(indexed.table_preview.title, "经营报表.xlsx · 工作表 1");
        assert!(indexed
            .table_preview
            .description
            .contains("月份、营收、成本"));

        let answered = state
            .ask_agent(indexed.active_space_id, "2026-06 营收".to_string())
            .await
            .expect("agent answers from table insight");
        let assistant_message = answered
            .messages
            .iter()
            .find(|message| message.role == ChatRole::Assistant)
            .expect("assistant message exists");

        assert!(assistant_message.content.contains("[表格洞察]"));
        assert_eq!(assistant_message.sources.len(), 1);
        assert_eq!(assistant_message.sources[0].source_kind, "table");
        assert_eq!(
            assistant_message.sources[0].source_locator,
            "经营报表.xlsx#sheet-001"
        );
    }

    #[test]
    fn failed_document_job_can_be_retried_into_searchable_block() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        fs::write(
            temp_dir.path().join("Redis面试.md"),
            "Redis 缓存穿透需要空值缓存。",
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
        let scanned = state
            .scan_knowledge_space(created.active_space_id)
            .expect("space scanned");
        let space_id = scanned.active_space_id.clone();

        state
            .run_next_document_parse_job_with_parser(space_id.clone(), |_root_path, _candidate| {
                Err(crate::error::AppError::Filesystem(
                    "DOC_PARSE_EMPTY".to_string(),
                ))
            })
            .expect("document failure is recorded");
        let failed = state.snapshot().expect("snapshot builds");
        assert_eq!(failed.files[0].status_label, "扫描失败");
        assert!(failed
            .parse_jobs
            .iter()
            .any(|job| job.job_type == "document" && job.status == "failed"));

        assert!(state
            .prepare_document_indexing(space_id.clone())
            .expect("retry queues document job"));
        state.finish_document_worker(&space_id);
        state
            .run_next_document_parse_job_with_parser(space_id, |root_path, candidate| {
                let file_candidate = crate::models::FileParseCandidate {
                    file_id: candidate.file_id.clone(),
                    relative_path: candidate.relative_path.clone(),
                    extension: candidate.extension.clone(),
                };
                parse_file(root_path, &file_candidate)
            })
            .expect("retry succeeds");
        let retried = state.snapshot().expect("snapshot builds");

        assert_eq!(retried.files[0].status_label, "已索引");
        assert!(retried.block_preview.excerpt.contains("缓存穿透"));
        assert!(retried
            .parse_jobs
            .iter()
            .any(|job| job.job_type == "document" && job.status == "succeeded"));
    }

    #[test]
    fn cancelled_running_document_job_does_not_write_successful_output() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        fs::write(temp_dir.path().join("README.md"), "hello").expect("write md");
        let state = AppState::new(SqliteStore::open_in_memory().expect("sqlite opens"));
        let created = state
            .create_knowledge_space(
                "文档".to_string(),
                temp_dir.path().to_string_lossy().to_string(),
                PermissionMode::Approval,
            )
            .expect("space created");
        let scanned = state
            .scan_knowledge_space(created.active_space_id)
            .expect("space scanned");

        state
            .run_next_document_parse_job_with_parser(
                scanned.active_space_id,
                |candidate_root, candidate| {
                    state
                        .cancel_parse_job(candidate.job_id.clone())
                        .expect("running job cancels");
                    Ok(crate::models::ParsedDocument {
                        title: candidate.relative_path.clone(),
                        body: format!("这段取消后的文档文本不应入库：{}", candidate_root.display()),
                        summary: "不应入库".to_string(),
                        source_locator: candidate.relative_path.clone(),
                        segments: Vec::new(),
                        table_insights: Vec::new(),
                    })
                },
            )
            .expect("cancelled document returns without failing command");
        let snapshot = state.snapshot().expect("snapshot builds");
        let document_job = snapshot
            .parse_jobs
            .iter()
            .find(|job| job.job_type == "document")
            .expect("document job exists");

        assert_eq!(document_job.status, "cancelled");
        assert_eq!(document_job.phase, "已取消");
        assert!(!snapshot.block_preview.excerpt.contains("不应入库"));
    }

    #[test]
    fn cancelled_queued_document_job_does_not_block_next_job() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        fs::write(temp_dir.path().join("A.md"), "A 文档").expect("write a");
        fs::write(temp_dir.path().join("B.md"), "B 文档").expect("write b");
        let state = AppState::new(SqliteStore::open_in_memory().expect("sqlite opens"));
        let created = state
            .create_knowledge_space(
                "文档".to_string(),
                temp_dir.path().to_string_lossy().to_string(),
                PermissionMode::Approval,
            )
            .expect("space created");
        let scanned = state
            .scan_knowledge_space(created.active_space_id)
            .expect("space scanned");
        let first_job_id = scanned
            .parse_jobs
            .iter()
            .find(|job| job.file_name == "A.md")
            .expect("first document job exists")
            .id
            .clone();
        state
            .cancel_parse_job(first_job_id)
            .expect("queued first job cancels");

        state
            .run_next_document_parse_job_with_parser(
                scanned.active_space_id,
                |root_path, candidate| {
                    let file_candidate = crate::models::FileParseCandidate {
                        file_id: candidate.file_id.clone(),
                        relative_path: candidate.relative_path.clone(),
                        extension: candidate.extension.clone(),
                    };
                    parse_file(root_path, &file_candidate)
                },
            )
            .expect("next document job runs");
        let snapshot = state.snapshot().expect("snapshot builds");
        let cancelled_job = snapshot
            .parse_jobs
            .iter()
            .find(|job| job.file_name == "A.md")
            .expect("cancelled job exists");
        let succeeded_job = snapshot
            .parse_jobs
            .iter()
            .find(|job| job.file_name == "B.md")
            .expect("succeeded job exists");

        assert_eq!(cancelled_job.status, "cancelled");
        assert_eq!(succeeded_job.status, "succeeded");
        assert!(snapshot.block_preview.source_file_name == "B.md");
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
            .enqueue_ocr_parse_job(scanned.active_space_id, scanned.files[0].id.clone(), None)
            .expect("ocr job queued");

        assert!(queued.parse_jobs.iter().any(|job| {
            job.job_type == "ocr" && job.file_id.as_deref() == Some(scanned.files[0].id.as_str())
        }));
    }

    #[test]
    fn enqueues_image_ocr_job_when_models_are_ready() {
        let knowledge_dir = tempfile::tempdir().expect("knowledge dir");
        fs::write(knowledge_dir.path().join("scan.png"), "image").expect("write image");
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
            .enqueue_ocr_parse_job(scanned.active_space_id, scanned.files[0].id.clone(), None)
            .expect("ocr job queued");

        assert_eq!(queued.files[0].extension, ".png");
        assert!(queued.parse_jobs.iter().any(|job| {
            job.job_type == "ocr" && job.file_id.as_deref() == Some(scanned.files[0].id.as_str())
        }));
    }

    #[test]
    fn document_worker_gate_allows_one_worker_per_space_until_finished() {
        let knowledge_dir = tempfile::tempdir().expect("knowledge dir");
        fs::write(knowledge_dir.path().join("README.md"), "hello").expect("write md");
        let state = AppState::new(SqliteStore::open_in_memory().expect("sqlite opens"));
        let created = state
            .create_knowledge_space(
                "文档".to_string(),
                knowledge_dir.path().to_string_lossy().to_string(),
                PermissionMode::Approval,
            )
            .expect("space created");
        let scanned = state
            .scan_knowledge_space(created.active_space_id)
            .expect("space scanned");
        let space_id = scanned.active_space_id;

        assert!(state
            .begin_document_worker(space_id.clone())
            .expect("first worker starts"));
        assert!(!state
            .begin_document_worker(space_id.clone())
            .expect("second worker is rejected"));

        state.finish_document_worker(&space_id);

        assert!(state
            .begin_document_worker(space_id.clone())
            .expect("worker can restart while queue still has work"));
        state.finish_document_worker(&space_id);
    }

    #[test]
    fn ocr_worker_gate_allows_one_worker_per_space_until_finished() {
        let knowledge_dir = tempfile::tempdir().expect("knowledge dir");
        let state = AppState::new(SqliteStore::open_in_memory().expect("sqlite opens"));
        let created = state
            .create_knowledge_space(
                "OCR".to_string(),
                knowledge_dir.path().to_string_lossy().to_string(),
                PermissionMode::Approval,
            )
            .expect("space created");
        let space_id = created.active_space_id;

        assert!(state
            .begin_ocr_worker(space_id.clone())
            .expect("first worker starts"));
        assert!(!state
            .begin_ocr_worker(space_id.clone())
            .expect("second worker is rejected"));

        state.finish_ocr_worker(&space_id);

        assert!(state
            .begin_ocr_worker(space_id.clone())
            .expect("worker can restart after finish"));
        state.finish_ocr_worker(&space_id);
    }

    #[test]
    fn rejects_non_ocr_supported_jobs_before_queueing() {
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
            state.enqueue_ocr_parse_job(scanned.active_space_id, scanned.files[0].id.clone(), None);

        assert!(result
            .expect_err("md file is rejected")
            .to_string()
            .contains("仅支持 PDF、图片文件或 DOCX 文档图片"));
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
            .enqueue_ocr_parse_job(
                scanned.active_space_id.clone(),
                scanned.files[0].id.clone(),
                None,
            )
            .expect("ocr job queued");

        state
            .run_next_ocr_parse_job_with_runner(
                scanned.active_space_id,
                |_candidate, _request, _progress| {
                    Ok(OcrSidecarResult {
                        text: "扫描版 PDF 的本地 OCR 文本".to_string(),
                        page_count: 1,
                        pages: vec![OcrPageResult {
                            page_index: 0,
                            text: "扫描版 PDF 的本地 OCR 文本".to_string(),
                            confidence: Some(0.93),
                            line_count: None,
                            char_count: None,
                        }],
                    })
                },
            )
            .expect("ocr job runs");
        let snapshot = state.snapshot().expect("snapshot builds");

        let ocr_job = snapshot
            .parse_jobs
            .iter()
            .find(|job| job.job_type == "ocr")
            .expect("ocr job exists");
        assert_eq!(snapshot.files[0].status_label, "已索引");
        assert_eq!(ocr_job.status, "succeeded");
        assert_eq!(ocr_job.phase, "已完成");
        assert!(snapshot.block_preview.excerpt.contains("OCR 文本"));
    }

    #[test]
    fn runs_next_image_ocr_job_into_searchable_knowledge_block() {
        let knowledge_dir = tempfile::tempdir().expect("knowledge dir");
        fs::write(knowledge_dir.path().join("scan.png"), "image").expect("write image");
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
            .enqueue_ocr_parse_job(
                scanned.active_space_id.clone(),
                scanned.files[0].id.clone(),
                None,
            )
            .expect("ocr job queued");

        state
            .run_next_ocr_parse_job_with_runner(
                scanned.active_space_id,
                |_candidate, request, _progress| {
                    assert!(request.file_path.ends_with("scan.png"));
                    Ok(OcrSidecarResult {
                        text: "截图里的本地 OCR 文本".to_string(),
                        page_count: 1,
                        pages: vec![OcrPageResult {
                            page_index: 0,
                            text: "截图里的本地 OCR 文本".to_string(),
                            confidence: Some(0.91),
                            line_count: None,
                            char_count: None,
                        }],
                    })
                },
            )
            .expect("image ocr job runs");
        let snapshot = state.snapshot().expect("snapshot builds");

        let ocr_job = snapshot
            .parse_jobs
            .iter()
            .find(|job| job.job_type == "ocr")
            .expect("ocr job exists");
        assert_eq!(snapshot.files[0].status_label, "已索引");
        assert_eq!(ocr_job.status, "succeeded");
        assert!(snapshot
            .block_preview
            .excerpt
            .contains("截图里的本地 OCR 文本"));
    }

    #[test]
    fn ocr_runner_progress_updates_parse_job_phase() {
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
            .enqueue_ocr_parse_job(
                scanned.active_space_id.clone(),
                scanned.files[0].id.clone(),
                None,
            )
            .expect("ocr job queued");

        state
            .run_next_ocr_parse_job_with_runner(
                scanned.active_space_id,
                |_candidate, request, progress| {
                    assert!(request.progress);
                    progress(crate::ocr::OcrProgressUpdate {
                        phase: "已识别第 1/2 页".to_string(),
                        current: 1,
                        total: 2,
                    });
                    let snapshot = state.snapshot().expect("snapshot builds during progress");
                    let ocr_job = snapshot
                        .parse_jobs
                        .iter()
                        .find(|job| job.job_type == "ocr")
                        .expect("ocr job exists");
                    assert_eq!(ocr_job.phase, "已识别第 1/2 页");
                    assert_eq!(ocr_job.progress_current, 1);
                    assert_eq!(ocr_job.progress_total, 2);

                    Ok(OcrSidecarResult {
                        text: "分段进度后的 OCR 文本".to_string(),
                        page_count: 2,
                        pages: vec![OcrPageResult {
                            page_index: 0,
                            text: "分段进度后的 OCR 文本".to_string(),
                            confidence: Some(0.93),
                            line_count: None,
                            char_count: None,
                        }],
                    })
                },
            )
            .expect("ocr job runs");
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
            .enqueue_ocr_parse_job(
                scanned.active_space_id.clone(),
                scanned.files[0].id.clone(),
                None,
            )
            .expect("ocr job queued");
        let space_id = scanned.active_space_id.clone();
        state
            .run_next_ocr_parse_job_with_runner(
                space_id.clone(),
                |_candidate, _request, _progress| {
                    Ok(OcrSidecarResult {
                        text: "扫描版 PDF 的本地 OCR 文本".to_string(),
                        page_count: 1,
                        pages: vec![OcrPageResult {
                            page_index: 0,
                            text: "扫描版 PDF 的本地 OCR 文本".to_string(),
                            confidence: Some(0.93),
                            line_count: None,
                            char_count: None,
                        }],
                    })
                },
            )
            .expect("ocr job runs");
        let indexed = state.snapshot().expect("snapshot builds");
        assert_eq!(indexed.block_preview.source_file_name, "scan.pdf");
        assert_eq!(
            indexed.block_preview.source_locator,
            "scan.pdf#ocr-page-001"
        );

        let answered = state
            .ask_agent(indexed.active_space_id, "扫描版".to_string())
            .await
            .expect("agent answers from local index");

        assert!(answered.messages.iter().any(|message| {
            message.role == ChatRole::Assistant && message.content.contains("本地 OCR 文本")
        }));
        let assistant_message = answered
            .messages
            .iter()
            .find(|message| message.role == ChatRole::Assistant)
            .expect("assistant message exists");
        assert_eq!(assistant_message.sources.len(), 1);
        assert_eq!(assistant_message.sources[0].source_file_name, "scan.pdf");
        assert!(assistant_message.sources[0]
            .excerpt
            .contains("本地 OCR 文本"));
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
            .enqueue_ocr_parse_job(
                scanned.active_space_id.clone(),
                scanned.files[0].id.clone(),
                None,
            )
            .expect("ocr job queued");

        state
            .run_next_ocr_parse_job_with_runner(
                scanned.active_space_id,
                |_candidate, _request, _progress| {
                    Err(crate::error::AppError::Filesystem(
                        "OCR_EMPTY_RESULT".to_string(),
                    ))
                },
            )
            .expect("failure is recorded in snapshot");
        let snapshot = state.snapshot().expect("snapshot builds");

        let ocr_job = snapshot
            .parse_jobs
            .iter()
            .find(|job| job.job_type == "ocr")
            .expect("ocr job exists");
        assert_eq!(snapshot.files[0].status_label, "扫描失败");
        assert_eq!(ocr_job.status, "failed");
        assert_eq!(
            ocr_job.error_message.as_deref(),
            Some("文件系统错误：OCR_EMPTY_RESULT")
        );

        let failed_job_id = ocr_job.id.clone();
        let retried = state
            .enqueue_ocr_parse_job(
                snapshot.active_space_id.clone(),
                snapshot.files[0].id.clone(),
                None,
            )
            .expect("failed ocr job can be retried");
        let ocr_jobs = retried
            .parse_jobs
            .iter()
            .filter(|job| job.job_type == "ocr")
            .collect::<Vec<_>>();
        let active_job_count = ocr_jobs
            .iter()
            .filter(|job| matches!(job.status.as_str(), "queued" | "running"))
            .count();

        assert_eq!(ocr_jobs.len(), 2);
        assert_eq!(active_job_count, 1);
        assert!(ocr_jobs
            .iter()
            .any(|job| job.id == failed_job_id && job.status == "failed"));
        assert!(ocr_jobs.iter().any(|job| {
            job.id != failed_job_id && job.status == "queued" && job.error_message.is_none()
        }));
    }

    #[test]
    fn cancelled_running_ocr_job_does_not_write_successful_output() {
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
            .enqueue_ocr_parse_job(
                scanned.active_space_id.clone(),
                scanned.files[0].id.clone(),
                None,
            )
            .expect("ocr job queued");

        state
            .run_next_ocr_parse_job_with_runner(
                scanned.active_space_id,
                |candidate, _request, _progress| {
                    state
                        .cancel_parse_job(candidate.job_id.clone())
                        .expect("running job cancels");
                    Ok(OcrSidecarResult {
                        text: "这段取消后的 OCR 文本不应入库".to_string(),
                        page_count: 1,
                        pages: vec![OcrPageResult {
                            page_index: 0,
                            text: "这段取消后的 OCR 文本不应入库".to_string(),
                            confidence: Some(0.91),
                            line_count: None,
                            char_count: None,
                        }],
                    })
                },
            )
            .expect("cancelled job returns without failing command");
        let snapshot = state.snapshot().expect("snapshot builds");

        let ocr_job = snapshot
            .parse_jobs
            .iter()
            .find(|job| job.job_type == "ocr")
            .expect("ocr job exists");
        assert_eq!(ocr_job.status, "cancelled");
        assert_eq!(ocr_job.phase, "已取消");
        assert!(!snapshot.block_preview.excerpt.contains("不应入库"));
    }

    #[test]
    fn cancelled_running_ocr_job_does_not_mark_file_failed_after_runner_error() {
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
            .enqueue_ocr_parse_job(
                scanned.active_space_id.clone(),
                scanned.files[0].id.clone(),
                None,
            )
            .expect("ocr job queued");

        state
            .run_next_ocr_parse_job_with_runner(
                scanned.active_space_id,
                |candidate, _request, _progress| {
                    state
                        .cancel_parse_job(candidate.job_id.clone())
                        .expect("running job cancels");
                    Err(crate::error::AppError::Filesystem(
                        "OCR_CANCELLED".to_string(),
                    ))
                },
            )
            .expect("cancelled runner error returns without failing command");
        let snapshot = state.snapshot().expect("snapshot builds");

        let ocr_job = snapshot
            .parse_jobs
            .iter()
            .find(|job| job.job_type == "ocr")
            .expect("ocr job exists");
        assert_eq!(ocr_job.status, "cancelled");
        assert_eq!(snapshot.files[0].status_label, "待解析");
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

    fn write_backup_fixture(app_data_dir: &Path, file_name: &str) -> PathBuf {
        let backup_dir = app_data_dir.join("backups");
        fs::create_dir_all(&backup_dir).expect("backup dir");
        let backup_path = backup_dir.join(file_name);
        fs::write(
            &backup_path,
            r#"{
  "format": "library.backup.v1",
  "schemaVersion": 1,
  "exportedAt": "2026-06-23T00:00:00Z",
  "space": {
    "id": "backup-space",
    "name": "备份空间",
    "rootPath": "D:\\知识库\\备份空间",
    "defaultPermission": "approval",
    "createdAt": "2026-06-22T00:00:00Z",
    "updatedAt": "2026-06-23T00:00:00Z"
  },
  "workspace": {
    "activeSpaceId": "backup-space",
    "defaultPermission": "approval"
  },
  "files": [
    {
      "id": "file-redis",
      "relativePath": "Redis面试.md",
      "extension": "md",
      "contentHash": "hash-redis",
      "sizeBytes": 128,
      "modifiedAt": "2026-06-22T00:00:00Z",
      "parseStatus": "indexed",
      "lastScannedAt": "2026-06-23T00:00:00Z",
      "createdAt": "2026-06-22T00:00:00Z",
      "updatedAt": "2026-06-23T00:00:00Z",
      "deletedAt": null
    }
  ],
  "markdownNotes": [],
  "knowledgeBlocks": [
    {
      "id": "block-redis",
      "fileId": "file-redis",
      "noteId": null,
      "title": "Redis 缓存",
      "body": "缓存穿透需要空值缓存。",
      "sourceKind": "original_file",
      "sourceLocator": "Redis面试.md",
      "searchable": true,
      "createdAt": "2026-06-22T00:00:00Z",
      "updatedAt": "2026-06-23T00:00:00Z",
      "deletedAt": null
    }
  ],
  "parseJobs": [],
  "trashEntries": []
}"#,
        )
        .expect("backup fixture");
        backup_path
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
