use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::models::KnowledgeBlockSearchHit;

const MAX_CONTEXT_CHARS: usize = 4_000;

pub async fn answer_question(question: &str, hits: &[KnowledgeBlockSearchHit]) -> String {
    let local_answer = build_local_answer(question, hits);
    let Some(config) = crate::runtime::deepseek_config() else {
        return local_answer;
    };

    match call_deepseek(&config, question, hits).await {
        Some(answer) => answer,
        None => format!("{local_answer}\n\nDeepSeek 暂时不可用，已使用本地索引生成回答。"),
    }
}

fn build_local_answer(question: &str, hits: &[KnowledgeBlockSearchHit]) -> String {
    let normalized_question = question.trim();

    if hits.is_empty() {
        return format!(
            "本地索引里暂时没有找到与“{}”直接相关的内容。请先扫描并建索引/摘要，或换一个更具体的问题。",
            normalized_question
        );
    }

    let mut answer = format!(
        "根据本地索引，关于“{}”可以先看这些内容：",
        normalized_question
    );

    for (index, hit) in hits.iter().enumerate() {
        answer.push_str(&format!("\n{}. {}：{}", index + 1, hit.title, hit.excerpt));
    }

    answer.push_str("\n\n来源：");
    for hit in hits {
        answer.push_str(&format!("\n- {}", hit.source_file_name));
    }

    answer
}

async fn call_deepseek(
    config: &crate::runtime::DeepSeekConfig,
    question: &str,
    hits: &[KnowledgeBlockSearchHit],
) -> Option<String> {
    if hits.is_empty() {
        return None;
    }

    let context = build_context(hits);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .ok()?;
    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
    let payload = ChatCompletionRequest {
        model: config.model.clone(),
        messages: vec![
            ChatMessagePayload {
                role: "system".to_string(),
                content: "你是本地优先桌面知识库助手。只能基于给定本地索引内容回答；回答要简洁，不要编造来源，并在末尾列出用到的来源编号和来源文件。".to_string(),
            },
            ChatMessagePayload {
                role: "user".to_string(),
                content: format!("问题：{question}\n\n本地索引内容：\n{context}"),
            },
        ],
        stream: false,
        temperature: 0.2,
    };
    let response = client
        .post(url)
        .bearer_auth(config.api_key.trim())
        .json(&payload)
        .send()
        .await
        .ok()?;

    if !response.status().is_success() {
        return None;
    }

    let body = response.json::<ChatCompletionResponse>().await.ok()?;
    body.choices
        .into_iter()
        .find_map(|choice| choice.message.content)
        .map(|content| content.trim().to_string())
        .filter(|content| !content.is_empty())
}

fn build_context(hits: &[KnowledgeBlockSearchHit]) -> String {
    let mut context = String::new();

    for (index, hit) in hits.iter().enumerate() {
        if context.chars().count() >= MAX_CONTEXT_CHARS {
            break;
        }

        context.push_str(&format!(
            "[来源 {}]\n来源文件：{}\n来源定位：{}\n标题：{}\n内容：{}\n\n",
            index + 1,
            hit.source_file_name,
            hit.source_locator,
            hit.title,
            hit.excerpt
        ));
    }

    context.chars().take(MAX_CONTEXT_CHARS).collect()
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessagePayload>,
    stream: bool,
    temperature: f32,
}

#[derive(Debug, Serialize)]
struct ChatMessagePayload {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChatChoiceMessage {
    content: Option<String>,
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
            }],
        );

        assert!(answer.contains("空值缓存"));
        assert!(answer.contains("来源"));
        assert!(answer.contains("Redis面试.md"));
    }
}
