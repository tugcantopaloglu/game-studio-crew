use crate::{VerifyResult, Verdict};
use std::collections::BTreeSet;

pub const MAX_REPAIR_ROUNDS: u32 = 3;

#[derive(Debug, Clone, PartialEq)]
pub enum RepairStep {
    Done,
    Reinvoke { round: u32, brief: String, failure_count: usize },
    RouteToInfra { reason: String },
    Escalate { rounds_spent: u32, unresolved: Vec<String> },
}

#[derive(Debug, Default)]
pub struct RepairLoop {
    round: u32,
    seen: BTreeSet<String>,
    last_failures: Vec<String>,
}

impl RepairLoop {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn round(&self) -> u32 {
        self.round
    }

    pub fn observe(&mut self, result: &VerifyResult) -> RepairStep {
        match result.verdict {
            Verdict::Pass => RepairStep::Done,

            Verdict::Inconclusive => RepairStep::RouteToInfra {
                reason: result
                    .inconclusive_reason
                    .clone()
                    .unwrap_or_else(|| "verification was inconclusive".into()),
            },

            Verdict::Fail => {
                self.last_failures = result.failures.iter().map(|f| f.digest()).collect();
                for d in &self.last_failures {
                    self.seen.insert(d.clone());
                }

                if self.round >= MAX_REPAIR_ROUNDS {
                    return RepairStep::Escalate {
                        rounds_spent: self.round,
                        unresolved: self.last_failures.clone(),
                    };
                }

                self.round += 1;
                RepairStep::Reinvoke {
                    round: self.round,
                    brief: result.brief_for_worker(),
                    failure_count: result.failures.len(),
                }
            }
        }
    }

    pub fn distinct_failures_seen(&self) -> usize {
        self.seen.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Failure, FailureKind};
    use studio_engine::VerifyScope;

    fn result(verdict: Verdict, n: usize) -> VerifyResult {
        VerifyResult {
            verdict,
            failures: (0..n)
                .map(|i| Failure {
                    id: format!("t{i}"),
                    kind: FailureKind::Test,
                    symbol: None,
                    file: Some("res://test/unit/test_dash.gd".into()),
                    line: Some(10 + i as u32),
                    message: format!("failure {i}"),
                    detail: None,
                })
                .collect(),
            scope: VerifyScope::TestFast,
            engine: "godot".into(),
            duration_ms: 10,
            raw_report_path: None,
            inconclusive_reason: if verdict == Verdict::Inconclusive {
                Some("editor lock held".into())
            } else {
                None
            },
        }
    }

    #[test]
    fn a_pass_ends_the_loop_immediately() {
        let mut l = RepairLoop::new();
        assert_eq!(l.observe(&result(Verdict::Pass, 0)), RepairStep::Done);
        assert_eq!(l.round(), 0);
    }

    #[test]
    fn a_failure_reinvokes_with_the_structured_brief() {
        let mut l = RepairLoop::new();
        match l.observe(&result(Verdict::Fail, 2)) {
            RepairStep::Reinvoke { round, brief, failure_count } => {
                assert_eq!(round, 1);
                assert_eq!(failure_count, 2);
                assert!(brief.contains("failure 0"));
                assert!(brief.contains("test_dash.gd:10"));
            }
            other => panic!("expected Reinvoke, got {other:?}"),
        }
    }

    #[test]
    fn the_loop_escalates_after_three_rounds_rather_than_looping_forever() {
        let mut l = RepairLoop::new();
        for expected in 1..=MAX_REPAIR_ROUNDS {
            match l.observe(&result(Verdict::Fail, 1)) {
                RepairStep::Reinvoke { round, .. } => assert_eq!(round, expected),
                other => panic!("round {expected}: expected Reinvoke, got {other:?}"),
            }
        }
        match l.observe(&result(Verdict::Fail, 1)) {
            RepairStep::Escalate { rounds_spent, unresolved } => {
                assert_eq!(rounds_spent, MAX_REPAIR_ROUNDS);
                assert_eq!(unresolved.len(), 1);
            }
            other => panic!("expected Escalate, got {other:?}"),
        }
    }

    #[test]
    fn an_inconclusive_verdict_routes_to_infra_without_spending_a_round() {
        let mut l = RepairLoop::new();
        l.observe(&result(Verdict::Fail, 1));
        assert_eq!(l.round(), 1);

        match l.observe(&result(Verdict::Inconclusive, 0)) {
            RepairStep::RouteToInfra { reason } => assert!(reason.contains("editor lock")),
            other => panic!("expected RouteToInfra, got {other:?}"),
        }
        assert_eq!(l.round(), 1, "an inconclusive run must not consume a repair round");
    }

    #[test]
    fn an_inconclusive_first_round_never_reaches_an_agent() {
        let mut l = RepairLoop::new();
        assert!(matches!(
            l.observe(&result(Verdict::Inconclusive, 0)),
            RepairStep::RouteToInfra { .. }
        ));
        assert_eq!(l.round(), 0);
    }

    #[test]
    fn a_pass_after_a_failed_round_ends_the_loop() {
        let mut l = RepairLoop::new();
        l.observe(&result(Verdict::Fail, 1));
        assert_eq!(l.observe(&result(Verdict::Pass, 0)), RepairStep::Done);
    }

    #[test]
    fn distinct_failures_accumulate_across_rounds() {
        let mut l = RepairLoop::new();
        l.observe(&result(Verdict::Fail, 2));
        l.observe(&result(Verdict::Fail, 3));
        assert_eq!(l.distinct_failures_seen(), 3, "digests dedup across rounds");
    }
}
