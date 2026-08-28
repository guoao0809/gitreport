//! AI 接口调用（OpenAI / Anthropic 协议，Tauri command，reqwest blocking + stream）。

use crate::git::{AIConfig, GeneratePayload};
use futures_util::StreamExt;
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

/// 从响应 JSON 提取文本：OpenAI 取 choices[0].message.content；Anthropic 取 content[0].text
fn extract_text(body: &str, is_anthropic: bool) -> Result<String, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("响应不是有效 JSON：{e}"))?;
    let text = if is_anthropic {
        v["content"][0]["text"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| format!("响应中未找到文本（content[0].text）：{}", &body.chars().take(200).collect::<String>()))?
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
            ]
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

/// 流式调用 AI：边收边通过 channel 推送增量文本，返回完整文本（供存档）。
#[tauri::command]
pub async fn generate_report_stream(
    payload: GeneratePayload,
    channel: Channel<String>,
) -> Result<String, String> {
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
        }
    }
    // 处理最后一段无换行结尾的 data
    if let Some(delta) = extract_sse_delta(buf.trim(), is_anthropic) {
        full.push_str(&delta);
        let _ = channel.send(delta);
    }

    if full.is_empty() {
        return Err("AI 未返回任何内容（流式响应为空）".to_string());
    }
    Ok(full)
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
}
