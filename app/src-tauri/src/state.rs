use std::sync::Mutex;

use crate::models::{
    ChatMessage, ChatRole, ChatScope, KnowledgeBlockPreview, KnowledgeFile, KnowledgeSpace,
    ParseStatus, PendingAction, PermissionMode, TableInsightPreview, WorkbenchSnapshot,
};

pub struct AppState {
    snapshot: Mutex<WorkbenchSnapshot>,
}

impl AppState {
    pub fn new_with_mock_data() -> Self {
        Self {
            snapshot: Mutex::new(WorkbenchSnapshot {
                active_space_id: "space-interview".to_string(),
                active_scope: ChatScope::CurrentFolder,
                session_permission: PermissionMode::Approval,
                spaces: vec![
                    KnowledgeSpace {
                        id: "space-interview".to_string(),
                        name: "面试".to_string(),
                        path: "D:\\知识库\\面试".to_string(),
                        default_permission: PermissionMode::Approval,
                        changed_file_count: 2,
                        ocr_queue_count: 1,
                    },
                    KnowledgeSpace {
                        id: "space-springboot".to_string(),
                        name: "SpringBoot".to_string(),
                        path: "D:\\知识库\\SpringBoot".to_string(),
                        default_permission: PermissionMode::Readonly,
                        changed_file_count: 0,
                        ocr_queue_count: 0,
                    },
                    KnowledgeSpace {
                        id: "space-work".to_string(),
                        name: "工作项目A".to_string(),
                        path: "D:\\知识库\\工作项目A".to_string(),
                        default_permission: PermissionMode::Readonly,
                        changed_file_count: 1,
                        ocr_queue_count: 0,
                    },
                ],
                files: vec![
                    KnowledgeFile {
                        id: "file-java-docx".to_string(),
                        name: "Java面试八股.docx".to_string(),
                        extension: ".docx".to_string(),
                        status: ParseStatus::Indexed,
                        status_label: "已索引".to_string(),
                    },
                    KnowledgeFile {
                        id: "file-redis-pdf".to_string(),
                        name: "Redis缓存.pdf".to_string(),
                        extension: ".pdf".to_string(),
                        status: ParseStatus::Changed,
                        status_label: "已变更".to_string(),
                    },
                    KnowledgeFile {
                        id: "file-interview-xlsx".to_string(),
                        name: "面试题.xlsx".to_string(),
                        extension: ".xlsx".to_string(),
                        status: ParseStatus::Indexed,
                        status_label: "表格模型就绪".to_string(),
                    },
                ],
                block_preview: KnowledgeBlockPreview {
                    id: "block-redis-cache-penetration".to_string(),
                    title: "知识块预览".to_string(),
                    excerpt: "Redis 缓存穿透：请求查询不存在的数据，缓存和数据库都无法命中，导致请求直接打到数据库。"
                        .to_string(),
                    source_file_name: "Redis缓存.pdf".to_string(),
                },
                table_preview: TableInsightPreview {
                    id: "table-interview-question-bank".to_string(),
                    title: "表格理解".to_string(),
                    description: "识别工作表、表头、字段含义、单位和可问答指标，不做复杂报表仪表盘。"
                        .to_string(),
                },
                messages: vec![
                    ChatMessage {
                        id: "msg-user-1".to_string(),
                        role: ChatRole::User,
                        content: "问：Redis 缓存穿透怎么回答面试？".to_string(),
                    },
                    ChatMessage {
                        id: "msg-assistant-1".to_string(),
                        role: ChatRole::Assistant,
                        content: "可以从定义、风险、解决方案和追问点四段回答。我会引用 3 个来源块。"
                            .to_string(),
                    },
                ],
                pending_action: Some(PendingAction {
                    id: "action-flash-card-draft".to_string(),
                    label: "待批准操作：生成复习卡草稿，批准后保存。".to_string(),
                    requires_approval: true,
                }),
            }),
        }
    }

    pub fn snapshot(&self) -> WorkbenchSnapshot {
        self.snapshot
            .lock()
            .expect("workbench snapshot mutex poisoned")
            .clone()
    }

    pub fn set_session_permission(&self, permission: PermissionMode) {
        self.snapshot
            .lock()
            .expect("workbench snapshot mutex poisoned")
            .session_permission = permission;
    }
}
