use serde_json::Value;

#[derive(Default, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    #[serde(skip_serializing_if = "Option::is_none")]
    input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_tokens: Option<u64>,
}

impl Usage {
    // SSE 用量通常是累计值，后续事件覆盖已有值，不能逐块累加。
    pub fn observe(&mut self, protocol: &str, raw: &str) {
        let Ok(value) = serde_json::from_str::<Value>(raw) else {
            return;
        };
        let usage = match protocol {
            "openai-responses" => &value["response"]["usage"],
            "anthropic-messages" if value["type"] == "message_start" => &value["message"]["usage"],
            "anthropic-messages" | "openai-completions" => &value["usage"],
            _ => return,
        };
        let input_key = if protocol == "openai-completions" {
            "prompt_tokens"
        } else {
            "input_tokens"
        };
        let output_key = if protocol == "openai-completions" {
            "completion_tokens"
        } else {
            "output_tokens"
        };
        if let Some(mut input) = usage[input_key].as_u64() {
            if protocol == "anthropic-messages" {
                // Anthropic 将缓存读取／写入与普通输入分开报告。
                for key in ["cache_read_input_tokens", "cache_creation_input_tokens"] {
                    input = input.saturating_add(usage[key].as_u64().unwrap_or(0));
                }
            }
            self.input_tokens = Some(input);
        }
        if let Some(output) = usage[output_key].as_u64() {
            self.output_tokens = Some(output);
        }
        if let Some(total) = usage["total_tokens"].as_u64() {
            self.total_tokens = Some(total);
        }
    }

    pub fn value(&self) -> Option<Value> {
        if self.input_tokens.is_none()
            && self.output_tokens.is_none()
            && self.total_tokens.is_none()
        {
            return None;
        }
        let mut value = serde_json::to_value(self).ok()?;
        if self.total_tokens.is_none() {
            if let (Some(input), Some(output)) = (self.input_tokens, self.output_tokens) {
                value["totalTokens"] = Value::from(input.saturating_add(output));
            }
        }
        Some(value)
    }
}

#[derive(Default)]
pub struct Decoder {
    buffer: Vec<u8>,
    data: Vec<String>,
}
impl Decoder {
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<String>, String> {
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() > 4_000_000 {
            return Err("流式事件超过长度限制".into());
        }
        let mut events = Vec::new();
        while let Some(pos) = self.buffer.iter().position(|b| *b == b'\n') {
            let line = self.buffer.drain(..=pos).collect::<Vec<_>>();
            let line = std::str::from_utf8(&line)
                .map_err(|_| "流式响应 UTF-8 无效")?
                .trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                if !self.data.is_empty() {
                    events.push(self.data.join("\n"));
                    self.data.clear();
                }
            } else if let Some(data) = line.strip_prefix("data:") {
                self.data
                    .push(data.strip_prefix(' ').unwrap_or(data).to_owned());
                if self.data.iter().map(String::len).sum::<usize>() > 4_000_000 {
                    return Err("流式事件超过长度限制".into());
                }
            }
        }
        Ok(events)
    }
}

pub fn event(protocol: &str, raw: &str) -> Result<(Option<String>, bool), String> {
    if raw.trim() == "[DONE]" {
        return Ok((None, true));
    }
    let value: Value = serde_json::from_str(raw).map_err(|_| "流式事件 JSON 无效")?;
    if value.get("error").is_some()
        || ["error", "response.failed", "response.incomplete"]
            .contains(&value["type"].as_str().unwrap_or(""))
    {
        return Err("模型返回错误或未完成回复".into());
    }
    let (delta, complete) = match protocol {
        "anthropic-messages" => (
            value["delta"]["text"].as_str(),
            value["type"] == "message_stop",
        ),
        "openai-responses" => (
            if value["type"] == "response.output_text.delta" {
                value["delta"].as_str()
            } else {
                None
            },
            value["type"] == "response.completed",
        ),
        "openai-completions" => (value["choices"][0]["delta"]["content"].as_str(), false),
        _ => return Err("不支持的对话协议".into()),
    };
    Ok((delta.map(str::to_owned), complete))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn usage_captures_openai_protocols_without_accumulating_snapshots() {
        let mut usage = Usage::default();
        let raw = r#"{"choices":[],"usage":{"prompt_tokens":18,"completion_tokens":7,"total_tokens":25}}"#;
        usage.observe("openai-completions", raw);
        usage.observe("openai-completions", raw);
        usage.observe("openai-completions", "[DONE]");
        assert_eq!(
            usage.value().unwrap(),
            serde_json::json!({"inputTokens":18,"outputTokens":7,"totalTokens":25})
        );
        let mut usage = Usage::default();
        usage.observe("openai-responses", r#"{"type":"response.completed","response":{"usage":{"input_tokens":10,"output_tokens":4,"total_tokens":14}}}"#);
        assert_eq!(usage.value().unwrap()["totalTokens"], 14);
    }

    #[test]
    fn anthropic_usage_merges_start_and_final_cumulative_output() {
        let mut usage = Usage::default();
        usage.observe("anthropic-messages", r#"{"type":"message_start","message":{"usage":{"input_tokens":10,"cache_read_input_tokens":20,"cache_creation_input_tokens":5,"output_tokens":1}}}"#);
        usage.observe(
            "anthropic-messages",
            r#"{"type":"message_delta","usage":{"output_tokens":9}}"#,
        );
        usage.observe(
            "anthropic-messages",
            r#"{"type":"message_delta","usage":{"output_tokens":9}}"#,
        );
        assert_eq!(
            usage.value().unwrap(),
            serde_json::json!({"inputTokens":35,"outputTokens":9,"totalTokens":44})
        );
    }

    #[test]
    fn usage_does_not_invent_missing_or_invalid_counts() {
        let mut usage = Usage::default();
        usage.observe("openai-completions", r#"{"usage":null}"#);
        usage.observe(
            "openai-completions",
            r#"{"usage":{"prompt_tokens":-1,"completion_tokens":"8"}}"#,
        );
        assert!(usage.value().is_none());
        usage.observe("openai-completions", r#"{"usage":{"completion_tokens":0}}"#);
        assert_eq!(
            usage.value().unwrap(),
            serde_json::json!({"outputTokens":0})
        );
        let mut partial = Usage::default();
        partial.observe("openai-responses", r#"{"type":"response.incomplete","response":{"usage":{"input_tokens":10,"output_tokens":2,"total_tokens":12}}}"#);
        assert_eq!(partial.value().unwrap()["totalTokens"], 12);
    }
    #[test]
    fn fragmented_utf8_and_multiline_events() {
        let input="event: message\r\ndata: {\r\ndata: \"delta\":\"中文\",\r\ndata: \"type\":\"response.output_text.delta\"}\r\n\r\n";
        let mut decoder = Decoder::default();
        let mut events = vec![];
        for byte in input.as_bytes() {
            events.extend(decoder.push(&[*byte]).unwrap());
        }
        assert_eq!(events.len(), 1);
        assert_eq!(
            event("openai-responses", &events[0]).unwrap(),
            (Some("中文".into()), false)
        );
    }
    #[test]
    fn protocols_errors_and_termination() {
        assert_eq!(
            event(
                "openai-completions",
                r#"{"choices":[{"delta":{"content":"hello"}}]}"#
            )
            .unwrap()
            .0,
            Some("hello".into())
        );
        assert_eq!(
            event(
                "anthropic-messages",
                r#"{"type":"content_block_delta","delta":{"text":"hello"}}"#
            )
            .unwrap()
            .0,
            Some("hello".into())
        );
        assert!(event("openai-completions", "[DONE]").unwrap().1);
        assert!(
            event("anthropic-messages", r#"{"type":"message_stop"}"#)
                .unwrap()
                .1
        );
        assert!(
            event("openai-responses", r#"{"type":"response.completed"}"#)
                .unwrap()
                .1
        );
        for input in [
            r#"{"error":{"message":"private"}}"#,
            r#"{"type":"response.failed"}"#,
            r#"{"type":"response.incomplete"}"#,
            "invalid",
        ] {
            assert!(event("openai-responses", input).is_err());
        }
    }
}
