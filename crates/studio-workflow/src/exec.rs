use crate::{Gate, GateKind, Node, OnFail, Workflow, WorkflowError};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq)]
pub enum NodeOutcome {
    Completed { capsule: String },
    Failed { reason: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum GateOutcome {
    Pass,
    Fail { failures: usize },
    Inconclusive { reason: String },
    Rejected { reason: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Admission {
    Admit,
    Degrade { step: u8 },
    Refuse { reason: String },
}

pub trait WorkflowHost {
    fn admit(&mut self, node: &Node) -> Admission;
    fn enter(&mut self, node: &Node, inputs: &[String]) -> NodeOutcome;
    fn gate(&mut self, gate: &Gate, node: &Node) -> GateOutcome;
    fn repair(&mut self, node: &Node, gate: &Gate, round: u32) -> GateOutcome;
    fn skip(&mut self, _node: &Node) {}
}

pub const MAX_REPAIR_ROUNDS: u32 = 3;

#[derive(Debug, Clone, PartialEq)]
pub enum RunOutcome {
    Completed,
    Blocked { node: String, reason: String },
    Escalated { node: String, reason: String },
    RoutedToInfra { node: String, reason: String },
    Refused { node: String, reason: String },
}

impl RunOutcome {
    pub fn is_clean(&self) -> bool {
        matches!(self, RunOutcome::Completed)
    }

    pub fn tag(&self) -> &'static str {
        match self {
            RunOutcome::Completed => "completed",
            RunOutcome::Blocked { .. } => "blocked",
            RunOutcome::Escalated { .. } => "escalated",
            RunOutcome::RoutedToInfra { .. } => "inconclusive",
            RunOutcome::Refused { .. } => "refused",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunReport {
    pub outcome: Option<RunOutcome>,
    pub entered: Vec<String>,
    pub skipped: Vec<String>,
    pub capsules: BTreeMap<String, String>,
    pub gates_passed: usize,
    pub gates_failed: usize,
    pub repair_rounds: u32,
    pub degradations: Vec<u8>,
}

pub fn execute<H: WorkflowHost>(
    wf: &Workflow,
    host: &mut H,
    already_done: &BTreeSet<String>,
) -> Result<RunReport, WorkflowError> {
    let order = wf.resume_from(already_done)?;
    let mut report = RunReport::default();

    for id in order {
        let node = wf.node(&id).expect("order only yields real nodes");

        match host.admit(node) {
            Admission::Admit => {}
            Admission::Degrade { step } => report.degradations.push(step),
            Admission::Refuse { reason } => {
                report.outcome = Some(RunOutcome::Refused { node: id.clone(), reason });
                return Ok(report);
            }
        }

        let inputs: Vec<String> = node
            .inputs
            .iter()
            .filter_map(|dep| report.capsules.get(dep).cloned())
            .collect();

        if node.optional && inputs.len() < node.inputs.len() {
            report.skipped.push(id.clone());
            host.skip(node);
            continue;
        }

        report.entered.push(id.clone());
        match host.enter(node, &inputs) {
            NodeOutcome::Completed { capsule } => {
                report.capsules.insert(id.clone(), capsule);
            }
            NodeOutcome::Failed { reason } => {
                if node.optional {
                    report.skipped.push(id.clone());
                    continue;
                }
                report.outcome = Some(RunOutcome::Blocked { node: id.clone(), reason });
                return Ok(report);
            }
        }

        for gate in wf.gates_after(&id) {
            match run_gate(wf, host, node, gate, &mut report) {
                Ok(()) => {}
                Err(outcome) => {
                    report.outcome = Some(outcome);
                    return Ok(report);
                }
            }
        }
    }

    report.outcome = Some(RunOutcome::Completed);
    Ok(report)
}

fn run_gate<H: WorkflowHost>(
    _wf: &Workflow,
    host: &mut H,
    node: &Node,
    gate: &Gate,
    report: &mut RunReport,
) -> Result<(), RunOutcome> {
    let mut result = host.gate(gate, node);

    if let GateOutcome::Inconclusive { reason } = &result {
        return Err(RunOutcome::RoutedToInfra {
            node: node.id.clone(),
            reason: reason.clone(),
        });
    }

    if gate.kind == GateKind::Verify && matches!(result, GateOutcome::Fail { .. }) {
        if gate.on_fail == OnFail::Repair {
            for round in 1..=MAX_REPAIR_ROUNDS {
                report.repair_rounds += 1;
                result = host.repair(node, gate, round);
                match &result {
                    GateOutcome::Pass => break,
                    GateOutcome::Inconclusive { reason } => {
                        return Err(RunOutcome::RoutedToInfra {
                            node: node.id.clone(),
                            reason: reason.clone(),
                        })
                    }
                    _ => {}
                }
            }
        }
    }

    match result {
        GateOutcome::Pass => {
            report.gates_passed += 1;
            Ok(())
        }
        GateOutcome::Inconclusive { reason } => Err(RunOutcome::RoutedToInfra {
            node: node.id.clone(),
            reason,
        }),
        GateOutcome::Rejected { reason } => {
            report.gates_failed += 1;
            Err(RunOutcome::Blocked { node: node.id.clone(), reason })
        }
        GateOutcome::Fail { failures } => {
            report.gates_failed += 1;
            let reason = format!("{failures} failure(s) survived the gate after {}", node.id);
            Err(match gate.on_fail {
                OnFail::Escalate | OnFail::Repair => {
                    RunOutcome::Escalated { node: node.id.clone(), reason }
                }
                OnFail::Block => RunOutcome::Blocked { node: node.id.clone(), reason },
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BUGFIX, FEATURE};

    #[derive(Default)]
    struct Host {
        fail_gate_at: Option<String>,
        repair_after: u32,
        inconclusive_at: Option<String>,
        fail_node: Option<String>,
        refuse_at: Option<String>,
        degrade_at: Option<String>,
        reject_approval: bool,
        repairs: u32,
        entered: Vec<String>,
    }

    impl WorkflowHost for Host {
        fn admit(&mut self, node: &Node) -> Admission {
            if self.refuse_at.as_deref() == Some(node.id.as_str()) {
                return Admission::Refuse { reason: "over budget".into() };
            }
            if self.degrade_at.as_deref() == Some(node.id.as_str()) {
                return Admission::Degrade { step: 1 };
            }
            Admission::Admit
        }

        fn enter(&mut self, node: &Node, _inputs: &[String]) -> NodeOutcome {
            self.entered.push(node.id.clone());
            if self.fail_node.as_deref() == Some(node.id.as_str()) {
                return NodeOutcome::Failed { reason: "worker crashed".into() };
            }
            NodeOutcome::Completed { capsule: format!("cap_{}", node.id) }
        }

        fn gate(&mut self, gate: &Gate, node: &Node) -> GateOutcome {
            if self.inconclusive_at.as_deref() == Some(node.id.as_str()) {
                return GateOutcome::Inconclusive { reason: "editor lock held".into() };
            }
            if gate.kind == GateKind::Approval && self.reject_approval {
                return GateOutcome::Rejected { reason: "the director said no".into() };
            }
            if self.fail_gate_at.as_deref() == Some(node.id.as_str()) {
                return GateOutcome::Fail { failures: 2 };
            }
            GateOutcome::Pass
        }

        fn repair(&mut self, _node: &Node, _gate: &Gate, round: u32) -> GateOutcome {
            self.repairs += 1;
            if self.repair_after > 0 && round >= self.repair_after {
                GateOutcome::Pass
            } else {
                GateOutcome::Fail { failures: 1 }
            }
        }
    }

    fn feature() -> Workflow {
        Workflow::parse(FEATURE).unwrap()
    }

    #[test]
    fn a_clean_run_enters_every_node_and_passes_every_gate() {
        let wf = feature();
        let mut h = Host::default();
        let r = execute(&wf, &mut h, &BTreeSet::new()).unwrap();
        assert_eq!(r.outcome, Some(RunOutcome::Completed));
        assert_eq!(r.entered.len(), wf.nodes.len());
        assert_eq!(r.gates_passed, wf.gates.len());
        assert_eq!(r.gates_failed, 0);
    }

    #[test]
    fn nodes_run_in_dependency_order() {
        let wf = feature();
        let mut h = Host::default();
        execute(&wf, &mut h, &BTreeSet::new()).unwrap();
        let pos = |id: &str| h.entered.iter().position(|x| x == id).unwrap();
        assert!(pos("design") < pos("implement"));
        assert!(pos("implement") < pos("integrate"));
        assert!(pos("integrate") < pos("review"));
    }

    #[test]
    fn a_failing_gate_with_repair_reruns_until_it_goes_green() {
        let wf = feature();
        let mut h = Host { fail_gate_at: Some("implement".into()), repair_after: 2, ..Default::default() };
        let r = execute(&wf, &mut h, &BTreeSet::new()).unwrap();
        assert_eq!(r.outcome, Some(RunOutcome::Completed));
        assert_eq!(r.repair_rounds, 2);
        assert_eq!(h.repairs, 2);
    }

    #[test]
    fn repair_gives_up_after_three_rounds_and_escalates() {
        let wf = feature();
        let mut h = Host { fail_gate_at: Some("implement".into()), repair_after: 0, ..Default::default() };
        let r = execute(&wf, &mut h, &BTreeSet::new()).unwrap();
        assert_eq!(r.repair_rounds, MAX_REPAIR_ROUNDS);
        assert!(matches!(r.outcome, Some(RunOutcome::Escalated { .. })));
        assert!(!r.entered.contains(&"review".to_string()), "the run stops at the failing gate");
    }

    #[test]
    fn an_inconclusive_gate_routes_to_infra_without_spending_a_repair_round() {
        let wf = feature();
        let mut h = Host { inconclusive_at: Some("implement".into()), ..Default::default() };
        let r = execute(&wf, &mut h, &BTreeSet::new()).unwrap();
        assert_eq!(r.repair_rounds, 0, "there is nothing for an agent to repair");
        match r.outcome {
            Some(RunOutcome::RoutedToInfra { reason, .. }) => assert!(reason.contains("editor lock")),
            other => panic!("expected the infra queue, got {other:?}"),
        }
    }

    #[test]
    fn a_rejected_approval_blocks_rather_than_escalating() {
        let wf = feature();
        let mut h = Host { reject_approval: true, ..Default::default() };
        let r = execute(&wf, &mut h, &BTreeSet::new()).unwrap();
        match r.outcome {
            Some(RunOutcome::Blocked { node, reason }) => {
                assert_eq!(node, "review");
                assert!(reason.contains("director"));
            }
            other => panic!("expected a block, got {other:?}"),
        }
    }

    #[test]
    fn a_budget_refusal_stops_the_run_before_the_node_is_entered() {
        let wf = feature();
        let mut h = Host { refuse_at: Some("implement".into()), ..Default::default() };
        let r = execute(&wf, &mut h, &BTreeSet::new()).unwrap();
        assert!(matches!(r.outcome, Some(RunOutcome::Refused { .. })));
        assert!(!h.entered.contains(&"implement".to_string()), "refused means no tokens paid");
    }

    #[test]
    fn a_degradation_is_recorded_and_the_run_continues() {
        let wf = feature();
        let mut h = Host { degrade_at: Some("implement".into()), ..Default::default() };
        let r = execute(&wf, &mut h, &BTreeSet::new()).unwrap();
        assert_eq!(r.degradations, vec![1]);
        assert_eq!(r.outcome, Some(RunOutcome::Completed));
    }

    #[test]
    fn a_failed_required_node_blocks_the_run() {
        let wf = feature();
        let mut h = Host { fail_node: Some("implement".into()), ..Default::default() };
        let r = execute(&wf, &mut h, &BTreeSet::new()).unwrap();
        match r.outcome {
            Some(RunOutcome::Blocked { node, .. }) => assert_eq!(node, "implement"),
            other => panic!("expected a block, got {other:?}"),
        }
    }

    #[test]
    fn a_failed_optional_node_is_skipped_and_the_run_carries_on() {
        let wf = feature();
        let mut h = Host { fail_node: Some("art_pass".into()), ..Default::default() };
        let r = execute(&wf, &mut h, &BTreeSet::new()).unwrap();
        assert_eq!(r.outcome, Some(RunOutcome::Completed));
        assert!(r.skipped.contains(&"art_pass".to_string()));
        assert!(r.entered.contains(&"integrate".to_string()));
    }

    #[test]
    fn resuming_does_not_re_enter_finished_nodes() {
        let wf = feature();
        let done: BTreeSet<String> = ["design".into(), "implement".into()].into_iter().collect();
        let mut h = Host::default();
        let r = execute(&wf, &mut h, &done).unwrap();
        assert!(!r.entered.contains(&"design".to_string()));
        assert!(!r.entered.contains(&"implement".to_string()));
        assert_eq!(r.outcome, Some(RunOutcome::Completed));
    }

    #[test]
    fn a_capsule_reaches_the_node_that_declares_it_as_input() {
        let wf = feature();
        let mut h = Host::default();
        let r = execute(&wf, &mut h, &BTreeSet::new()).unwrap();
        assert_eq!(r.capsules.get("design"), Some(&"cap_design".to_string()));
        assert_eq!(r.capsules.len(), wf.nodes.len());
    }

    #[test]
    fn bugfix_stops_at_the_reproduction_gate_when_the_bug_will_not_reproduce() {
        let wf = Workflow::parse(BUGFIX).unwrap();
        let mut h = Host { fail_gate_at: Some("triage".into()), ..Default::default() };
        let r = execute(&wf, &mut h, &BTreeSet::new()).unwrap();
        assert!(matches!(r.outcome, Some(RunOutcome::Escalated { .. })));
        assert!(!h.entered.contains(&"fix".to_string()), "never fix a bug you cannot reproduce");
        assert_eq!(r.repair_rounds, 0, "the reproduction gate escalates, it does not repair");
    }

    #[test]
    fn every_builtin_workflow_runs_clean_on_a_healthy_host() {
        for wf in Workflow::builtin() {
            let mut h = Host::default();
            let r = execute(&wf, &mut h, &BTreeSet::new()).unwrap();
            assert_eq!(r.outcome, Some(RunOutcome::Completed), "{} did not finish", wf.id);
            assert_eq!(r.entered.len(), wf.nodes.len(), "{} skipped a node", wf.id);
        }
    }

    #[test]
    fn outcome_tags_are_stable_for_the_event_wire() {
        assert_eq!(RunOutcome::Completed.tag(), "completed");
        assert_eq!(
            RunOutcome::Blocked { node: "n".into(), reason: "r".into() }.tag(),
            "blocked"
        );
        assert_eq!(
            RunOutcome::RoutedToInfra { node: "n".into(), reason: "r".into() }.tag(),
            "inconclusive"
        );
    }
}
