use serde_json::{json, Value};
use std::io::{BufRead, Write};
use studio_context::{render, Capsule};

pub const PROTOCOL_VERSION: &str = "2024-11-05";
pub const SERVER_NAME: &str = "studio";

pub const TOOL_CAPSULE_SUBMIT: &str = "capsule_submit";
pub const TOOL_DECISION_SEARCH: &str = "decision_search";
pub const TOOL_SYMBOL_LOOKUP: &str = "symbol_lookup";
pub const TOOL_ESCALATE: &str = "escalate";
pub const TOOL_REQUEST_MEETING: &str = "request_meeting";

pub const ALL_TOOLS: [&str; 5] = [
    TOOL_CAPSULE_SUBMIT,
    TOOL_DECISION_SEARCH,
    TOOL_SYMBOL_LOOKUP,
    TOOL_ESCALATE,
    TOOL_REQUEST_MEETING,
];

pub fn qualified(tool: &str) -> String {
    format!("mcp__{SERVER_NAME}__{tool}")
}

const NEIGHBOURS_SHOWN: usize = 8;

fn render_hit(hit: &SymbolHit) -> String {
    let signature = hit.signature.clone().unwrap_or_default();
    let mut line = match (hit.line_start, hit.line_end) {
        (Some(a), Some(b)) => format!("{} {}:{}-{} {}", hit.fqname, hit.path, a, b, signature),
        _ => format!("{} {} {}", hit.fqname, hit.path, signature),
    };

    if let Some(doc) = hit.doc.as_deref().filter(|d| !d.is_empty()) {
        line.push_str(&format!("\n  doc: {doc}"));
    }
    if !hit.calls.is_empty() {
        line.push_str(&format!("\n  calls: {}", neighbours(&hit.calls)));
    }
    if !hit.called_by.is_empty() {
        line.push_str(&format!("\n  called by: {}", neighbours(&hit.called_by)));
    }

    line
}

fn neighbours(names: &[String]) -> String {
    let shown = names
        .iter()
        .take(NEIGHBOURS_SHOWN)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    match names.len().checked_sub(NEIGHBOURS_SHOWN) {
        Some(rest) if rest > 0 => format!("{shown} (+{rest} more)"),
        _ => shown,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SymbolHit {
    pub fqname: String,
    pub path: String,
    pub signature: Option<String>,
    pub line_start: Option<u32>,
    pub line_end: Option<u32>,
    pub doc: Option<String>,
    pub calls: Vec<String>,
    pub called_by: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecisionHit {
    pub id: String,
    pub title: String,
    pub claim: String,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ToolError {
    #[error("{0}")]
    Rejected(String),
}

pub trait StudioTools {
    fn capsule_submit(&mut self, capsule: Capsule) -> Result<String, ToolError>;
    fn decision_search(&mut self, query: &str, limit: usize) -> Result<Vec<DecisionHit>, ToolError>;
    fn symbol_lookup(&mut self, name: &str) -> Result<Vec<SymbolHit>, ToolError>;
    fn escalate(&mut self, reason: &str, capsule: Option<Capsule>) -> Result<String, ToolError>;
    fn request_meeting(&mut self, kind: &str, participants: Vec<String>) -> Result<String, ToolError>;
}

fn tool_definitions() -> Value {
    json!([
        {
            "name": TOOL_CAPSULE_SUBMIT,
            "description": "Submit the capsule for your task. This is the only way to return work. Emit exactly one.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "kind": {"type": "string", "enum": ["task_return","consult_answer","decision","escalation","status"]},
                    "from": {"type": "string"},
                    "task": {"type": "string"},
                    "summary": {"type": "string", "description": "The headline. Always kept, never truncated. Keep it under 512 tokens."},
                    "outcome": {"type": "string", "enum": ["done","blocked","needs_verify","rejected"]},
                    "artifacts": {"type": "array", "items": {"type": "object", "properties": {
                        "path": {"type": "string"},
                        "symbol": {"type": "string"},
                        "change": {"type": "string", "enum": ["added","modified","removed"]}
                    }, "required": ["path","change"]}},
                    "decisions": {"type": "array", "items": {"type": "object", "properties": {
                        "claim": {"type": "string"}, "rationale": {"type": "string"}
                    }, "required": ["claim"]}},
                    "open_questions": {"type": "array", "items": {"type": "string"}},
                    "do_not_revisit": {"type": "array", "items": {"type": "string"},
                        "description": "Dead ends. Carried forward so no worker re-derives this failure."},
                    "handoff": {"type": "string"}
                },
                "required": ["kind","from","task","summary","outcome"]
            }
        },
        {
            "name": TOOL_DECISION_SEARCH,
            "description": "Search recorded studio decisions (ADRs) that may bind your work.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "limit": {"type": "integer", "default": 5}
                },
                "required": ["query"]
            }
        },
        {
            "name": TOOL_SYMBOL_LOOKUP,
            "description": "Pull a symbol's signature, location and one-hop neighbours from the index. Use this instead of reading whole files. Neighbours are matched syntactically by name, so treat them as a strong hint rather than a resolved call graph.",
            "inputSchema": {
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "required": ["name"]
            }
        },
        {
            "name": TOOL_ESCALATE,
            "description": "Escalate to your parent role when blocked or out of scope.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "reason": {"type": "string"},
                    "capsule": {"type": "object"}
                },
                "required": ["reason"]
            }
        },
        {
            "name": TOOL_REQUEST_MEETING,
            "description": "Convene a meeting with named participants.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "kind": {"type": "string", "enum": ["delegation","consult","arbitration"]},
                    "participants": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["kind","participants"]
            }
        }
    ])
}

fn ok_result(id: Value, text: String) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {"content": [{"type": "text", "text": text}]}
    })
}

fn tool_error(id: Value, text: String) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {"content": [{"type": "text", "text": text}], "isError": true}
    })
}

fn rpc_error(id: Value, code: i64, message: String) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn parse_capsule(v: &Value) -> Result<Capsule, String> {
    let mut obj = v.clone();
    if obj.get("v").is_none() {
        if let Some(m) = obj.as_object_mut() {
            m.insert("v".into(), json!(1));
        }
    }
    serde_json::from_value::<Capsule>(obj).map_err(|e| format!("capsule does not match the schema: {e}"))
}

pub fn handle_request<T: StudioTools>(tools: &mut T, line: &str) -> Option<Value> {
    let req: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return Some(rpc_error(Value::Null, -32700, format!("parse error: {e}"))),
    };

    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");

    match method {
        "initialize" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION")}
            }
        })),

        "notifications/initialized" => None,

        "tools/list" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {"tools": tool_definitions()}
        })),

        "tools/call" => {
            let params = req.get("params").cloned().unwrap_or(json!({}));
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));

            match name {
                TOOL_CAPSULE_SUBMIT => match parse_capsule(&args) {
                    Err(e) => Some(tool_error(id, e)),
                    Ok(capsule) => match render(&capsule) {
                        Err(e) => Some(tool_error(id, format!("{e}"))),
                        Ok(_) => match tools.capsule_submit(capsule) {
                            Ok(cid) => Some(ok_result(id, format!("capsule accepted: {cid}"))),
                            Err(ToolError::Rejected(e)) => Some(tool_error(id, e)),
                        },
                    },
                },

                TOOL_DECISION_SEARCH => {
                    let query = args.get("query").and_then(Value::as_str).unwrap_or("");
                    let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(5) as usize;
                    match tools.decision_search(query, limit) {
                        Ok(hits) if hits.is_empty() => {
                            Some(ok_result(id, "no matching decisions".into()))
                        }
                        Ok(hits) => {
                            let text = hits
                                .iter()
                                .map(|h| format!("{} [{}]: {}", h.title, h.id, h.claim))
                                .collect::<Vec<_>>()
                                .join("\n");
                            Some(ok_result(id, text))
                        }
                        Err(ToolError::Rejected(e)) => Some(tool_error(id, e)),
                    }
                }

                TOOL_SYMBOL_LOOKUP => {
                    let sym = args.get("name").and_then(Value::as_str).unwrap_or("");
                    match tools.symbol_lookup(sym) {
                        Ok(hits) if hits.is_empty() => {
                            Some(ok_result(id, format!("no symbol named {sym} in the index")))
                        }
                        Ok(hits) => {
                            let text = hits
                                .iter()
                                .map(render_hit)
                                .collect::<Vec<_>>()
                                .join("\n");
                            Some(ok_result(id, text))
                        }
                        Err(ToolError::Rejected(e)) => Some(tool_error(id, e)),
                    }
                }

                TOOL_ESCALATE => {
                    let reason = args.get("reason").and_then(Value::as_str).unwrap_or("");
                    if reason.trim().is_empty() {
                        return Some(tool_error(id, "escalate requires a reason".into()));
                    }
                    let capsule = args.get("capsule").and_then(|c| parse_capsule(c).ok());
                    match tools.escalate(reason, capsule) {
                        Ok(to) => Some(ok_result(id, format!("escalated to {to}"))),
                        Err(ToolError::Rejected(e)) => Some(tool_error(id, e)),
                    }
                }

                TOOL_REQUEST_MEETING => {
                    let kind = args.get("kind").and_then(Value::as_str).unwrap_or("");
                    let participants: Vec<String> = args
                        .get("participants")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter().filter_map(|p| p.as_str().map(str::to_string)).collect()
                        })
                        .unwrap_or_default();
                    if participants.is_empty() {
                        return Some(tool_error(id, "a meeting needs at least one participant".into()));
                    }
                    match tools.request_meeting(kind, participants) {
                        Ok(mid) => Some(ok_result(id, format!("meeting {mid} convened"))),
                        Err(ToolError::Rejected(e)) => Some(tool_error(id, e)),
                    }
                }

                other => Some(tool_error(id, format!("unknown tool: {other}"))),
            }
        }

        other => {
            if id.is_null() {
                None
            } else {
                Some(rpc_error(id, -32601, format!("method not found: {other}")))
            }
        }
    }
}

pub fn serve<T: StudioTools, R: BufRead, W: Write>(
    tools: &mut T,
    reader: R,
    mut writer: W,
) -> std::io::Result<()> {
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_request(tools, &line) {
            writeln!(writer, "{}", serde_json::to_string(&response)?)?;
            writer.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_context::{CapsuleKind, CapsuleOutcome};

    #[derive(Default)]
    struct Spy {
        submitted: Vec<Capsule>,
        escalations: Vec<String>,
        meetings: Vec<(String, Vec<String>)>,
        decisions: Vec<DecisionHit>,
        symbols: Vec<SymbolHit>,
        reject_submit: Option<String>,
    }

    impl StudioTools for Spy {
        fn capsule_submit(&mut self, capsule: Capsule) -> Result<String, ToolError> {
            if let Some(e) = &self.reject_submit {
                return Err(ToolError::Rejected(e.clone()));
            }
            self.submitted.push(capsule);
            Ok(format!("cap_{}", self.submitted.len()))
        }
        fn decision_search(&mut self, _q: &str, limit: usize) -> Result<Vec<DecisionHit>, ToolError> {
            Ok(self.decisions.iter().take(limit).cloned().collect())
        }
        fn symbol_lookup(&mut self, _n: &str) -> Result<Vec<SymbolHit>, ToolError> {
            Ok(self.symbols.clone())
        }
        fn escalate(&mut self, reason: &str, _c: Option<Capsule>) -> Result<String, ToolError> {
            self.escalations.push(reason.into());
            Ok("systems_engineer".into())
        }
        fn request_meeting(&mut self, kind: &str, p: Vec<String>) -> Result<String, ToolError> {
            self.meetings.push((kind.into(), p));
            Ok("meeting_1".into())
        }
    }

    fn call(tools: &mut Spy, name: &str, args: Value) -> Value {
        let req = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": name, "arguments": args}
        });
        handle_request(tools, &req.to_string()).unwrap()
    }

    fn text_of(v: &Value) -> String {
        v["result"]["content"][0]["text"].as_str().unwrap_or("").to_string()
    }

    fn is_error(v: &Value) -> bool {
        v["result"]["isError"].as_bool().unwrap_or(false)
    }

    fn good_capsule() -> Value {
        json!({
            "kind": "task_return",
            "from": "gameplay_engineer#7",
            "task": "task_01J",
            "summary": "Added the dash ability.",
            "outcome": "done"
        })
    }

    #[test]
    fn initialize_reports_the_protocol_the_cli_expects() {
        let mut s = Spy::default();
        let r = handle_request(
            &mut s,
            &json!({"jsonrpc":"2.0","id":0,"method":"initialize"}).to_string(),
        )
        .unwrap();
        assert_eq!(r["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(r["result"]["serverInfo"]["name"], SERVER_NAME);
    }

    #[test]
    fn the_initialized_notification_gets_no_reply() {
        let mut s = Spy::default();
        let r = handle_request(
            &mut s,
            &json!({"jsonrpc":"2.0","method":"notifications/initialized"}).to_string(),
        );
        assert!(r.is_none(), "notifications must not be answered");
    }

    #[test]
    fn advertises_exactly_the_five_orchestrator_tools() {
        let mut s = Spy::default();
        let r = handle_request(
            &mut s,
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}).to_string(),
        )
        .unwrap();
        let names: Vec<&str> = r["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names.len(), 5);
        for t in ALL_TOOLS {
            assert!(names.contains(&t), "missing tool {t}");
        }
    }

    #[test]
    fn qualified_names_match_what_the_allowlist_must_contain() {
        assert_eq!(qualified(TOOL_CAPSULE_SUBMIT), "mcp__studio__capsule_submit");
    }

    #[test]
    fn a_valid_capsule_is_accepted_and_reaches_the_daemon() {
        let mut s = Spy::default();
        let r = call(&mut s, TOOL_CAPSULE_SUBMIT, good_capsule());
        assert!(!is_error(&r));
        assert!(text_of(&r).contains("cap_1"));
        assert_eq!(s.submitted.len(), 1);
        assert_eq!(s.submitted[0].kind, CapsuleKind::TaskReturn);
        assert_eq!(s.submitted[0].outcome, CapsuleOutcome::Done);
    }

    #[test]
    fn the_version_field_defaults_so_workers_need_not_send_it() {
        let mut s = Spy::default();
        call(&mut s, TOOL_CAPSULE_SUBMIT, good_capsule());
        assert_eq!(s.submitted[0].v, 1);
    }

    #[test]
    fn a_malformed_capsule_is_rejected_without_reaching_the_daemon() {
        let mut s = Spy::default();
        let r = call(&mut s, TOOL_CAPSULE_SUBMIT, json!({"kind": "task_return"}));
        assert!(is_error(&r));
        assert!(s.submitted.is_empty());
    }

    #[test]
    fn an_unknown_capsule_kind_is_rejected() {
        let mut s = Spy::default();
        let mut c = good_capsule();
        c["kind"] = json!("gossip");
        let r = call(&mut s, TOOL_CAPSULE_SUBMIT, c);
        assert!(is_error(&r));
        assert!(s.submitted.is_empty());
    }

    #[test]
    fn an_oversized_summary_is_rejected_at_the_boundary() {
        let mut s = Spy::default();
        let mut c = good_capsule();
        c["summary"] = json!("word ".repeat(3000));
        let r = call(&mut s, TOOL_CAPSULE_SUBMIT, c);
        assert!(is_error(&r), "the cap must be enforced before the daemon sees it");
        assert!(s.submitted.is_empty());
    }

    #[test]
    fn a_daemon_rejection_is_reported_to_the_worker_as_a_tool_error() {
        let mut s = Spy::default();
        s.reject_submit = Some("conflicts with adr_3".into());
        let r = call(&mut s, TOOL_CAPSULE_SUBMIT, good_capsule());
        assert!(is_error(&r));
        assert!(text_of(&r).contains("adr_3"));
    }

    #[test]
    fn decision_search_returns_hits_and_says_so_when_empty() {
        let mut s = Spy::default();
        let r = call(&mut s, TOOL_DECISION_SEARCH, json!({"query": "dash"}));
        assert!(text_of(&r).contains("no matching decisions"));

        s.decisions = vec![DecisionHit {
            id: "adr_1".into(),
            title: "Dash".into(),
            claim: "Dash is a state machine".into(),
        }];
        let r = call(&mut s, TOOL_DECISION_SEARCH, json!({"query": "dash"}));
        assert!(text_of(&r).contains("Dash is a state machine"));
        assert!(text_of(&r).contains("adr_1"));
    }

    #[test]
    fn decision_search_honours_the_limit() {
        let mut s = Spy::default();
        s.decisions = (0..10)
            .map(|i| DecisionHit {
                id: format!("adr_{i}"),
                title: "T".into(),
                claim: "c".into(),
            })
            .collect();
        let r = call(&mut s, TOOL_DECISION_SEARCH, json!({"query": "x", "limit": 3}));
        assert_eq!(text_of(&r).lines().count(), 3);
    }

    #[test]
    fn symbol_lookup_returns_a_slice_not_a_file_body() {
        let mut s = Spy::default();
        s.symbols = vec![SymbolHit {
            fqname: "Player.Dash".into(),
            path: "src/Player.cs".into(),
            signature: Some("void Dash(Vector2 dir)".into()),
            line_start: Some(40),
            line_end: Some(58),
            doc: Some("dashes in a direction".into()),
            calls: vec!["Normalize".into(), "Move".into()],
            called_by: vec!["InputRouter.OnDash".into()],
        }];
        let r = call(&mut s, TOOL_SYMBOL_LOOKUP, json!({"name": "Player.Dash"}));
        let t = text_of(&r);
        assert!(t.contains("void Dash(Vector2 dir)"));
        assert!(t.contains("40-58"));
        assert!(!t.contains("class Player"));
    }

    #[test]
    fn a_hit_carries_its_one_hop_neighbourhood_and_doc() {
        let mut s = Spy::default();
        s.symbols = vec![SymbolHit {
            fqname: "Player.Dash".into(),
            path: "src/Player.cs".into(),
            signature: Some("void Dash(Vector2 dir)".into()),
            line_start: Some(40),
            line_end: Some(58),
            doc: Some("dashes in a direction".into()),
            calls: vec!["Normalize".into(), "Move".into()],
            called_by: vec!["InputRouter.OnDash".into()],
        }];
        let t = text_of(&call(&mut s, TOOL_SYMBOL_LOOKUP, json!({"name": "Player.Dash"})));
        assert!(t.contains("dashes in a direction"));
        assert!(t.contains("calls: Normalize, Move"));
        assert!(t.contains("called by: InputRouter.OnDash"));
    }

    #[test]
    fn a_long_neighbour_list_is_capped_with_a_visible_remainder() {
        let names: Vec<String> = (0..12).map(|i| format!("f{i}")).collect();
        let mut s = Spy::default();
        s.symbols = vec![SymbolHit {
            fqname: "A.b".into(),
            path: "a.gd".into(),
            signature: None,
            line_start: None,
            line_end: None,
            doc: None,
            calls: names,
            called_by: Vec::new(),
        }];
        let t = text_of(&call(&mut s, TOOL_SYMBOL_LOOKUP, json!({"name": "A.b"})));
        assert!(t.contains("(+4 more)"));
        assert!(!t.contains("f8"));
    }

    #[test]
    fn escalate_requires_a_reason() {
        let mut s = Spy::default();
        let r = call(&mut s, TOOL_ESCALATE, json!({"reason": "  "}));
        assert!(is_error(&r));
        assert!(s.escalations.is_empty());
    }

    #[test]
    fn escalate_routes_to_the_parent_role() {
        let mut s = Spy::default();
        let r = call(&mut s, TOOL_ESCALATE, json!({"reason": "needs a build config change"}));
        assert!(!is_error(&r));
        assert!(text_of(&r).contains("systems_engineer"));
        assert_eq!(s.escalations.len(), 1);
    }

    #[test]
    fn a_meeting_needs_participants() {
        let mut s = Spy::default();
        let r = call(&mut s, TOOL_REQUEST_MEETING, json!({"kind": "arbitration", "participants": []}));
        assert!(is_error(&r));
        assert!(s.meetings.is_empty());
    }

    #[test]
    fn an_unknown_tool_is_an_error_not_a_panic() {
        let mut s = Spy::default();
        let r = call(&mut s, "drop_database", json!({}));
        assert!(is_error(&r));
        assert!(text_of(&r).contains("unknown tool"));
    }

    #[test]
    fn malformed_json_gets_a_parse_error_rather_than_killing_the_server() {
        let mut s = Spy::default();
        let r = handle_request(&mut s, "{not json").unwrap();
        assert_eq!(r["error"]["code"], -32700);
    }

    #[test]
    fn an_unknown_method_with_an_id_gets_method_not_found() {
        let mut s = Spy::default();
        let r = handle_request(
            &mut s,
            &json!({"jsonrpc":"2.0","id":7,"method":"resources/list"}).to_string(),
        )
        .unwrap();
        assert_eq!(r["error"]["code"], -32601);
    }

    #[test]
    fn the_serve_loop_answers_a_full_handshake_over_a_pipe() {
        let input = format!(
            "{}\n{}\n{}\n",
            json!({"jsonrpc":"2.0","id":0,"method":"initialize"}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
                "name": TOOL_CAPSULE_SUBMIT, "arguments": good_capsule()}}),
        );
        let mut out: Vec<u8> = Vec::new();
        let mut spy = Spy::default();
        serve(&mut spy, std::io::Cursor::new(input), &mut out).unwrap();

        let text = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = text.trim().lines().collect();
        assert_eq!(lines.len(), 2, "the notification must not produce a line");
        assert!(lines[0].contains(PROTOCOL_VERSION));
        assert!(lines[1].contains("cap_1"));
        assert_eq!(spy.submitted.len(), 1);
    }
}
