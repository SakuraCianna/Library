import { useEffect, useState } from "react";
import { Icon } from "@iconify/react";
import plusIcon from "@iconify-icons/lucide/plus";
import messageSquareIcon from "@iconify-icons/lucide/message-square";
import {
  createConversation,
  listConversations,
  switchConversation,
} from "../../lib/tauriClient";
import type { Conversation } from "../../types/workbench";
import styles from "./ConversationList.module.css";

interface ConversationListProps {
  activeSpaceId: string | null;
  activeConversationId: string | null;
  onConversationSwitched: (id: string) => void;
}

export function ConversationList({
  activeSpaceId,
  activeConversationId,
  onConversationSwitched,
}: ConversationListProps) {
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (activeSpaceId) {
      loadConversations(activeSpaceId);
    } else {
      setConversations([]);
    }
  }, [activeSpaceId]);

  async function loadConversations(spaceId: string) {
    try {
      setLoading(true);
      const list = await listConversations(spaceId);
      setConversations(list);
    } catch (error) {
      console.error("Failed to load conversations:", error);
    } finally {
      setLoading(false);
    }
  }

  async function handleNewChat() {
    if (!activeSpaceId) return;
    try {
      const title = "新的对话 " + new Date().toLocaleTimeString();
      const newConv = await createConversation(activeSpaceId, title);
      await loadConversations(activeSpaceId);
      handleSwitchChat(newConv.id);
    } catch (error) {
      console.error("Failed to create conversation:", error);
    }
  }

  async function handleSwitchChat(id: string) {
    try {
      await switchConversation(id);
      onConversationSwitched(id);
    } catch (error) {
      console.error("Failed to switch conversation:", error);
    }
  }

  if (!activeSpaceId) return null;

  return (
    <section className={styles.conversationSection} aria-label="对话列表">
      <div className={styles.sectionHeader}>
        <div className={styles.sectionLabel}>历史会话</div>
        <button
          className={styles.newChatButton}
          onClick={handleNewChat}
          title="新建对话"
          type="button"
        >
          <Icon icon={plusIcon} />
        </button>
      </div>
      <div className={styles.conversationList}>
        {loading && conversations.length === 0 ? (
          <div className={styles.emptyState}>加载中...</div>
        ) : conversations.length > 0 ? (
          conversations.map((conv) => (
            <button
              key={conv.id}
              className={`${styles.conversationItem} ${
                conv.id === activeConversationId ? styles.active : ""
              }`}
              onClick={() => handleSwitchChat(conv.id)}
              type="button"
            >
              <Icon icon={messageSquareIcon} className={styles.icon} />
              <span className={styles.title}>{conv.title}</span>
            </button>
          ))
        ) : (
          <div className={styles.emptyState}>暂无历史会话</div>
        )}
      </div>
    </section>
  );
}
