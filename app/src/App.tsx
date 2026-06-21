import { mockWorkbench } from "./data/mockWorkbench";
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
  const snapshot = mockWorkbench;
  const activeSpace =
    snapshot.spaces.find((space) => space.id === snapshot.activeSpaceId) ??
    snapshot.spaces[0];

  return (
    <div className={styles.shell}>
      <aside className={styles.sidebar} aria-label="知识库导航">
        <div className={styles.sidebarHeader}>
          <h1 className={styles.title}>知识库</h1>
          <button className={styles.ghostButton} type="button">
            新建
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
            {snapshot.spaces.map((space) => (
              <button
                className={`${styles.spaceItem} ${
                  space.id === activeSpace.id ? styles.spaceItemActive : ""
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
            ))}
          </nav>
        </section>

        <section className={styles.defaultPermission} aria-label="默认权限">
          <span>默认权限</span>
          <strong>{permissionLabel[activeSpace.defaultPermission]}</strong>
        </section>
      </aside>

      <main className={styles.main}>
        <header className={styles.folderHeader}>
          <div className={styles.folderTitleRow}>
            <h2 className={styles.folderName}>{activeSpace.name}</h2>
            <span className={styles.folderPath}>{activeSpace.path}</span>
          </div>
        </header>

        <nav className={styles.tabs} aria-label="内容标签">
          {tabs.map((tab, index) => (
            <button
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
              <span>已变更 {activeSpace.changedFileCount} 个文件</span>
              <span>OCR 队列 {activeSpace.ocrQueueCount} 个</span>
            </div>

            <article className={`${styles.panel} ${styles.panelPadded}`}>
              <div className={styles.panelKicker}>文件夹总览 README.md</div>
              <h3 className={styles.panelTitle}>面试知识库总览</h3>
              <p className={styles.panelText}>
                这里汇总当前文件夹的重点文档、最近变更和可问答内容。后续解析完成后，README
                会用于承接人工整理和自动生成的知识结构。
              </p>
            </article>

            <section className={styles.panel} aria-label="文件列表">
              <div className={styles.fileHeader}>文件列表</div>
              {snapshot.files.map((file) => (
                <div className={styles.fileRow} key={file.id}>
                  <div>
                    <strong>{file.name}</strong>
                    <span>{file.extension}</span>
                  </div>
                  <span className={fileStatusClass(file)}>{file.statusLabel}</span>
                </div>
              ))}
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
                  查看来源
                </button>
                <button className={styles.plainButton} type="button">
                  加入复习
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
          </div>
        </section>
      </main>

      <aside className={styles.agent} aria-label="智能助手">
        <header className={styles.agentTop}>
          <div className={styles.agentHeader}>
            <h2 className={styles.agentTitle}>智能助手</h2>
            <span>会话权限：{permissionLabel[snapshot.sessionPermission]}</span>
          </div>
          <div className={styles.scopeLabel}>范围切换</div>
          <div className={styles.scopeGroup} aria-label="范围切换">
            {Object.entries(scopeLabel).map(([scope, label]) => (
              <button
                className={
                  snapshot.activeScope === scope
                    ? styles.scopeActive
                    : styles.scope
                }
                key={scope}
                type="button"
              >
                {label}
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
                  批准
                </button>
                <button className={styles.plainButton} type="button">
                  拒绝
                </button>
              </div>
            </article>
          ) : null}
        </section>

        <form className={styles.composer}>
          <textarea
            aria-label="向智能助手提问"
            className={styles.composerBox}
            placeholder="询问当前文件夹"
            rows={3}
          />
          <button className={styles.sendButton} type="button">
            发送
          </button>
        </form>
      </aside>
    </div>
  );
}
