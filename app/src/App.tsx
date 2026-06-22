import bookmarkPlusIcon from "@iconify-icons/lucide/bookmark-plus";
import checkIcon from "@iconify-icons/lucide/check";
import chevronDownIcon from "@iconify-icons/lucide/chevron-down";
import eyeIcon from "@iconify-icons/lucide/eye";
import fileSearchIcon from "@iconify-icons/lucide/file-search";
import folderPlusIcon from "@iconify-icons/lucide/folder-plus";
import refreshCwIcon from "@iconify-icons/lucide/refresh-cw";
import sendIcon from "@iconify-icons/lucide/send";
import settingsIcon from "@iconify-icons/lucide/settings";
import xIcon from "@iconify-icons/lucide/x";
import { Icon } from "@iconify/react";
import { type FormEvent, useState } from "react";

import { useRuntimeStatus } from "./hooks/useRuntimeStatus";
import { useWorkbenchSnapshot } from "./hooks/useWorkbenchSnapshot";
import type {
  ChatMessage,
  ChatScope,
  KnowledgeFile,
  PermissionMode,
} from "./types/workbench";
import styles from "./App.module.css";

const permissionLabel: Record<PermissionMode, string> = {
  readonly: "只读",
  approval: "请求批准",
  full: "完全访问",
};

const scopeLabel: Record<ChatScope, string> = {
  current_file: "当前文件",
  current_folder: "当前文件夹",
  all: "全库",
};

const tabs = ["总览", "文件", "知识块", "表格", "回收站"];
const scopes = Object.keys(scopeLabel) as ChatScope[];
const permissionOptions = Object.keys(permissionLabel) as PermissionMode[];

function fileStatusClass(file: KnowledgeFile) {
  if (file.status === "changed") return styles.statusChanged;
  if (file.status === "queued") return styles.statusQueued;
  if (file.status === "failed") return styles.statusFailed;
  return styles.statusIndexed;
}

function messageClass(message: ChatMessage) {
  if (message.role === "assistant") return styles.messageAssistant;
  if (message.role === "system") return styles.messageSystem;
  return styles.messageUser;
}

export default function App() {
  const [showDefaultPermissionHelp, setShowDefaultPermissionHelp] =
    useState(false);
  const [question, setQuestion] = useState("");
  const {
    snapshot,
    error,
    loading,
    askAgentQuestion,
    cancelJob,
    createSpaceFromFolder,
    enqueueOcrJob,
    indexActiveSpace,
    scanActiveSpace,
    setFolderDefaultPermission,
    setSessionPermission,
  } = useWorkbenchSnapshot();
  const { runtimeStatus, runtimeStatusError } = useRuntimeStatus();
  const activeSpace =
    snapshot.spaces.find((space) => space.id === snapshot.activeSpaceId) ??
    snapshot.spaces[0] ??
    null;
  const hasActiveSpace = activeSpace !== null;
  const defaultPermission = activeSpace?.defaultPermission ?? "readonly";
  const changedFileCount = activeSpace?.changedFileCount ?? 0;
  const ocrQueueCount = activeSpace?.ocrQueueCount ?? 0;
  const activeOcrFileIds = new Set(
    snapshot.parseJobs
      .filter(
        (job) =>
          job.jobType === "ocr" &&
          (job.status === "queued" || job.status === "running") &&
          Boolean(job.fileId),
      )
      .flatMap((job) => (job.fileId ? [job.fileId] : [])),
  );
  const canAskAgent = hasActiveSpace && question.trim().length > 0 && !loading;

  function handleAskAgent(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!canAskAgent) {
      return;
    }

    const submittedQuestion = question.trim();
    setQuestion("");
    void askAgentQuestion(submittedQuestion);
  }

  return (
    <div className={styles.shell}>
      <aside className={styles.sidebar} aria-label="知识库导航">
        <div className={styles.sidebarHeader}>
          <h1 className={styles.title}>知识库</h1>
          <button
            className={styles.ghostButton}
            disabled={loading}
            onClick={() => void createSpaceFromFolder("approval")}
            type="button"
          >
            <Icon aria-hidden icon={folderPlusIcon} />
            <span>新建</span>
          </button>
        </div>

        <input
          aria-label="搜索知识库"
          className={styles.searchBox}
          placeholder="搜索文件、笔记、表格"
          type="search"
        />

        <section className={styles.spaceSection}>
          <div className={styles.sectionLabel}>文件夹列表</div>
          <nav className={styles.spaceList} aria-label="文件夹列表">
            {snapshot.spaces.length > 0 ? (
              snapshot.spaces.map((space) => (
                <button
                  className={`${styles.spaceItem} ${
                    space.id === activeSpace?.id ? styles.spaceItemActive : ""
                  }`}
                  key={space.id}
                  type="button"
                >
                  <span className={styles.spaceName}>{space.name}</span>
                  <span className={styles.spacePath}>{space.path}</span>
                  <span className={styles.spaceMeta}>
                    变更 {space.changedFileCount} · OCR 队列 {space.ocrQueueCount}
                  </span>
                </button>
              ))
            ) : (
              <div className={styles.emptyState}>暂无知识库文件夹</div>
            )}
          </nav>
        </section>

        <section className={styles.defaultPermission} aria-label="默认权限">
          <div className={styles.defaultPermissionHeader}>
            <span>默认权限</span>
            <button
              aria-label="打开默认权限设置"
              className={styles.iconButton}
              aria-expanded={showDefaultPermissionHelp}
              onClick={() =>
                setShowDefaultPermissionHelp((currentValue) => !currentValue)
              }
              title="默认权限设置"
              type="button"
            >
              <Icon aria-hidden icon={settingsIcon} />
            </button>
          </div>
          <select
            aria-label="切换文件夹默认权限"
            className={styles.defaultPermissionSelect}
            disabled={!hasActiveSpace}
            value={defaultPermission}
            onChange={(event) =>
              void setFolderDefaultPermission(event.target.value as PermissionMode)
            }
          >
            {permissionOptions.map((permission) => (
              <option key={permission} value={permission}>
                {permissionLabel[permission]}
              </option>
            ))}
          </select>
          {showDefaultPermissionHelp ? (
            <div className={styles.permissionHelp}>
              <strong>默认权限</strong>
              <p>
                默认权限是这个文件夹长期保存的 Agent 操作边界；右侧会话权限只影响当前聊天。
              </p>
              <div className={styles.runtimeStatus}>
                <div className={styles.runtimeRow}>
                  <span>DeepSeek</span>
                  <strong>
                    {runtimeStatus?.deepseek.model ?? "deepseek-v4-flash"}
                  </strong>
                </div>
                <div className={styles.runtimeMeta}>
                  {runtimeStatus?.deepseek.configured
                    ? `密钥 ${runtimeStatus.deepseek.keyHint}`
                    : "密钥未配置"}
                </div>
                <div className={styles.runtimeRow}>
                  <span>本地 OCR</span>
                  <strong>{runtimeStatus?.ocr.configured ? "已就绪" : "未就绪"}</strong>
                </div>
                <div className={styles.runtimeMeta}>
                  {runtimeStatusError ??
                    (runtimeStatus?.ocr.configured
                      ? `模型目录 ${runtimeStatus.ocr.modelDir}`
                      : `缺少 ${
                          runtimeStatus?.ocr.missingModels.join("、") ?? "OCR 模型"
                        }`)}
                </div>
              </div>
              <button
                className={styles.helpToggle}
                onClick={() => setShowDefaultPermissionHelp(false)}
                type="button"
              >
                <span>收起说明</span>
                <Icon aria-hidden icon={chevronDownIcon} />
              </button>
            </div>
          ) : null}
        </section>
      </aside>

      <main className={styles.main}>
        <header className={styles.folderHeader}>
          <div className={styles.folderTitleRow}>
            <h2 className={styles.folderName}>{activeSpace?.name ?? "未选择文件夹"}</h2>
            <span className={styles.folderPath}>
              {activeSpace?.path ?? "请先添加一个真实文件夹"}
            </span>
          </div>
          <div className={styles.folderActions}>
            <button
              className={styles.plainButton}
              disabled={!hasActiveSpace || loading}
              onClick={() => void scanActiveSpace()}
              type="button"
            >
              <Icon aria-hidden icon={refreshCwIcon} />
              <span>扫描</span>
            </button>
            <button
              className={styles.plainButton}
              disabled={!hasActiveSpace || loading}
              onClick={() => void indexActiveSpace()}
              type="button"
            >
              <Icon aria-hidden icon={fileSearchIcon} />
              <span>建索引/摘要</span>
            </button>
          </div>
        </header>

        <nav className={styles.tabs} aria-label="内容标签">
          {tabs.map((tab, index) => (
            <button
              aria-current={index === 0 ? "page" : undefined}
              className={index === 0 ? styles.tabActive : styles.tab}
              key={tab}
              type="button"
            >
              {tab}
            </button>
          ))}
        </nav>

        <section className={styles.contentGrid} aria-label="当前文件夹内容">
          <div className={styles.leftContent}>
            <div className={styles.statusLine}>
              <span>已索引 {snapshot.files.length} 个文件</span>
              <span>已变更 {changedFileCount} 个文件</span>
              <span>OCR 队列 {ocrQueueCount} 个</span>
              {loading ? <span>处理中</span> : null}
              {error ? <span>{error}</span> : null}
            </div>

            <article className={`${styles.panel} ${styles.panelPadded}`}>
              <div className={styles.panelKicker}>文件夹总览 README.md</div>
              <h3 className={styles.panelTitle}>
                {activeSpace ? `${activeSpace.name} 知识库总览` : "等待添加知识库"}
              </h3>
              <p className={styles.panelText}>
                {activeSpace
                  ? "当前阶段从真实文件夹读取文件元数据，扫描完成后会进入本地 SQLite 索引。"
                  : "添加文件夹后，这里会显示当前知识库的文件状态和后续解析结果。"}
              </p>
            </article>

            <section className={styles.panel} aria-label="文件列表">
              <div className={styles.fileHeader}>文件列表</div>
              {snapshot.files.length > 0 ? (
                snapshot.files.map((file) => (
                  <div className={styles.fileRow} key={file.id}>
                    <div>
                      <strong>{file.name}</strong>
                      <span>{file.extension}</span>
                    </div>
                    <div className={styles.fileActions}>
                      {file.extension.toLowerCase() === ".pdf" ? (
                        <button
                          aria-label={`排队 OCR ${file.name}`}
                          className={styles.queueButton}
                          disabled={loading || activeOcrFileIds.has(file.id)}
                          onClick={() => void enqueueOcrJob(file.id)}
                          title="排队 OCR"
                          type="button"
                        >
                          OCR
                        </button>
                      ) : null}
                      <span className={fileStatusClass(file)}>{file.statusLabel}</span>
                    </div>
                  </div>
                ))
              ) : (
                <div className={styles.emptyState}>暂无已扫描文件</div>
              )}
            </section>
          </div>

          <div className={styles.rightContent}>
            <article className={`${styles.panel} ${styles.panelPadded}`}>
              <div className={styles.panelKicker}>
                {snapshot.blockPreview.sourceFileName}
              </div>
              <h3 className={styles.panelTitle}>
                {snapshot.blockPreview.title}
              </h3>
              <p className={styles.blockExcerpt}>
                {snapshot.blockPreview.excerpt}
              </p>
              <div className={styles.buttonRow}>
                <button className={styles.plainButton} type="button">
                  <Icon aria-hidden icon={eyeIcon} />
                  <span>查看来源</span>
                </button>
                <button className={styles.plainButton} type="button">
                  <Icon aria-hidden icon={bookmarkPlusIcon} />
                  <span>加入复习</span>
                </button>
              </div>
            </article>

            <article className={`${styles.panel} ${styles.panelPadded}`}>
              <div className={styles.panelKicker}>表格理解预览</div>
              <h3 className={styles.panelTitle}>
                {snapshot.tablePreview.title}
              </h3>
              <p className={styles.panelText}>
                {snapshot.tablePreview.description}
              </p>
              <div className={styles.tablePreview}>
                <div>工作表</div>
                <div>字段含义</div>
                <div>可问答指标</div>
              </div>
            </article>

            {snapshot.parseJobs.length > 0 ? (
              <section className={`${styles.panel} ${styles.panelPadded}`} aria-label="解析队列">
                <div className={styles.panelKicker}>后台任务</div>
                <h3 className={styles.panelTitle}>解析队列</h3>
                <div className={styles.queueList}>
                  {snapshot.parseJobs.map((job) => (
                    <div className={styles.queueRow} key={job.id}>
                      <div>
                        <strong>{job.fileName}</strong>
                        <span>{job.jobType}</span>
                      </div>
                      <div className={styles.queueActions}>
                        <span className={styles.queueStatus}>{job.status}</span>
                        {job.status === "queued" ? (
                          <button
                            aria-label={`取消解析任务 ${job.fileName}`}
                            className={styles.iconButton}
                            disabled={loading}
                            onClick={() => void cancelJob(job.id)}
                            title="取消解析任务"
                            type="button"
                          >
                            <Icon aria-hidden icon={xIcon} />
                          </button>
                        ) : null}
                      </div>
                    </div>
                  ))}
                </div>
              </section>
            ) : null}
          </div>
        </section>
      </main>

      <aside className={styles.agent} aria-label="智能助手">
        <header className={styles.agentTop}>
          <div className={styles.agentHeader}>
            <h2 className={styles.agentTitle}>智能助手</h2>
            <label className={styles.permissionPicker}>
              <span>会话权限</span>
              <select
                aria-label="切换会话权限"
                className={styles.permissionSelect}
                disabled={!hasActiveSpace}
                value={snapshot.sessionPermission}
                onChange={(event) =>
                  void setSessionPermission(event.target.value as PermissionMode)
                }
              >
                {permissionOptions.map((permission) => (
                  <option key={permission} value={permission}>
                    {permissionLabel[permission]}
                  </option>
                ))}
              </select>
            </label>
          </div>
          <div className={styles.scopeLabel}>范围切换</div>
          <div className={styles.scopeGroup} aria-label="范围切换">
            {scopes.map((scope) => (
              <button
                aria-pressed={snapshot.activeScope === scope}
                className={
                  snapshot.activeScope === scope
                    ? styles.scopeActive
                    : styles.scope
                }
                disabled={!hasActiveSpace}
                key={scope}
                type="button"
              >
                {scopeLabel[scope]}
              </button>
            ))}
          </div>
        </header>

        <section className={styles.messages} aria-label="助手会话">
          {snapshot.messages.map((message) => (
            <article className={messageClass(message)} key={message.id}>
              {message.content}
            </article>
          ))}
          {snapshot.pendingAction ? (
            <article className={styles.pendingAction}>
              <strong>待批准操作</strong>
              <span>{snapshot.pendingAction.label}</span>
              <div className={styles.buttonRow}>
                <button className={styles.plainButton} type="button">
                  <Icon aria-hidden icon={checkIcon} />
                  <span>批准</span>
                </button>
                <button className={styles.plainButton} type="button">
                  <Icon aria-hidden icon={xIcon} />
                  <span>拒绝</span>
                </button>
              </div>
            </article>
          ) : null}
        </section>

        <form
          aria-label="智能助手输入区"
          className={styles.composer}
          onSubmit={handleAskAgent}
        >
          <div className={styles.composerInput}>
            <textarea
              aria-label="向智能助手提问"
              className={styles.composerBox}
              placeholder={hasActiveSpace ? "询问当前文件夹" : "先添加知识库文件夹"}
              rows={3}
              value={question}
              onChange={(event) => setQuestion(event.target.value)}
            />
            <button className={styles.sendButton} disabled={!canAskAgent} type="submit">
              <Icon aria-hidden icon={sendIcon} />
              <span>发送</span>
            </button>
          </div>
        </form>
      </aside>
    </div>
  );
}
