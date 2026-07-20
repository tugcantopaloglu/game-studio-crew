pub mod driver;
pub mod parsers;
pub mod repair;

pub use driver::{EngineDriver, ProfileDriver, ProjectPaths};
pub use parsers::parse_report;
pub use repair::{RepairLoop, RepairStep, MAX_REPAIR_ROUNDS};

use serde::{Deserialize, Serialize};
use studio_engine::VerifyScope;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Pass,
    Fail,
    Inconclusive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    Compile,
    Test,
    Import,
    Export,
    Timeout,
    Crash,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Failure {
    pub id: String,
    pub kind: FailureKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl Failure {
    pub fn digest(&self) -> String {
        format!("{}:{}", self.id, self.message)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VerifyResult {
    pub verdict: Verdict,
    pub failures: Vec<Failure>,
    pub scope: VerifyScope,
    pub engine: String,
    pub duration_ms: u64,
    pub raw_report_path: Option<String>,
    pub inconclusive_reason: Option<String>,
}

impl VerifyResult {
    pub fn passed(&self) -> bool {
        self.verdict == Verdict::Pass
    }

    pub fn brief_for_worker(&self) -> String {
        if self.failures.is_empty() {
            return format!("{:?} {}: no failures", self.scope, self.engine);
        }
        let mut s = format!(
            "{} failure(s) from {} {:?}:\n",
            self.failures.len(),
            self.engine,
            self.scope
        );
        for f in &self.failures {
            let loc = match (&f.file, f.line) {
                (Some(file), Some(line)) => format!("{file}:{line}"),
                (Some(file), None) => file.clone(),
                _ => f.symbol.clone().unwrap_or_else(|| "unknown".into()),
            };
            s.push_str(&format!("- [{:?}] {loc}: {}\n", f.kind, f.message));
            if let Some(d) = &f.detail {
                for line in d.lines().take(3) {
                    s.push_str(&format!("    {line}\n"));
                }
            }
        }
        s
    }
}

pub const INFRA_SIGNATURES: [&str; 10] = [
    "license",
    "licensing",
    "failed to acquire",
    "another instance",
    "editor is already running",
    "out of memory",
    "no such device",
    "could not create window",
    "vulkan",
    "no gpu",
];

pub fn looks_like_infrastructure(log: &str) -> Option<String> {
    let lower = log.to_lowercase();
    INFRA_SIGNATURES
        .iter()
        .find(|sig| lower.contains(*sig))
        .map(|sig| format!("log matched infrastructure signature '{sig}'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failure(id: &str, msg: &str) -> Failure {
        Failure {
            id: id.into(),
            kind: FailureKind::Test,
            symbol: None,
            file: Some("res://test/unit/test_dash.gd".into()),
            line: Some(12),
            message: msg.into(),
            detail: Some("expected 3 but got 4\nstack line\nanother\nfourth line".into()),
        }
    }

    fn result(failures: Vec<Failure>) -> VerifyResult {
        VerifyResult {
            verdict: if failures.is_empty() { Verdict::Pass } else { Verdict::Fail },
            failures,
            scope: VerifyScope::TestFast,
            engine: "godot".into(),
            duration_ms: 100,
            raw_report_path: Some("C:/out/gut-unit.xml".into()),
            inconclusive_reason: None,
        }
    }

    #[test]
    fn the_worker_brief_names_the_location_and_message() {
        let r = result(vec![failure("t1", "dash cooldown was not applied")]);
        let brief = r.brief_for_worker();
        assert!(brief.contains("test_dash.gd:12"));
        assert!(brief.contains("dash cooldown was not applied"));
    }

    #[test]
    fn the_worker_brief_never_carries_the_raw_report_path() {
        let r = result(vec![failure("t1", "boom")]);
        assert!(
            !r.brief_for_worker().contains("gut-unit.xml"),
            "agents must never be pointed at raw engine output"
        );
    }

    #[test]
    fn the_worker_brief_truncates_detail_to_a_few_lines() {
        let r = result(vec![failure("t1", "boom")]);
        let brief = r.brief_for_worker();
        assert!(brief.contains("expected 3 but got 4"));
        assert!(!brief.contains("fourth line"), "detail must stay short");
    }

    #[test]
    fn failure_digests_are_stable_for_dedup_across_rounds() {
        let a = failure("t1", "boom");
        let b = failure("t1", "boom");
        assert_eq!(a.digest(), b.digest());
        assert_ne!(a.digest(), failure("t2", "boom").digest());
    }

    #[test]
    fn infrastructure_signatures_are_recognised_in_logs() {
        assert!(looks_like_infrastructure("ERROR: Licensing failed").is_some());
        assert!(looks_like_infrastructure("Another instance is running").is_some());
        assert!(looks_like_infrastructure("Vulkan device not found").is_some());
        assert!(looks_like_infrastructure("Parse Error: expected ')'").is_none());
    }
}
