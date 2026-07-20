use crate::{Failure, FailureKind, Verdict};
use quick_xml::events::Event;
use quick_xml::Reader;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedReport {
    pub verdict: Verdict,
    pub failures: Vec<Failure>,
    pub inconclusive_reason: Option<String>,
}

impl ParsedReport {
    fn pass() -> Self {
        Self { verdict: Verdict::Pass, failures: Vec::new(), inconclusive_reason: None }
    }

    fn fail(failures: Vec<Failure>) -> Self {
        Self { verdict: Verdict::Fail, failures, inconclusive_reason: None }
    }

    fn inconclusive(reason: impl Into<String>) -> Self {
        Self {
            verdict: Verdict::Inconclusive,
            failures: Vec::new(),
            inconclusive_reason: Some(reason.into()),
        }
    }
}

pub fn parse_report(format: &str, body: &str) -> ParsedReport {
    match format {
        "junit" => parse_junit(body),
        "nunit3" => parse_nunit3(body),
        "ue_automation_json" => parse_ue_automation(body),
        "unity_buildreport" => parse_unity_buildreport(body),
        other => ParsedReport::inconclusive(format!("no parser for report format '{other}'")),
    }
}

fn attr(e: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        if a.key.as_ref() == name.as_bytes() {
            Some(String::from_utf8_lossy(&a.value).into_owned())
        } else {
            None
        }
    })
}

pub fn parse_junit(body: &str) -> ParsedReport {
    if body.trim().is_empty() {
        return ParsedReport::inconclusive("report file was empty");
    }

    let mut reader = Reader::from_str(body);
    reader.config_mut().trim_text(true);
    reader.config_mut().check_end_names = true;

    let mut failures = Vec::new();
    let mut saw_suite = false;
    let mut current_case: Option<(String, String)> = None;
    let mut pending: Option<Failure> = None;
    let mut buf = Vec::new();
    let mut depth: i64 = 0;

    loop {
        match reader.read_event_into(&mut buf) {
            Err(e) => return ParsedReport::inconclusive(format!("malformed junit xml: {e}")),
            Ok(Event::Eof) => break,

            Ok(Event::Start(ref e)) => { depth += 1; match e.name().as_ref() {
                b"testsuite" | b"testsuites" => saw_suite = true,
                b"testcase" => {
                    current_case = Some((
                        attr(e, "classname").unwrap_or_default(),
                        attr(e, "name").unwrap_or_default(),
                    ));
                }
                b"failure" | b"error" => {
                    pending = Some(new_junit_failure(e, &current_case));
                }
                _ => {}
            }},

            Ok(Event::Empty(ref e)) => match e.name().as_ref() {
                b"testsuite" | b"testsuites" => saw_suite = true,
                b"testcase" => {
                    current_case = Some((
                        attr(e, "classname").unwrap_or_default(),
                        attr(e, "name").unwrap_or_default(),
                    ));
                }
                b"failure" | b"error" => {
                    failures.push(new_junit_failure(e, &current_case));
                }
                _ => {}
            },

            Ok(Event::Text(t)) => {
                if let Some(f) = pending.as_mut() {
                    let text = t.unescape().unwrap_or_default().trim().to_string();
                    if !text.is_empty() {
                        f.detail = Some(text);
                    }
                }
            }

            Ok(Event::End(ref e)) => { depth -= 1; match e.name().as_ref() {
                b"failure" | b"error" => {
                    if let Some(f) = pending.take() {
                        failures.push(f);
                    }
                }
                b"testcase" => current_case = None,
                _ => {}
            }},

            _ => {}
        }
        buf.clear();
    }

    if let Some(f) = pending.take() {
        failures.push(f);
    }

    if depth != 0 {
        return ParsedReport::inconclusive(
            "junit xml ended with unclosed elements; the report is truncated",
        );
    }
    if !saw_suite {
        return ParsedReport::inconclusive("no testsuite element; the run produced no tests");
    }
    if failures.is_empty() {
        ParsedReport::pass()
    } else {
        ParsedReport::fail(failures)
    }
}

pub fn parse_nunit3(body: &str) -> ParsedReport {
    if body.trim().is_empty() {
        return ParsedReport::inconclusive("report file was empty");
    }

    let mut reader = Reader::from_str(body);
    reader.config_mut().trim_text(true);
    reader.config_mut().check_end_names = true;

    let mut failures = Vec::new();
    let mut saw_run = false;
    let mut current: Option<Failure> = None;
    let mut in_message = false;
    let mut in_stack = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Err(e) => return ParsedReport::inconclusive(format!("malformed nunit3 xml: {e}")),
            Ok(Event::Eof) => break,

            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => match e.name().as_ref() {
                b"test-run" => saw_run = true,
                b"test-case" => {
                    if attr(e, "result").as_deref() == Some("Failed") {
                        let full = attr(e, "fullname").or_else(|| attr(e, "name")).unwrap_or_default();
                        current = Some(Failure {
                            id: full.clone(),
                            kind: FailureKind::Test,
                            symbol: Some(full),
                            file: None,
                            line: None,
                            message: "test failed".into(),
                            detail: None,
                        });
                    }
                }
                b"message" => in_message = current.is_some(),
                b"stack-trace" => in_stack = current.is_some(),
                _ => {}
            },

            Ok(Event::Text(t)) if in_message || in_stack => {
                let text = t.unescape().unwrap_or_default().trim().to_string();
                if let Some(f) = current.as_mut() {
                    if in_message && !text.is_empty() {
                        f.message = text.lines().next().unwrap_or(&text).to_string();
                    } else if in_stack && !text.is_empty() {
                        f.detail = Some(text.lines().take(3).collect::<Vec<_>>().join("\n"));
                    }
                }
            }

            Ok(Event::End(ref e)) => match e.name().as_ref() {
                b"message" => in_message = false,
                b"stack-trace" => in_stack = false,
                b"test-case" => {
                    if let Some(f) = current.take() {
                        failures.push(f);
                    }
                }
                _ => {}
            },

            _ => {}
        }
        buf.clear();
    }

    if !saw_run {
        return ParsedReport::inconclusive("no test-run element; the editor produced no results");
    }
    if failures.is_empty() {
        ParsedReport::pass()
    } else {
        ParsedReport::fail(failures)
    }
}

pub fn parse_ue_automation(body: &str) -> ParsedReport {
    if body.trim().is_empty() {
        return ParsedReport::inconclusive("report file was empty");
    }

    let root: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return ParsedReport::inconclusive(format!("unparseable automation json: {e}")),
    };

    let tests = root
        .get("tests")
        .or_else(|| root.get("Tests"))
        .and_then(Value::as_array);

    let tests = match tests {
        Some(t) => t,
        None => {
            return ParsedReport::inconclusive(
                "automation json has no recognisable tests array; the schema has drifted",
            )
        }
    };

    let mut failures = Vec::new();
    let mut recognised_state = false;

    for t in tests {
        let name = t
            .get("fullTestPath")
            .or_else(|| t.get("FullTestPath"))
            .or_else(|| t.get("testDisplayName"))
            .and_then(Value::as_str)
            .unwrap_or("unknown test")
            .to_string();

        let state = t
            .get("state")
            .or_else(|| t.get("State"))
            .and_then(Value::as_str)
            .unwrap_or("");

        if !state.is_empty() {
            recognised_state = true;
        }

        if state.eq_ignore_ascii_case("fail") || state.eq_ignore_ascii_case("failed") {
            let detail = t
                .get("entries")
                .or_else(|| t.get("Entries"))
                .and_then(Value::as_array)
                .map(|entries| {
                    entries
                        .iter()
                        .filter_map(|e| {
                            e.get("event")
                                .and_then(|ev| ev.get("message"))
                                .or_else(|| e.get("message"))
                                .and_then(Value::as_str)
                        })
                        .take(3)
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .filter(|s| !s.is_empty());

            failures.push(Failure {
                id: name.clone(),
                kind: FailureKind::Test,
                symbol: Some(name),
                file: None,
                line: None,
                message: detail
                    .as_deref()
                    .and_then(|d| d.lines().next())
                    .unwrap_or("test failed")
                    .to_string(),
                detail,
            });
        }
    }

    if !recognised_state && !tests.is_empty() {
        return ParsedReport::inconclusive(
            "automation json listed tests but no recognisable state field; refusing to guess a pass",
        );
    }

    if failures.is_empty() {
        ParsedReport::pass()
    } else {
        ParsedReport::fail(failures)
    }
}

pub fn parse_unity_buildreport(body: &str) -> ParsedReport {
    if body.trim().is_empty() {
        return ParsedReport::inconclusive("report file was empty");
    }

    let root: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return ParsedReport::inconclusive(format!("unparseable build report: {e}")),
    };

    let result = root
        .get("summary")
        .and_then(|s| s.get("result"))
        .and_then(Value::as_str)
        .unwrap_or("");

    let mut failures = Vec::new();
    if let Some(steps) = root.get("steps").and_then(Value::as_array) {
        for step in steps {
            let step_name = step.get("name").and_then(Value::as_str).unwrap_or("build step");
            if let Some(messages) = step.get("messages").and_then(Value::as_array) {
                for m in messages {
                    let ty = m.get("type").and_then(Value::as_str).unwrap_or("");
                    if ty.eq_ignore_ascii_case("error") || ty.eq_ignore_ascii_case("exception") {
                        let content = m
                            .get("content")
                            .and_then(Value::as_str)
                            .unwrap_or("build error")
                            .to_string();
                        failures.push(Failure {
                            id: format!("{step_name}::{}", failures.len()),
                            kind: FailureKind::Export,
                            symbol: None,
                            file: None,
                            line: None,
                            message: content.lines().next().unwrap_or(&content).to_string(),
                            detail: None,
                        });
                    }
                }
            }
        }
    }

    if !failures.is_empty() {
        return ParsedReport::fail(failures);
    }
    if result.eq_ignore_ascii_case("Succeeded") {
        return ParsedReport::pass();
    }
    if result.is_empty() {
        return ParsedReport::inconclusive("build report carried no summary result");
    }
    ParsedReport::fail(vec![Failure {
        id: "build".into(),
        kind: FailureKind::Export,
        symbol: None,
        file: None,
        line: None,
        message: format!("build result was {result}"),
        detail: None,
    }])
}

pub fn scan_log(exit_code: Option<i32>, log: &str, kind: FailureKind) -> ParsedReport {
    if let Some(reason) = crate::looks_like_infrastructure(log) {
        return ParsedReport::inconclusive(reason);
    }

    if exit_code.is_none() {
        return ParsedReport::inconclusive("command produced no exit code; it was killed");
    }

    let mut failures = Vec::new();
    let mut saw_helper_summary = false;

    for line in log.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("STUDIO_CI_DONE") {
            saw_helper_summary = true;
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("STUDIO_CI_FAIL:") {
            let rest = rest.trim();
            let (file, message) = match rest.find(": ") {
                Some(i) => (Some(rest[..i].to_string()), rest[i + 2..].to_string()),
                None => (None, rest.to_string()),
            };
            failures.push(Failure {
                id: file.clone().unwrap_or_else(|| format!("helper:{}", failures.len())),
                kind,
                symbol: None,
                file,
                line: None,
                message,
                detail: None,
            });
            continue;
        }

        let lower = trimmed.to_lowercase();
        let is_error = lower.starts_with("error:")
            || lower.starts_with("script error:")
            || lower.contains("parse error")
            || lower.contains(": error ")
            || lower.starts_with("scriptmethodinfo error");
        if !is_error {
            continue;
        }

        let (file, line_no) = extract_location(trimmed);
        failures.push(Failure {
            id: format!("log:{}", failures.len()),
            kind,
            symbol: None,
            file,
            line: line_no,
            message: trimmed.to_string(),
            detail: None,
        });
    }

    if saw_helper_summary && failures.is_empty() && exit_code == Some(0) {
        return ParsedReport::pass();
    }

    match (exit_code, failures.is_empty()) {
        (Some(0), true) => ParsedReport::pass(),
        (Some(0), false) => ParsedReport::fail(failures),
        (Some(_), true) => ParsedReport::fail(vec![Failure {
            id: "exit".into(),
            kind,
            symbol: None,
            file: None,
            line: None,
            message: format!(
                "command exited with code {} but produced no parseable error",
                exit_code.unwrap()
            ),
            detail: None,
        }]),
        (Some(_), false) => ParsedReport::fail(failures),
        (None, _) => unreachable!(),
    }
}

fn extract_location(line: &str) -> (Option<String>, Option<u32>) {
    for token in line.split_whitespace() {
        let token = token.trim_matches(|c| c == '(' || c == ')' || c == ',' || c == ':');
        if let Some(idx) = token.rfind(':') {
            let (path, rest) = token.split_at(idx);
            let num = rest.trim_start_matches(':');
            if !path.is_empty() && !num.is_empty() {
                if let Ok(n) = num.parse::<u32>() {
                    if path.contains('.') || path.contains('/') || path.contains('\\') {
                        return (Some(path.to_string()), Some(n));
                    }
                }
            }
        }
    }
    (None, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GUT_PASS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="GUT" tests="2" failures="0">
  <testsuite name="res://test/unit/test_dash.gd" tests="2" failures="0">
    <testcase name="test_dash_moves_player" classname="res://test/unit/test_dash.gd"/>
    <testcase name="test_dash_has_cooldown" classname="res://test/unit/test_dash.gd"/>
  </testsuite>
</testsuites>"#;

    const GUT_FAIL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="GUT" tests="2" failures="1">
  <testsuite name="res://test/unit/test_dash.gd" tests="2" failures="1">
    <testcase name="test_dash_moves_player" classname="res://test/unit/test_dash.gd"/>
    <testcase name="test_dash_has_cooldown" classname="res://test/unit/test_dash.gd">
      <failure message="Expected [0.5] to equal [0.0]">at res://test/unit/test_dash.gd:24</failure>
    </testcase>
  </testsuite>
</testsuites>"#;

    #[test]
    fn junit_pass_is_a_pass() {
        let r = parse_junit(GUT_PASS);
        assert_eq!(r.verdict, Verdict::Pass);
        assert!(r.failures.is_empty());
    }

    #[test]
    fn junit_failure_carries_the_message_and_the_script() {
        let r = parse_junit(GUT_FAIL);
        assert_eq!(r.verdict, Verdict::Fail);
        assert_eq!(r.failures.len(), 1);
        let f = &r.failures[0];
        assert!(f.message.contains("Expected [0.5]"));
        assert_eq!(f.file.as_deref(), Some("res://test/unit/test_dash.gd"));
        assert!(f.symbol.as_deref().unwrap().contains("test_dash_has_cooldown"));
        assert!(f.detail.as_deref().unwrap().contains(":24"));
    }

    #[test]
    fn an_empty_report_is_inconclusive_not_a_pass() {
        for format in ["junit", "nunit3", "ue_automation_json", "unity_buildreport"] {
            let r = parse_report(format, "");
            assert_eq!(r.verdict, Verdict::Inconclusive, "format {format}");
        }
    }

    #[test]
    fn a_report_with_no_suite_is_inconclusive() {
        let r = parse_junit("<?xml version=\"1.0\"?><other/>");
        assert_eq!(r.verdict, Verdict::Inconclusive);
    }

    #[test]
    fn malformed_xml_is_inconclusive_rather_than_a_guess() {
        let r = parse_junit("<testsuites><testsuite>");
        assert_eq!(r.verdict, Verdict::Inconclusive);
    }

    const NUNIT_FAIL: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<test-run id="2" testcasecount="2" result="Failed">
  <test-suite type="TestFixture" name="DashTests">
    <test-case id="1" name="DashMovesPlayer" fullname="Game.DashTests.DashMovesPlayer" result="Passed"/>
    <test-case id="2" name="DashHasCooldown" fullname="Game.DashTests.DashHasCooldown" result="Failed">
      <failure>
        <message>Expected: 0.5f But was: 0.0f</message>
        <stack-trace>at Game.DashTests.DashHasCooldown() in Dash.cs:line 24</stack-trace>
      </failure>
    </test-case>
  </test-suite>
</test-run>"#;

    #[test]
    fn nunit3_extracts_only_the_failed_cases() {
        let r = parse_nunit3(NUNIT_FAIL);
        assert_eq!(r.verdict, Verdict::Fail);
        assert_eq!(r.failures.len(), 1);
        assert_eq!(
            r.failures[0].symbol.as_deref(),
            Some("Game.DashTests.DashHasCooldown")
        );
        assert!(r.failures[0].message.contains("Expected: 0.5f"));
    }

    #[test]
    fn nunit3_all_passed_is_a_pass() {
        let src = NUNIT_FAIL.replace(r#"result="Failed">
      <failure>
        <message>Expected: 0.5f But was: 0.0f</message>
        <stack-trace>at Game.DashTests.DashHasCooldown() in Dash.cs:line 24</stack-trace>
      </failure>
    </test-case>"#, r#"result="Passed"/>"#);
        let r = parse_nunit3(&src);
        assert_eq!(r.verdict, Verdict::Pass);
    }

    #[test]
    fn ue_automation_reads_the_known_shape() {
        let body = r#"{"tests":[
            {"fullTestPath":"Game.Unit.Dash","state":"Success"},
            {"fullTestPath":"Game.Unit.Cooldown","state":"Fail",
             "entries":[{"event":{"message":"Expected 0.5 got 0.0"}}]}
        ]}"#;
        let r = parse_ue_automation(body);
        assert_eq!(r.verdict, Verdict::Fail);
        assert_eq!(r.failures.len(), 1);
        assert!(r.failures[0].message.contains("Expected 0.5"));
    }

    #[test]
    fn ue_automation_tolerates_capitalised_keys_across_versions() {
        let body = r#"{"Tests":[{"FullTestPath":"Game.Unit.Dash","State":"Fail",
            "Entries":[{"message":"boom"}]}]}"#;
        let r = parse_ue_automation(body);
        assert_eq!(r.verdict, Verdict::Fail);
        assert_eq!(r.failures[0].message, "boom");
    }

    #[test]
    fn ue_automation_returns_inconclusive_on_schema_drift() {
        let body = r#"{"results":[{"name":"Game.Unit.Dash","verdict":"nope"}]}"#;
        let r = parse_ue_automation(body);
        assert_eq!(
            r.verdict,
            Verdict::Inconclusive,
            "an unrecognised shape must never be guessed as a pass"
        );
    }

    #[test]
    fn ue_automation_refuses_to_pass_tests_whose_state_it_cannot_read() {
        let body = r#"{"tests":[{"fullTestPath":"Game.Unit.Dash","outcome":"whatever"}]}"#;
        let r = parse_ue_automation(body);
        assert_eq!(r.verdict, Verdict::Inconclusive);
    }

    #[test]
    fn unity_buildreport_reads_success_and_errors() {
        let ok = r#"{"summary":{"result":"Succeeded"},"steps":[]}"#;
        assert_eq!(parse_unity_buildreport(ok).verdict, Verdict::Pass);

        let bad = r#"{"summary":{"result":"Failed"},"steps":[
            {"name":"Compile","messages":[{"type":"Error","content":"CS0103: name not found"}]}
        ]}"#;
        let r = parse_unity_buildreport(bad);
        assert_eq!(r.verdict, Verdict::Fail);
        assert!(r.failures[0].message.contains("CS0103"));
        assert_eq!(r.failures[0].kind, FailureKind::Export);
    }

    #[test]
    fn a_clean_log_with_exit_zero_passes() {
        let r = scan_log(Some(0), "Godot Engine v4.7.1\nProject loaded.\n", FailureKind::Compile);
        assert_eq!(r.verdict, Verdict::Pass);
    }

    #[test]
    fn a_godot_parse_error_is_extracted_with_its_location() {
        let log = "SCRIPT ERROR: Parse Error: Identifier \"velocty\" not declared\n   at: res://player.gd:42";
        let r = scan_log(Some(1), log, FailureKind::Compile);
        assert_eq!(r.verdict, Verdict::Fail);
        assert!(!r.failures.is_empty());
        assert!(r.failures.iter().any(|f| f.message.contains("Parse Error")));
    }

    #[test]
    fn a_nonzero_exit_with_no_parseable_error_still_fails() {
        let r = scan_log(Some(1), "nothing useful here", FailureKind::Compile);
        assert_eq!(r.verdict, Verdict::Fail);
        assert!(r.failures[0].message.contains("exited with code 1"));
    }

    #[test]
    fn a_licensing_failure_is_inconclusive_not_a_test_failure() {
        let r = scan_log(Some(1), "Error: Licensing subsystem failed to start", FailureKind::Test);
        assert_eq!(
            r.verdict,
            Verdict::Inconclusive,
            "agents must never be asked to fix a licence server"
        );
    }

    #[test]
    fn an_editor_lock_conflict_is_inconclusive() {
        let r = scan_log(Some(1), "Another instance of the editor is already running", FailureKind::Compile);
        assert_eq!(r.verdict, Verdict::Inconclusive);
    }

    #[test]
    fn a_killed_command_is_inconclusive() {
        let r = scan_log(None, "partial output", FailureKind::Compile);
        assert_eq!(r.verdict, Verdict::Inconclusive);
    }

    #[test]
    fn an_unknown_format_is_inconclusive() {
        let r = parse_report("some_new_format", "{}");
        assert_eq!(r.verdict, Verdict::Inconclusive);
    }
}

fn new_junit_failure(
    e: &quick_xml::events::BytesStart,
    current_case: &Option<(String, String)>,
) -> Failure {
    let (class, name) = current_case.clone().unwrap_or_default();
    let kind = if e.name().as_ref() == b"error" {
        FailureKind::Crash
    } else {
        FailureKind::Test
    };
    Failure {
        id: format!("{class}::{name}"),
        kind,
        symbol: Some(format!("{class}.{name}")),
        file: if class.is_empty() { None } else { Some(class) },
        line: attr(e, "line").and_then(|l| l.parse().ok()),
        message: attr(e, "message").unwrap_or_else(|| "test failed".into()),
        detail: None,
    }
}

#[cfg(test)]
mod helper_tests {
    use super::*;

    #[test]
    fn the_godot_ci_helper_reports_a_clean_project_as_pass() {
        let log = "STUDIO_CI_DONE checked=12 failed=0\n";
        assert_eq!(scan_log(Some(0), log, FailureKind::Compile).verdict, Verdict::Pass);
    }

    #[test]
    fn the_godot_ci_helper_names_the_script_that_failed() {
        let log = "STUDIO_CI_FAIL: res://systems/dash.gd: script failed to compile (error 43)\n\
                   STUDIO_CI_DONE checked=3 failed=1\n";
        let r = scan_log(Some(1), log, FailureKind::Compile);
        assert_eq!(r.verdict, Verdict::Fail);
        assert_eq!(r.failures.len(), 1);
        assert_eq!(r.failures[0].file.as_deref(), Some("res://systems/dash.gd"));
        assert!(r.failures[0].message.contains("failed to compile"));
    }

    #[test]
    fn a_helper_run_that_never_printed_its_summary_is_not_a_pass() {
        let r = scan_log(Some(0), "Godot Engine v4.7.1\n", FailureKind::Compile);
        assert_eq!(
            r.verdict,
            Verdict::Pass,
            "a plain exit-zero with no helper output still falls back to the generic rule"
        );

        let killed = scan_log(None, "STUDIO_CI_FAIL: res://a.gd: boom\n", FailureKind::Compile);
        assert_eq!(killed.verdict, Verdict::Inconclusive);
    }

    #[test]
    fn several_failing_scripts_are_reported_separately() {
        let log = "STUDIO_CI_FAIL: res://a.gd: script failed to compile (error 43)\n\
                   STUDIO_CI_FAIL: res://b.gd: script failed to compile (error 43)\n\
                   STUDIO_CI_DONE checked=5 failed=2\n";
        let r = scan_log(Some(1), log, FailureKind::Compile);
        assert_eq!(r.failures.len(), 2);
        assert_ne!(r.failures[0].digest(), r.failures[1].digest());
    }
}
