export type PermissionMode = "readonly" | "approval" | "full";

export type ChatScope = "current_file" | "current_folder" | "all";

export type ParseStatus = "indexed" | "changed" | "queued" | "failed";

export interface KnowledgeSpace {
  id: string;
  name: string;
  path: string;
  defaultPermission: PermissionMode;
  changedFileCount: number;
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
}

export interface TableInsightPreview {
  id: string;
  title: string;
  description: string;
}

export interface ChatMessage {
  id: string;
  role: "user" | "assistant" | "system";
  content: string;
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

export interface ParseJobSummary {
  id: string;
  fileId: string | null;
  fileName: string;
  jobType: string;
  status: string;
  errorMessage: string | null;
  startedAt: string | null;
  finishedAt: string | null;
  progressCurrent: number;
  progressTotal: number;
  phase: string;
}

export interface WorkbenchSnapshot {
  spaces: KnowledgeSpace[];
  activeSpaceId: string;
  activeScope: ChatScope;
  sessionPermission: PermissionMode;
  files: KnowledgeFile[];
  parseJobs: ParseJobSummary[];
  blockPreview: KnowledgeBlockPreview;
  tablePreview: TableInsightPreview;
  messages: ChatMessage[];
  pendingAction: PendingAction | null;
}
