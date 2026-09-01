//! AI 接口调用（OpenAI / Anthropic 协议，Tauri command，reqwest blocking + stream）。

use crate::git::{AIConfig, GeneratePayload};
use futures_util::StreamExt;
use serde::Serialize;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tauri::ipc::Channel;

const TIMEOUT: Duration = Duration::from_secs(120);

/// Base URL 智能拼接：去掉尾部 '/'，按协议补 /v1。
/// OpenAI：不以 /v1 或 /chat/completions 结尾则补 /v1（请求再拼 /chat/completions）。
/// Anthropic：不以 /v1 结尾则补 /v1（请求再拼 /v1/messages）。
fn normalize_base(config: &AIConfig) -> String {
    let base = config.base_url.trim_end_matches('/');
    if config.protocol == "anthropic" {
        if base.ends_with("/v1") {
            base.to_string()
        } else {
            format!("{base}/v1")
        }
    } else {
        if base.ends_with("/v1") || base.ends_with("/chat/completions") {
            base.to_string()
        } else {
            format!("{base}/v1")
        }
    }
}

/// 按协议构造请求 URL
fn request_url(config: &AIConfig, is_anthropic: bool) -> String {
    let base = normalize_base(config);
    if is_anthropic {
        // base 已保证以 /v1 结尾（或本身就是完整 messages 路径的父级）
        if base.ends_with("/messages") {
            base
        } else {
            format!("{base}/messages")
        }
    } else if base.ends_with("/chat/completions") {
        base
    } else {
        format!("{base}/chat/completions")
    }
}

// ===== 模型列表端点候选构造 =====
// 参考 cc-switch：不固定拼 /v1/models，而是按 base_url 生成候选列表逐个尝试。
// 关键：很多供应商把 Anthropic 协议挂在 /anthropic、/api/anthropic 等兼容子路径上，
// 但模型列表端点仍在根上（如 DeepSeek /models），需要剥离后缀兜底。

/// 模型列表请求超时（单候选）
const MODEL_FETCH_TIMEOUT: Duration = Duration::from_secs(20);

/// 已知的「Anthropic 协议兼容子路径」后缀；按长度降序，最长前缀优先匹配。
const KNOWN_COMPAT_SUFFIXES: &[&str] = &[
    "/api/claudecode",
    "/api/anthropic",
    "/apps/anthropic",
    "/api/coding",
    "/claudecode",
    "/anthropic",
    "/step_plan",
    "/coding",
    "/claude",
];

/// base_url 是否以 OpenAI 风格版本段 /v{N} 结尾（如 /v1、/v4）
fn ends_with_version_segment(url: &str) -> bool {
    let last = url.rsplit('/').next().unwrap_or("");
    last.strip_prefix('v')
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

/// 若 base_url 以已知兼容子路径结尾，返回剥离后的剩余部分；否则 None。
fn strip_compat_suffix(base_url: &str) -> Option<&str> {
    for suffix in KNOWN_COMPAT_SUFFIXES {
        if base_url.ends_with(suffix) {
            return Some(&base_url[..base_url.len() - suffix.len()]);
        }
    }
    None
}

/// 生成模型列表端点候选 URL（去重，保持顺序）。
/// 直接用原始 base_url，不预先补 /v1（否则会丢失剥离兼容后缀的机会）。
fn build_models_url_candidates(config: &AIConfig) -> Vec<String> {
    let trimmed = config.base_url.trim().trim_end_matches('/');
    let mut candidates: Vec<String> = Vec::new();
    if trimmed.is_empty() {
        return candidates;
    }

    // 已以版本段 /v{N} 结尾 → 拼 /models；版本段非 /v1 时再兜底 /v1/models
    if ends_with_version_segment(trimmed) {
        candidates.push(format!("{trimmed}/models"));
        if !trimmed.ends_with("/v1") {
            candidates.push(format!("{trimmed}/v1/models"));
        }
    } else {
        candidates.push(format!("{trimmed}/v1/models"));
    }

    // 剥离兼容子路径（/anthropic 等），在根上追加 /v1/models、/models
    if let Some(stripped) = strip_compat_suffix(trimmed) {
        let root = stripped.trim_end_matches('/');
        if !root.is_empty() && root.contains("://") {
            candidates.push(format!("{root}/v1/models"));
            candidates.push(format!("{root}/models"));
        }
    }

    let mut unique: Vec<String> = Vec::new();
    for url in candidates {
        if !unique.iter().any(|u| u == &url) {
            unique.push(url);
        }
    }
    unique
}

/// 发送 JSON POST，返回响应文本（HTTP 错误透传 status + body 前 200 字符）
fn post_json(url: &str, config: &AIConfig, body: &Value, is_anthropic: bool) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败：{e}"))?;

    let mut req = client.post(url).json(body);
    if is_anthropic {
        req = req
            .header("x-api-key", &config.api_key)
            .header("anthropic-version", "2023-06-01");
    } else {
        req = req.header("Authorization", format!("Bearer {}", config.api_key));
    }

    let resp = req.send().map_err(|e| format!("网络请求失败：{e}"))?;
    let status = resp.status();
    let text = resp.text().map_err(|e| format!("读取响应失败：{e}"))?;
    if !status.is_success() {
        let snippet: String = text.chars().take(200).collect();
        return Err(format!("AI 接口返回错误（HTTP {status}）：{snippet}"));
    }
    Ok(text)
}

/// 从响应 JSON 提取文本：OpenAI 取 choices[0].message.content；Anthropic 取 content 数组中首个 text 块。
/// 兼容模型先返回 thinking 块（content[0] 为 {type:"thinking",...}）的情况——不能硬编码 content[0].text。
fn extract_text(body: &str, is_anthropic: bool) -> Result<String, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("响应不是有效 JSON：{e}"))?;
    let text = if is_anthropic {
        v["content"]
            .as_array()
            .and_then(|arr| arr.iter().find_map(|item| item["text"].as_str()))
            .map(|s| s.to_string())
            .ok_or_else(|| format!("响应中未找到文本（content[].text）：{}", &body.chars().take(200).collect::<String>()))?
    } else {
        v["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| format!("响应中未找到文本（choices[0].message.content）：{}", &body.chars().take(200).collect::<String>()))?
    };
    Ok(text)
}

// ===== command：测试 AI 连接 =====

#[tauri::command]
pub fn test_ai_connection(config: AIConfig) -> Result<String, String> {
    let is_anthropic = config.protocol == "anthropic";
    let url = request_url(&config, is_anthropic);
    let body = if is_anthropic {
        json!({
            "model": config.model,
            "max_tokens": 1,
            "messages": [{ "role": "user", "content": "ping" }]
        })
    } else {
        json!({
            "model": config.model,
            "messages": [{ "role": "user", "content": "ping" }],
            "max_tokens": 1
        })
    };

    let start = Instant::now();
    let text = post_json(&url, &config, &body, is_anthropic)?;
    // 只验证响应可解析（不要求文本非空，max_tokens=1 时可能截断）
    let _ = extract_text(&text, is_anthropic)?;
    let secs = start.elapsed().as_secs_f64();
    Ok(format!("{secs:.1}s"))
}

// ===== command：获取模型列表 =====

/// 拉取接口的模型列表，返回 model id 数组（用于设置页下拉填充）。
/// 候选端点逐个尝试：404/405 尝试下一个，其它错误直接返回。
#[tauri::command]
pub fn fetch_models(config: AIConfig) -> Result<Vec<String>, String> {
    let candidates = build_models_url_candidates(&config);
    if candidates.is_empty() {
        return Err("Base URL 为空".to_string());
    }
    // /models 是 OpenAI 标准端点，绝大多数网关（含 new-api、DeepSeek 等把 Anthropic
    // 挂在兼容子路径上的供应商）都用 Bearer 认证；仅 Anthropic 官方用 x-api-key。
    let use_x_api_key = config.base_url.contains("api.anthropic.com");
    let client = reqwest::blocking::Client::builder()
        .timeout(MODEL_FETCH_TIMEOUT)
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败：{e}"))?;

    let mut last_err = String::new();
    for url in &candidates {
        let mut req = client.get(url);
        if use_x_api_key {
            req = req
                .header("x-api-key", &config.api_key)
                .header("anthropic-version", "2023-06-01");
        } else {
            req = req.header("Authorization", format!("Bearer {}", config.api_key));
        }

        let resp = req.send().map_err(|e| format!("网络请求失败：{e}"))?;
        let status = resp.status();
        if status.is_success() {
            let text = resp.text().map_err(|e| format!("读取响应失败：{e}"))?;
            let v: Value =
                serde_json::from_str(&text).map_err(|e| format!("响应不是有效 JSON：{e}"))?;
            let models: Vec<String> = v["data"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|m| m["id"].as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            return Ok(models);
        }

        if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::METHOD_NOT_ALLOWED {
            last_err = format!("HTTP {status}");
            continue; // 候选端点不存在，尝试下一个
        }

        let text = resp.text().unwrap_or_default();
        let snippet: String = text.chars().take(200).collect();
        return Err(format!("模型列表接口返回错误（HTTP {status}）：{snippet}"));
    }

    Err(format!("无法获取模型列表（已尝试所有端点，最后：{last_err}）"))
}

// ===== command：生成报告 =====

#[tauri::command]
pub fn generate_report(payload: GeneratePayload) -> Result<String, String> {
    let config = &payload.config;
    let is_anthropic = config.protocol == "anthropic";
    let url = request_url(config, is_anthropic);
    let body = if is_anthropic {
        json!({
            "model": config.model,
            "max_tokens": 4096,
            "system": payload.system,
            "messages": [{ "role": "user", "content": payload.user }]
        })
    } else {
        json!({
            "model": config.model,
            "max_tokens": 4096,
            "messages": [
                { "role": "system", "content": payload.system },
                { "role": "user", "content": payload.user }
            ]
        })
    };

    let text = post_json(&url, config, &body, is_anthropic)?;
    extract_text(&text, is_anthropic)
}

// ===== command：流式生成报告 =====

/// 流式生成返回结构：正文 + 接口上报的 token 用量
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamResult {
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

impl Default for Usage {
    fn default() -> Self {
        Usage { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 }
    }
}

/// 构造流式请求 body（stream: true，双协议字段布局不同）
fn stream_body(payload: &GeneratePayload) -> Value {
    if payload.config.protocol == "anthropic" {
        json!({
            "model": payload.config.model,
            "max_tokens": 4096,
            "stream": true,
            "system": payload.system,
            "messages": [{ "role": "user", "content": payload.user }]
        })
    } else {
        json!({
            "model": payload.config.model,
            "max_tokens": 4096,
            "stream": true,
            "messages": [
                { "role": "system", "content": payload.system },
                { "role": "user", "content": payload.user }
            ],
            // OpenAI 流式要在最后一个 chunk 携带 usage，需显式开启
            "stream_options": { "include_usage": true }
        })
    }
}

/// 从一行 SSE `data: {...}` 提取增量文本。
/// OpenAI：choices[0].delta.content（可能缺失 → None，如仅含 role 的首帧）
/// Anthropic：delta.text（event 为 content_block_delta 时才返回）
fn extract_sse_delta(line: &str, is_anthropic: bool) -> Option<String> {
    let data = line.strip_prefix("data:")?;
    let data = data.trim();
    if data.is_empty() || data == "[DONE]" {
        return None;
    }
    let v: Value = serde_json::from_str(data).ok()?;
    if is_anthropic {
        v["delta"]["text"].as_str().map(|s| s.to_string())
    } else {
        v["choices"][0]["delta"]["content"]
            .as_str()
            .map(|s| s.to_string())
    }
}

/// 从一行 SSE 提取 usage。OpenAI：最后一个 chunk 的顶层 usage 字段；
/// Anthropic：message_start 带 input_tokens，message_delta 带 output_tokens。
fn extract_usage(line: &str, is_anthropic: bool, usage: &mut Usage) {
    let Some(data) = line.strip_prefix("data:") else { return; };
    let data = data.trim();
    if data.is_empty() {
        return;
    }
    let Ok(v) = serde_json::from_str::<Value>(data) else { return; };
    if is_anthropic {
        // message_start: usage.input_tokens（input 用 tokens）
        if v["message"]["usage"]["input_tokens"].as_u64().is_some() {
            usage.prompt_tokens = v["message"]["usage"]["input_tokens"].as_u64().unwrap() as usize;
        }
        // message_delta: usage.output_tokens（每次为累计值，取其最大值即可）
        if let Some(o) = v["usage"]["output_tokens"].as_u64() {
            let o = o as usize;
            if o > usage.completion_tokens {
                usage.completion_tokens = o;
            }
        }
    } else if let Some(obj) = v["usage"].as_object() {
        // OpenAI 最终 chunk：choices 为空、带 usage 对象
        usage.prompt_tokens = obj.get("prompt_tokens").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
        usage.completion_tokens = obj.get("completion_tokens").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
        usage.total_tokens = obj.get("total_tokens").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
    }
}

/// 流式调用 AI：边收边通过 channel 推送增量文本，返回完整文本与 token 用量。
#[tauri::command]
pub async fn generate_report_stream(
    payload: GeneratePayload,
    channel: Channel<String>,
) -> Result<StreamResult, String> {
    let config = &payload.config;
    let is_anthropic = config.protocol == "anthropic";
    let url = request_url(config, is_anthropic);
    let body = stream_body(&payload);

    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败：{e}"))?;

    let mut req = client.post(&url).json(&body);
    if is_anthropic {
        req = req
            .header("x-api-key", &config.api_key)
            .header("anthropic-version", "2023-06-01");
    } else {
        req = req.header("Authorization", format!("Bearer {}", config.api_key));
    }

    let resp = req.send().await.map_err(|e| format!("网络请求失败：{e}"))?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        let snippet: String = text.chars().take(200).collect();
        return Err(format!("AI 接口返回错误（HTTP {status}）：{snippet}"));
    }

    let mut full = String::new();
    let mut usage = Usage::default();
    let mut stream = resp.bytes_stream();
    let mut buf = String::new();

    // SSE 按行处理：行以 \n 分隔，data: 行承载 JSON
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("读取流失败：{e}"))?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buf.find('\n') {
            let line: String = buf.drain(..=pos).collect();
            let line = line.trim_end();
            if let Some(delta) = extract_sse_delta(line, is_anthropic) {
                full.push_str(&delta);
                let _ = channel.send(delta);
            }
            extract_usage(line, is_anthropic, &mut usage);
        }
    }
    // 处理最后一段无换行结尾的 data
    if let Some(delta) = extract_sse_delta(buf.trim(), is_anthropic) {
        full.push_str(&delta);
        let _ = channel.send(delta);
    }
    extract_usage(buf.trim(), is_anthropic, &mut usage);

    if full.is_empty() {
        return Err("AI 未返回任何内容（流式响应为空）".to_string());
    }
    // total 缺省时由 prompt + completion 推导（OpenAI 会带，Anthropic 需补）
    if usage.total_tokens == 0 {
        usage.total_tokens = usage.prompt_tokens + usage.completion_tokens;
    }
    let has_usage = usage.prompt_tokens + usage.completion_tokens > 0;
    Ok(StreamResult { content: full, usage: has_usage.then_some(usage) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(protocol: &str, base: &str) -> AIConfig {
        AIConfig {
            protocol: protocol.to_string(),
            base_url: base.to_string(),
            model: "m".to_string(),
            api_key: "k".to_string(),
        }
    }

    #[test]
    fn openai_url_join() {
        assert_eq!(request_url(&cfg("openai", "https://api.openai.com/"), false), "https://api.openai.com/v1/chat/completions");
        assert_eq!(request_url(&cfg("openai", "https://x.com/v1"), false), "https://x.com/v1/chat/completions");
        assert_eq!(request_url(&cfg("openai", "https://x.com/v1/"), false), "https://x.com/v1/chat/completions");
        assert_eq!(request_url(&cfg("openai", "https://x.com/chat/completions"), false), "https://x.com/chat/completions");
        assert_eq!(request_url(&cfg("openai", "https://x.com/v1/chat/completions"), false), "https://x.com/v1/chat/completions");
    }

    #[test]
    fn anthropic_url_join() {
        assert_eq!(request_url(&cfg("anthropic", "https://api.anthropic.com/"), true), "https://api.anthropic.com/v1/messages");
        assert_eq!(request_url(&cfg("anthropic", "https://x.com/v1"), true), "https://x.com/v1/messages");
        assert_eq!(request_url(&cfg("anthropic", "https://x.com/v1/"), true), "https://x.com/v1/messages");
    }

    #[test]
    fn extract_text_from_both_protocols() {
        let openai = r#"{"choices":[{"message":{"content":"hello"}}]}"#;
        let anthropic = r#"{"content":[{"type":"text","text":"hi"}]}"#;
        assert_eq!(extract_text(openai, false).unwrap(), "hello");
        assert_eq!(extract_text(anthropic, true).unwrap(), "hi");
        assert!(extract_text("{}", false).is_err());
    }

    #[test]
    fn extract_text_anthropic_with_thinking_first() {
        // 模型先返回 thinking 块（content[0].text 不存在，会误报未找到文本）
        let body = r#"{"content":[{"type":"thinking","thinking":"We","signature":"s"},{"type":"text","text":"hi"}]}"#;
        assert_eq!(extract_text(body, true).unwrap(), "hi");
        // 纯 thinking 无 text 块 → 仍应报错
        assert!(extract_text(r#"{"content":[{"type":"thinking","thinking":"xx"}]}"#, true).is_err());
    }

    #[test]
    fn sse_delta_openai() {
        // 含 role 的首帧无 content → None
        assert_eq!(extract_sse_delta(r#"data: {"choices":[{"delta":{"role":"assistant"}}]}"#, false), None);
        // 正常增量
        assert_eq!(extract_sse_delta(r#"data: {"choices":[{"delta":{"content":"你好"}}]}"#, false), Some("你好".into()));
        // [DONE] → None
        assert_eq!(extract_sse_delta("data: [DONE]", false), None);
        // 空行 → None
        assert_eq!(extract_sse_delta("", false), None);
    }

    #[test]
    fn sse_delta_anthropic() {
        assert_eq!(extract_sse_delta(r#"data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"你好"}}"#, true), Some("你好".into()));
        // 非 content_block_delta 帧（如 message_stop）→ None
        assert_eq!(extract_sse_delta(r#"data: {"type":"message_stop"}"#, true), None);
        assert_eq!(extract_sse_delta(r#"data: {"type":"content_block_start","content_block":{"type":"text","text":""}}"#, true), None);
    }

    #[test]
    fn models_url_candidates() {
        // 裸域名 → /v1/models
        assert_eq!(build_models_url_candidates(&cfg("openai", "https://api.openai.com/")), vec!["https://api.openai.com/v1/models"]);
        // 已带 /v1 → /v1/models
        assert_eq!(build_models_url_candidates(&cfg("openai", "https://x.com/v1")), vec!["https://x.com/v1/models"]);
        // Anthropic 官方 → /v1/models
        assert_eq!(build_models_url_candidates(&cfg("anthropic", "https://api.anthropic.com/")), vec!["https://api.anthropic.com/v1/models"]);
        // 版本段非 /v1（智谱 /v4）→ /models 在前，/v1/models 兜底
        assert_eq!(
            build_models_url_candidates(&cfg("openai", "https://open.bigmodel.cn/api/coding/paas/v4")),
            vec![
                "https://open.bigmodel.cn/api/coding/paas/v4/models",
                "https://open.bigmodel.cn/api/coding/paas/v4/v1/models",
            ]
        );
        // 兼容子路径 /anthropic → 剥离后缀在根上找 /v1/models、/models
        assert_eq!(
            build_models_url_candidates(&cfg("anthropic", "https://api.deepseek.com/anthropic")),
            vec![
                "https://api.deepseek.com/anthropic/v1/models",
                "https://api.deepseek.com/v1/models",
                "https://api.deepseek.com/models",
            ]
        );
    }

    #[test]
    fn models_url_candidates_empty() {
        assert!(build_models_url_candidates(&cfg("openai", "")).is_empty());
    }

    #[test]
    fn usage_openai_final_chunk() {
        // OpenAI 流式最后一个 chunk：choices 为空、带 usage 对象
        let mut u = Usage::default();
        extract_usage(r#"data: {"choices":[],"usage":{"prompt_tokens":120,"completion_tokens":340,"total_tokens":460}}"#, false, &mut u);
        assert_eq!(u.prompt_tokens, 120);
        assert_eq!(u.completion_tokens, 340);
        assert_eq!(u.total_tokens, 460);
    }

    #[test]
    fn usage_anthropic_start_and_delta() {
        let mut u = Usage::default();
        // message_start：input_tokens
        extract_usage(r#"data: {"type":"message_start","message":{"usage":{"input_tokens":90,"output_tokens":1}}}"#, true, &mut u);
        assert_eq!(u.prompt_tokens, 90);
        // message_delta：output_tokens（累计值，取最大值）
        extract_usage(r#"data: {"type":"message_delta","usage":{"output_tokens":210}}"#, true, &mut u);
        assert_eq!(u.completion_tokens, 210);
        // 一次更小的 delta 不覆盖
        extract_usage(r#"data: {"type":"message_delta","usage":{"output_tokens":150}}"#, true, &mut u);
        assert_eq!(u.completion_tokens, 210);
        // total 推导
        u.total_tokens = u.prompt_tokens + u.completion_tokens;
        assert_eq!(u.total_tokens, 300);
    }

    #[test]
    fn usage_ignores_empty_or_done() {
        let mut u = Usage::default();
        extract_usage("data: ", false, &mut u);
        extract_usage("data: [DONE]", false, &mut u);
        assert_eq!(u.total_tokens, 0);
    }
}
