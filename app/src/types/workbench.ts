export type PermissionMode = "readonly" | "approval" | "full";

export type ChatScope = "current_file" | "current_folder" | "all";

export type ParseStatus = "indexed" | "changed" | "queued" | "failed";

export interface KnowledgeSpace {
  id: string;
  name: string;
  path: string;
  defaultPermission: PermissionMode;
  changedFileCount: number;
  scanQueueCount: number;
  documentQueueCount: number;
  ocrQueueCount: number;
}

export interface KnowledgeFile {
  id: string;
  name: string;
  extension: string;
  status: ParseStatus;
  statusLabel: string;
}

export interface KnowledgeBlockPreview {
  id: string;
  title: string;
  excerpt: string;
  sourceFileName: string;
  sourceLocator: string;
  sourceKind?: string;
}

export interface KnowledgeBlockContext {
  currentIndex: number;
  totalCount: number;
  blocks: KnowledgeBlockPreview[];
}

export interface TableInsightPreview {
  id: string;
  title: string;
  description: string;
}

export interface ChatMessageSource {
  id: string;
  title: string;
  excerpt: string;
  sourceFileName: string;
  sourceLocator: string;
  sourceKind?: string;
}

export interface ChatMessage {
  id: string;
  conversationId: string;
  role: "user" | "assistant" | "system";
  content: string;
  sources: ChatMessageSource[];
  createdAt: string;
}

export interface PendingAction {
  id: string;
  label: string;
  requiresApproval: boolean;
}

export interface RuntimeStatus {
  deepseek: DeepSeekRuntimeStatus;
  ocr: OcrRuntimeStatus;
}

export interface DeepSeekRuntimeStatus {
  configured: boolean;
  model: string;
  baseUrl: string;
  keyHint: string;
}

export interface OcrRuntimeStatus {
  configured: boolean;
  tier: string;
  modelDir: string;
  missingModels: string[];
}

export interface OcrEnvironmentReport {
  ok: boolean;
  checks: OcrEnvironmentCheck[];
}

export interface OcrEnvironmentCheck {
  name: string;
  ok: boolean;
  message: string;
  details?: Record<string, unknown>;
}

export interface ParseJobSummary {
  id: string;
  fileId: string | null;
  fileName: string;
  sourceLocator: string | null;
  jobType: string;
  status: string;
  errorMessage: string | null;
  startedAt: string | null;
  finishedAt: string | null;
  progressCurrent: number;
  progressTotal: number;
  phase: string;
}

export interface BackupExportResult {
  path: string;
  fileName: string;
  sizeBytes: number;
  exportedAt: string;
  fileCount: number;
  knowledgeBlockCount: number;
  parseJobCount: number;
}

export interface BackupRestorePreflight {
  path: string;
  fileName: string;
  format: string;
  schemaVersion: number;
  exportedAt: string;
  spaceId: string;
  spaceName: string;
  rootPath: string;
  defaultPermission: PermissionMode;
  fileCount: number;
  knowledgeBlockCount: number;
  parseJobCount: number;
  trashEntryCount: number;
  willOverwrite: boolean;
}

export interface BackupRestoreResult extends BackupRestorePreflight {
  restoredAt: string;
  overwritten: boolean;
}

export interface WorkbenchSnapshot {
  spaces: KnowledgeSpace[];
  activeSpaceId: string;
  activeConversationId: string | null;
  activeScope: ChatScope;
  sessionPermission: PermissionMode;
  files: KnowledgeFile[];
  parseJobs: ParseJobSummary[];
  blockPreview: KnowledgeBlockPreview;
  tablePreview: TableInsightPreview;
  messages: ChatMessage[];
  pendingAction: PendingAction | null;
}

export interface Conversation {
  id: string;
  spaceId: string;
  title: string;
  createdAt: string;
  updatedAt: string;
}

export interface SwitchConversationRequest {
  conversationId: string | null;
}

export interface CreateConversationRequest {
  spaceId: string;
  title: string;
}

export interface ListConversationsRequest {
  spaceId: string;
}
