use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::models::KnowledgeBlockSearchHit;

const MAX_CONTEXT_CHARS: usize = 4_000;

pub async fn chat_completion_step(
    config: &crate::runtime::DeepSeekConfig,
    messages: &[ChatMessagePayload],
    tools: Option<&[Tool]>,
    tone: Option<&str>,
) -> Option<ChatChoiceMessage> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .ok()?;
    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));

    let mut messages = messages.to_vec();

    // Ensure system prompt is present
    if !messages.iter().any(|m| m.role == "system") {
        let tone_instruction = tone
            .map(|t| format!(" 请保持{}的语气。", t))
            .unwrap_or_default();
        messages.insert(0, ChatMessagePayload {
            role: "system".to_string(),
            content: Some(format!("你是本地优先桌面知识库助手。只能基于给定本地索引内容回答；回答要简洁，不要编造来源，并在末尾列出用到的来源编号和来源文件。如果需要搜索信息请使用提供的工具。{}", tone_instruction)),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        });
    }

    let payload = ChatCompletionRequest {
        model: config.model.clone(),
        messages,
        stream: false,
        temperature: 0.2,
        tools: tools.map(|t| t.to_vec()),
    };

    let mut attempts = 0;
    let max_attempts = 3;
    let mut base_delay = 1;

    while attempts < max_attempts {
        if let Ok(response) = client
            .post(&url)
            .bearer_auth(config.api_key.trim())
            .json(&payload)
            .send()
            .await
        {
            if response.status().is_success() {
                if let Ok(body) = response.json::<ChatCompletionResponse>().await {
                    return body.choices.into_iter().next().map(|c| c.message);
                }
            }
        }

        attempts += 1;
        if attempts < max_attempts {
            tokio::time::sleep(Duration::from_secs(base_delay)).await;
            base_delay *= 2;
        }
    }

    None
}

pub fn build_context(hits: &[KnowledgeBlockSearchHit]) -> String {
    let mut context = String::new();

    for (index, hit) in hits.iter().enumerate() {
        if context.chars().count() >= MAX_CONTEXT_CHARS {
            break;
        }

        context.push_str(&format!(
            "[来源 {}]\n来源类型：{}\n来源文件：{}\n标题：{}\n内容：{}\n\n",
            index + 1,
            source_kind_label(&hit.source_kind),
            hit.source_file_name,
            hit.title,
            hit.excerpt
        ));
    }

    context.chars().take(MAX_CONTEXT_CHARS).collect()
}

fn source_kind_label(source_kind: &str) -> &'static str {
    match source_kind {
        "ocr" => "本地 OCR",
        "table" => "表格洞察",
        "markdown_note" => "Markdown 笔记",
        _ => "原始文件",
    }
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessagePayload>,
    pub stream: bool,
    pub temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: ToolFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessagePayload {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Deserialize)]
pub struct ChatCompletionResponse {
    pub choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
pub struct ChatChoice {
    pub message: ChatChoiceMessage,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatChoiceMessage {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[cfg(test)]
mod tests {
    use super::build_local_answer;
    use crate::models::KnowledgeBlockSearchHit;

    #[test]
    fn local_answer_includes_source_names() {
        let answer = build_local_answer(
            "缓存穿透",
            &[KnowledgeBlockSearchHit {
                id: "block-1".to_string(),
                title: "Redis面试.md".to_string(),
                excerpt: "缓存穿透需要空值缓存和布隆过滤器。".to_string(),
                source_file_name: "Redis面试.md".to_string(),
                source_locator: "Redis面试.md".to_string(),
                source_kind: "original_file".to_string(),
            }],
        );

        assert!(answer.contains("空值缓存"));
        assert!(answer.contains("来源"));
        assert!(answer.contains("Redis面试.md"));
    }

    #[test]
    fn local_answer_labels_table_sources() {
        let answer = build_local_answer(
            "6月营收",
            &[KnowledgeBlockSearchHit {
                id: "table-1".to_string(),
                title: "经营报表.xlsx · 工作表 1".to_string(),
                excerpt: "表头：月份、营收、成本 样例 1：2026-06 | 120 | 70".to_string(),
                source_file_name: "经营报表.xlsx".to_string(),
                source_locator: "经营报表.xlsx#sheet-001".to_string(),
                source_kind: "table".to_string(),
            }],
        );

        assert!(answer.contains("[表格洞察]"));
        assert!(answer.contains("2026-06 | 120 | 70"));
        assert!(answer.contains("经营报表.xlsx（表格洞察）"));
    }

    #[test]
    fn local_answer_labels_ocr_sources() {
        let answer = build_local_answer(
            "扫描版发票",
            &[KnowledgeBlockSearchHit {
                id: "ocr-1".to_string(),
                title: "scan.pdf · OCR 片段 1/1".to_string(),
                excerpt: "扫描版发票金额为 120 元。".to_string(),
                source_file_name: "scan.pdf".to_string(),
                source_locator: "scan.pdf#ocr-block-001".to_string(),
                source_kind: "ocr".to_string(),
            }],
        );

        assert!(answer.contains("[本地 OCR]"));
        assert!(answer.contains("scan.pdf（本地 OCR）"));
    }

    #[test]
    fn local_answer_without_hits_is_explicitly_evidence_bounded() {
        let answer = build_local_answer("不存在的问题", &[]);

        assert!(answer.contains("没有足够本地证据"));
        assert!(answer.contains("不会编造"));
        assert!(answer.contains("请先扫描并建索引/摘要"));
    }
}
