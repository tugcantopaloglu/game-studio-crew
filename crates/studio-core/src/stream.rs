use serde_json::Value;
use studio_events::Usage;

#[derive(Debug, Clone, PartialEq)]
pub enum CliEvent {
    Init {
        session_id: String,
        model: Option<String>,
        mcp_servers: Vec<McpServer>,
    },
    UsageDelta {
        usage: Usage,
    },
    ToolCall {
        tool: String,
        args_digest: String,
    },
    ToolResult {
        tool: String,
        ok: bool,
        bytes: usize,
    },
    Text {
        text: String,
    },
    RateLimit {
        raw: Value,
    },
    Result {
        session_id: Option<String>,
        is_error: bool,
        usage: Usage,
        cost_usd: f64,
        message: Option<String>,
    },
    Other {
        kind: String,
    },
    Unparsed {
        raw: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct McpServer {
    pub name: String,
    pub status: String,
}

impl McpServer {
    pub fn is_connected(&self) -> bool {
        self.status == "connected"
    }
}

fn usage_from(v: &Value) -> Usage {
    Usage {
        input: v.get("input_tokens").and_then(Value::as_u64).unwrap_or(0),
        output: v.get("output_tokens").and_then(Value::as_u64).unwrap_or(0),
        cache_read: v
            .get("cache_read_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_creation: v
            .get("cache_creation_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    }
}

fn digest(v: &Value) -> String {
    let s = v.to_string();
    let hash = blake3_like(&s);
    format!("{hash:016x}")
}

fn blake3_like(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn parse_line(line: &str) -> Option<CliEvent> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    let v: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return Some(CliEvent::Unparsed { raw: line.to_string() }),
    };

    let ty = v.get("type").and_then(Value::as_str).unwrap_or("");

    match ty {
        "system" => {
            let subtype = v.get("subtype").and_then(Value::as_str).unwrap_or("");
            if subtype != "init" {
                return Some(CliEvent::Other { kind: format!("system/{subtype}") });
            }
            let servers = v
                .get("mcp_servers")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| {
                            Some(McpServer {
                                name: s.get("name")?.as_str()?.to_string(),
                                status: s
                                    .get("status")
                                    .and_then(Value::as_str)
                                    .unwrap_or("unknown")
                                    .to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(CliEvent::Init {
                session_id: v
                    .get("session_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                model: v.get("model").and_then(Value::as_str).map(str::to_string),
                mcp_servers: servers,
            })
        }

        "stream_event" => {
            let ev = v.get("event")?;
            let evty = ev.get("type").and_then(Value::as_str).unwrap_or("");
            match evty {
                "message_start" => ev
                    .get("message")
                    .and_then(|m| m.get("usage"))
                    .map(|u| CliEvent::UsageDelta { usage: usage_from(u) })
                    .or(Some(CliEvent::Other { kind: "message_start".into() })),

                "message_delta" => ev
                    .get("usage")
                    .map(|u| CliEvent::UsageDelta { usage: usage_from(u) })
                    .or(Some(CliEvent::Other { kind: "message_delta".into() })),

                "content_block_start" => {
                    let block = ev.get("content_block")?;
                    if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                        Some(CliEvent::ToolCall {
                            tool: block
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("unknown")
                                .to_string(),
                            args_digest: digest(block.get("input").unwrap_or(&Value::Null)),
                        })
                    } else {
                        Some(CliEvent::Other { kind: "content_block_start".into() })
                    }
                }

                "content_block_delta" => {
                    let delta = ev.get("delta")?;
                    match delta.get("type").and_then(Value::as_str) {
                        Some("text_delta") => Some(CliEvent::Text {
                            text: delta
                                .get("text")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                        }),
                        _ => Some(CliEvent::Other { kind: "content_block_delta".into() }),
                    }
                }

                other => Some(CliEvent::Other { kind: other.to_string() }),
            }
        }

        "user" => {
            let content = v.get("message").and_then(|m| m.get("content"))?;
            let block = content.as_array()?.iter().find(|b| {
                b.get("type").and_then(Value::as_str) == Some("tool_result")
            })?;
            let body = block.get("content").map(|c| c.to_string()).unwrap_or_default();
            Some(CliEvent::ToolResult {
                tool: block
                    .get("tool_use_id")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_string(),
                ok: block.get("is_error").and_then(Value::as_bool) != Some(true),
                bytes: body.len(),
            })
        }

        "rate_limit_event" => Some(CliEvent::RateLimit { raw: v.clone() }),

        "result" => Some(CliEvent::Result {
            session_id: v.get("session_id").and_then(Value::as_str).map(str::to_string),
            is_error: v.get("is_error").and_then(Value::as_bool).unwrap_or(false),
            usage: v.get("usage").map(usage_from).unwrap_or_default(),
            cost_usd: v
                .get("total_cost_usd")
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
            message: v.get("result").and_then(Value::as_str).map(str::to_string),
        }),

        other => Some(CliEvent::Other { kind: other.to_string() }),
    }
}

#[derive(Debug, Default)]
pub struct StreamState {
    pub session_id: Option<String>,
    pub latest_usage: Option<Usage>,
    pub final_usage: Option<Usage>,
    pub cost_usd: f64,
    pub saw_result: bool,
    pub is_error: bool,
    pub mcp_servers: Vec<McpServer>,
    pub text: String,
}

impl StreamState {
    pub fn apply(&mut self, ev: &CliEvent) {
        match ev {
            CliEvent::Init { session_id, mcp_servers, .. } => {
                if !session_id.is_empty() {
                    self.session_id = Some(session_id.clone());
                }
                self.mcp_servers = mcp_servers.clone();
            }
            CliEvent::UsageDelta { usage } => {
                self.latest_usage = Some(*usage);
            }
            CliEvent::Text { text } => self.text.push_str(text),
            CliEvent::Result { session_id, is_error, usage, cost_usd, .. } => {
                self.saw_result = true;
                self.is_error = *is_error;
                self.final_usage = Some(*usage);
                self.cost_usd = *cost_usd;
                if let Some(id) = session_id {
                    self.session_id = Some(id.clone());
                }
            }
            _ => {}
        }
    }

    pub fn authoritative_usage(&self) -> Option<Usage> {
        self.final_usage.or(self.latest_usage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL_STREAM: &str = include_str!("../testdata/stream-real.ndjson");

    fn parse_all(s: &str) -> Vec<CliEvent> {
        s.lines().filter_map(parse_line).collect()
    }

    #[test]
    fn parses_a_real_captured_stream_without_unparsed_lines() {
        let events = parse_all(REAL_STREAM);
        assert!(!events.is_empty());
        let unparsed: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, CliEvent::Unparsed { .. }))
            .collect();
        assert!(unparsed.is_empty(), "unparsed lines: {unparsed:?}");
    }

    #[test]
    fn interim_usage_arrives_before_the_result() {
        let events = parse_all(REAL_STREAM);
        let result_at = events
            .iter()
            .position(|e| matches!(e, CliEvent::Result { .. }))
            .expect("stream must end in a result");
        let interim = events[..result_at]
            .iter()
            .filter(|e| matches!(e, CliEvent::UsageDelta { .. }))
            .count();
        assert!(
            interim > 0,
            "M1 settled R2: usage is available before the terminal result"
        );
    }

    #[test]
    fn the_final_result_supersedes_every_interim_estimate() {
        let mut st = StreamState::default();
        for ev in parse_all(REAL_STREAM) {
            st.apply(&ev);
        }
        assert!(st.saw_result);
        assert!(!st.is_error);
        let usage = st.authoritative_usage().unwrap();
        assert_eq!(usage, st.final_usage.unwrap());
        assert!(usage.total_input() > 0);
        assert!(st.cost_usd > 0.0);
    }

    #[test]
    fn captures_the_session_id_for_crash_recovery() {
        let mut st = StreamState::default();
        for ev in parse_all(REAL_STREAM) {
            st.apply(&ev);
        }
        assert!(st.session_id.is_some());
    }

    #[test]
    fn reads_cache_numbers_off_the_real_stream() {
        let mut st = StreamState::default();
        for ev in parse_all(REAL_STREAM) {
            st.apply(&ev);
        }
        let u = st.final_usage.unwrap();
        assert!(
            u.cache_creation > 0 || u.cache_read > 0,
            "a frozen prefix must either write or read cache"
        );
    }

    #[test]
    fn detects_a_connected_mcp_server() {
        let line = r#"{"type":"system","subtype":"init","session_id":"s1",
            "mcp_servers":[{"name":"studio","status":"connected"}]}"#;
        match parse_line(line).unwrap() {
            CliEvent::Init { mcp_servers, session_id, .. } => {
                assert_eq!(session_id, "s1");
                assert!(mcp_servers[0].is_connected());
            }
            other => panic!("expected Init, got {other:?}"),
        }
    }

    #[test]
    fn detects_an_mcp_server_that_never_connected() {
        let line = r#"{"type":"system","subtype":"init","session_id":"s1",
            "mcp_servers":[{"name":"studio","status":"pending"}]}"#;
        match parse_line(line).unwrap() {
            CliEvent::Init { mcp_servers, .. } => assert!(!mcp_servers[0].is_connected()),
            other => panic!("expected Init, got {other:?}"),
        }
    }

    #[test]
    fn extracts_tool_calls() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_start",
            "content_block":{"type":"tool_use","name":"mcp__studio__capsule_submit",
            "input":{"kind":"task_return"}}}}"#;
        match parse_line(line).unwrap() {
            CliEvent::ToolCall { tool, args_digest } => {
                assert_eq!(tool, "mcp__studio__capsule_submit");
                assert!(!args_digest.is_empty());
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn tool_call_digests_never_leak_the_arguments() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_start",
            "content_block":{"type":"tool_use","name":"Read",
            "input":{"file_path":"/secret/path.txt"}}}}"#;
        match parse_line(line).unwrap() {
            CliEvent::ToolCall { args_digest, .. } => {
                assert!(!args_digest.contains("secret"));
                assert!(!args_digest.contains("path"));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn a_not_logged_in_result_is_surfaced_as_an_error() {
        let line = r#"{"type":"result","subtype":"success","is_error":true,
            "result":"Not logged in · Please run /login","session_id":"s",
            "total_cost_usd":0,"usage":{"input_tokens":0,"output_tokens":0,
            "cache_read_input_tokens":0,"cache_creation_input_tokens":0}}"#;
        match parse_line(line).unwrap() {
            CliEvent::Result { is_error, message, .. } => {
                assert!(is_error);
                assert!(message.unwrap().contains("Not logged in"));
            }
            other => panic!("expected Result, got {other:?}"),
        }
    }

    #[test]
    fn malformed_lines_do_not_panic() {
        assert!(matches!(
            parse_line("this is not json").unwrap(),
            CliEvent::Unparsed { .. }
        ));
        assert!(parse_line("   ").is_none());
        assert!(parse_line("{}").is_some());
    }

    #[test]
    fn unknown_event_types_are_tolerated() {
        let ev = parse_line(r#"{"type":"some_future_event","payload":1}"#).unwrap();
        assert_eq!(ev, CliEvent::Other { kind: "some_future_event".into() });
    }
}
