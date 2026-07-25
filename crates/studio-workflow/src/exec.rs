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

#[derive(Debug, Clone, PartialEq)]
pub enum WaveVerdict {
    Continue,
    Redo,
    Stop { reason: String },
}

pub trait ParallelWorkflowHost: Sync {
    fn admit(&self, node: &Node) -> Admission;
    fn enter(&self, node: &Node, inputs: &[String]) -> NodeOutcome;
    fn gate(&self, gate: &Gate, node: &Node) -> GateOutcome;
    fn repair(&self, node: &Node, gate: &Gate, round: u32) -> GateOutcome;
    fn skip(&self, _node: &Node) {}
    fn wave_done(&self, _completed: &[&Node]) {}
    fn before_wave(&self, _ready: &[&Node]) -> WaveVerdict {
        WaveVerdict::Continue
    }
    fn after_wave(&self, _completed: &[&Node]) -> WaveVerdict {
        WaveVerdict::Continue
    }
}

pub const MAX_REPAIR_ROUNDS: u32 = 3;

#[derive(Debug, Clone, PartialEq)]
pub enum RunOutcome {
    Completed,
    Blocked { node: String, reason: String },
    Escalated { node: String, reason: String },
    RoutedToInfra { node: String, reason: String },
    Refused { node: String, reason: String },
    Interrupted { node: String, reason: String },
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
            RunOutcome::Interrupted { .. } => "interrupted",
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
    pub redo_rounds: u32,
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

pub fn execute_parallel<H: ParallelWorkflowHost>(
    wf: &Workflow,
    host: &H,
    already_done: &BTreeSet<String>,
    max_parallel: usize,
) -> Result<RunReport, WorkflowError> {
    let order = wf.resume_from(already_done)?;
    let width = max_parallel.max(1);
    let mut report = RunReport::default();

    let mut deps: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for id in &order {
        deps.insert(id.as_str(), BTreeSet::new());
    }
    for e in &wf.edges {
        if let Some(set) = deps.get_mut(e.to.as_str()) {
            if !already_done.contains(&e.from) {
                set.insert(e.from.as_str());
            }
        }
    }

    let mut pending: Vec<&str> = order.iter().map(String::as_str).collect();
    let mut settled: BTreeSet<&str> = BTreeSet::new();
    let mut stop: Option<RunOutcome> = None;

    while stop.is_none() && !pending.is_empty() {
        let ready: Vec<&Node> = pending
            .iter()
            .filter(|id| deps[**id].iter().all(|d| settled.contains(d)))
            .map(|id| wf.node(id).expect("order only yields real nodes"))
            .collect();
        if ready.is_empty() {
            break;
        }

        if let WaveVerdict::Stop { reason } = host.before_wave(&ready) {
            stop = Some(RunOutcome::Interrupted {
                node: ready[0].id.clone(),
                reason,
            });
            break;
        }

        let mut wave: Vec<(&Node, Vec<String>)> = Vec::new();
        for node in ready.iter().copied() {
            match host.admit(node) {
                Admission::Admit => {}
                Admission::Degrade { step } => report.degradations.push(step),
                Admission::Refuse { reason } => {
                    stop = Some(RunOutcome::Refused { node: node.id.clone(), reason });
                    break;
                }
            }
            if stop.is_some() {
                break;
            }

            let inputs: Vec<String> = node
                .inputs
                .iter()
                .filter_map(|dep| report.capsules.get(dep).cloned())
                .collect();

            if node.optional && inputs.len() < node.inputs.len() {
                report.skipped.push(node.id.clone());
                host.skip(node);
                pending.retain(|id| *id != node.id);
                settled.insert(node.id.as_str());
                continue;
            }

            wave.push((node, inputs));
        }
        if stop.is_some() || wave.is_empty() {
            continue;
        }

        let mut outcomes: Vec<(&Node, NodeOutcome)> = Vec::new();
        for chunk in wave.chunks(width) {
            let batch = std::thread::scope(|scope| {
                let handles: Vec<_> = chunk
                    .iter()
                    .map(|(node, inputs)| scope.spawn(move || (*node, host.enter(node, inputs))))
                    .collect();
                handles.into_iter().map(|h| h.join().expect("worker thread panicked")).collect::<Vec<_>>()
            });
            outcomes.extend(batch);
        }

        let mut completed: Vec<&Node> = Vec::new();
        for (node, outcome) in outcomes {
            pending.retain(|id| *id != node.id);
            settled.insert(node.id.as_str());
            if !report.entered.contains(&node.id) {
                report.entered.push(node.id.clone());
            }
            match outcome {
                NodeOutcome::Completed { capsule } => {
                    report.capsules.insert(node.id.clone(), capsule);
                    completed.push(node);
                }
                NodeOutcome::Failed { reason } => {
                    if node.optional {
                        report.entered.retain(|id| id != &node.id);
                        report.skipped.push(node.id.clone());
                    } else if stop.is_none() {
                        stop = Some(RunOutcome::Blocked { node: node.id.clone(), reason });
                    }
                }
            }
        }

        if !completed.is_empty() {
            host.wave_done(&completed);
        }

        for node in completed.iter().copied() {
            if stop.is_some() {
                break;
            }
            for gate in wf.gates_after(&node.id) {
                if let Err(outcome) = run_parallel_gate(host, node, gate, &mut report) {
                    stop = Some(outcome);
                    break;
                }
            }
        }

        if stop.is_some() || completed.is_empty() {
            continue;
        }

        match host.after_wave(&completed) {
            WaveVerdict::Continue => {}
            WaveVerdict::Stop { reason } => {
                stop = Some(RunOutcome::Interrupted {
                    node: completed[0].id.clone(),
                    reason,
                });
            }
            WaveVerdict::Redo => {
                report.redo_rounds += 1;
                for node in completed {
                    settled.remove(node.id.as_str());
                    report.capsules.remove(&node.id);
                }
                pending = order
                    .iter()
                    .map(String::as_str)
                    .filter(|id| !settled.contains(id))
                    .collect();
            }
        }
    }

    report.outcome = Some(stop.unwrap_or(RunOutcome::Completed));
    Ok(report)
}

fn run_parallel_gate<H: ParallelWorkflowHost>(
    host: &H,
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

    if gate.kind == GateKind::Verify
        && matches!(result, GateOutcome::Fail { .. })
        && gate.on_fail == OnFail::Repair
    {
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

    if gate.kind == GateKind::Verify
        && matches!(result, GateOutcome::Fail { .. })
        && gate.on_fail == OnFail::Repair
    {
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

    mod parallel {
        use super::super::*;
        use crate::{Edge, FEATURE};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Mutex;
        use std::time::Duration;

        #[derive(Default)]
        struct ParHost {
            active: AtomicUsize,
            peak: AtomicUsize,
            entered: Mutex<Vec<String>>,
            waves: Mutex<Vec<Vec<String>>>,
            fail_node: Option<String>,
            stop_before: Option<String>,
            redo_after: Option<String>,
            redone: AtomicUsize,
            notes: Mutex<Vec<String>>,
            briefs: Mutex<Vec<String>>,
        }

        impl ParallelWorkflowHost for ParHost {
            fn admit(&self, _node: &Node) -> Admission {
                Admission::Admit
            }

            fn enter(&self, node: &Node, _inputs: &[String]) -> NodeOutcome {
                let now = self.active.fetch_add(1, Ordering::SeqCst) + 1;
                self.peak.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(25));
                self.active.fetch_sub(1, Ordering::SeqCst);
                self.entered.lock().unwrap().push(node.id.clone());
                let notes = self.notes.lock().unwrap().join(" ");
                self.briefs.lock().unwrap().push(format!("{} {notes}", node.id));
                if self.fail_node.as_deref() == Some(node.id.as_str()) {
                    return NodeOutcome::Failed { reason: "worker crashed".into() };
                }
                NodeOutcome::Completed { capsule: format!("cap_{}", node.id) }
            }

            fn before_wave(&self, ready: &[&Node]) -> WaveVerdict {
                match &self.stop_before {
                    Some(id) if ready.iter().any(|n| &n.id == id) => {
                        WaveVerdict::Stop { reason: "you stopped the run".into() }
                    }
                    _ => WaveVerdict::Continue,
                }
            }

            fn after_wave(&self, completed: &[&Node]) -> WaveVerdict {
                self.notes.lock().unwrap().clear();
                let asked = self
                    .redo_after
                    .as_deref()
                    .is_some_and(|id| completed.iter().any(|n| n.id == id));
                if !asked || self.redone.fetch_add(1, Ordering::SeqCst) > 0 {
                    return WaveVerdict::Continue;
                }
                self.notes.lock().unwrap().push("make the pipes green".into());
                WaveVerdict::Redo
            }

            fn gate(&self, _gate: &Gate, _node: &Node) -> GateOutcome {
                GateOutcome::Pass
            }

            fn repair(&self, _node: &Node, _gate: &Gate, _round: u32) -> GateOutcome {
                GateOutcome::Pass
            }

            fn wave_done(&self, completed: &[&Node]) {
                self.waves
                    .lock()
                    .unwrap()
                    .push(completed.iter().map(|n| n.id.clone()).collect());
            }
        }

        fn diamond() -> Workflow {
            let node = |id: &str, inputs: &[&str]| Node {
                id: id.into(),
                role: "gameplay_engineer".into(),
                inputs: inputs.iter().map(|s| s.to_string()).collect(),
                budget_tokens: 1,
                optional: false,
            };
            let edge = |from: &str, to: &str| Edge {
                from: from.into(),
                to: to.into(),
                carries: "task_return".into(),
            };
            Workflow {
                schema_version: 1,
                id: "diamond".into(),
                title: "Diamond".into(),
                nodes: vec![
                    node("t1", &[]),
                    node("t2", &["t1"]),
                    node("t3", &["t1"]),
                    node("t4", &["t1"]),
                    node("t5", &["t2", "t3", "t4"]),
                ],
                edges: vec![
                    edge("t1", "t2"),
                    edge("t1", "t3"),
                    edge("t1", "t4"),
                    edge("t2", "t5"),
                    edge("t3", "t5"),
                    edge("t4", "t5"),
                ],
                gates: Vec::new(),
            }
        }

        #[test]
        fn independent_nodes_actually_overlap() {
            let wf = diamond();
            let h = ParHost::default();
            let r = execute_parallel(&wf, &h, &BTreeSet::new(), 4).unwrap();
            assert_eq!(r.outcome, Some(RunOutcome::Completed));
            assert_eq!(r.entered.len(), 5);
            assert!(
                h.peak.load(Ordering::SeqCst) >= 2,
                "t2, t3 and t4 share no dependency and must run at the same time"
            );
        }

        #[test]
        fn dependencies_still_hold_under_parallelism() {
            let wf = diamond();
            let h = ParHost::default();
            execute_parallel(&wf, &h, &BTreeSet::new(), 4).unwrap();
            let entered = h.entered.lock().unwrap();
            let pos = |id: &str| entered.iter().position(|x| x == id).unwrap();
            assert!(pos("t1") < pos("t2"));
            assert!(pos("t1") < pos("t3"));
            assert!(pos("t1") < pos("t4"));
            assert!(pos("t5") > pos("t2"));
            assert!(pos("t5") > pos("t3"));
            assert!(pos("t5") > pos("t4"));
        }

        #[test]
        fn width_one_serializes_without_changing_the_outcome() {
            let wf = diamond();
            let h = ParHost::default();
            let r = execute_parallel(&wf, &h, &BTreeSet::new(), 1).unwrap();
            assert_eq!(r.outcome, Some(RunOutcome::Completed));
            assert_eq!(h.peak.load(Ordering::SeqCst), 1);
        }

        #[test]
        fn a_failed_required_node_blocks_and_stops_later_waves() {
            let wf = diamond();
            let h = ParHost { fail_node: Some("t3".into()), ..Default::default() };
            let r = execute_parallel(&wf, &h, &BTreeSet::new(), 4).unwrap();
            assert!(matches!(r.outcome, Some(RunOutcome::Blocked { ref node, .. }) if node == "t3"));
            assert!(!r.entered.contains(&"t5".to_string()), "t5 must not start after t3 died");
        }

        #[test]
        fn every_wave_is_reported_for_committing() {
            let wf = diamond();
            let h = ParHost::default();
            execute_parallel(&wf, &h, &BTreeSet::new(), 4).unwrap();
            let waves = h.waves.lock().unwrap();
            assert_eq!(waves.len(), 3);
            assert_eq!(waves[0], vec!["t1"]);
            assert_eq!(waves[2], vec!["t5"]);
            let mut mid = waves[1].clone();
            mid.sort();
            assert_eq!(mid, vec!["t2", "t3", "t4"]);
        }

        #[test]
        fn the_feature_workflow_completes_in_parallel_mode() {
            let wf = Workflow::parse(FEATURE).unwrap();
            let h = ParHost::default();
            let r = execute_parallel(&wf, &h, &BTreeSet::new(), 4).unwrap();
            assert_eq!(r.outcome, Some(RunOutcome::Completed));
            assert_eq!(r.entered.len(), wf.nodes.len());
            assert_eq!(r.gates_passed, wf.gates.len());
        }

        #[test]
        fn an_interrupt_lands_between_waves_instead_of_being_swallowed() {
            let wf = diamond();
            let h = ParHost { stop_before: Some("t2".into()), ..Default::default() };
            let r = execute_parallel(&wf, &h, &BTreeSet::new(), 4).unwrap();

            match r.outcome {
                Some(RunOutcome::Interrupted { ref reason, .. }) => {
                    assert!(reason.contains("stopped"))
                }
                other => panic!("expected an interruption, got {other:?}"),
            }
            assert_eq!(r.entered, vec!["t1".to_string()], "t1 was already running when you hit stop");
            let entered = h.entered.lock().unwrap();
            for later in ["t2", "t3", "t4", "t5"] {
                assert!(!entered.contains(&later.to_string()), "{later} started after the stop");
            }
        }

        #[test]
        fn an_interrupt_before_the_first_wave_spends_nothing() {
            let wf = diamond();
            let h = ParHost { stop_before: Some("t1".into()), ..Default::default() };
            let r = execute_parallel(&wf, &h, &BTreeSet::new(), 4).unwrap();
            assert!(matches!(r.outcome, Some(RunOutcome::Interrupted { .. })));
            assert!(r.entered.is_empty(), "an interrupt that arrives first pays for no worker");
        }

        #[test]
        fn a_rejected_step_runs_its_whole_tier_again_with_the_notes() {
            let wf = diamond();
            let h = ParHost { redo_after: Some("t1".into()), ..Default::default() };
            let r = execute_parallel(&wf, &h, &BTreeSet::new(), 4).unwrap();

            assert_eq!(r.outcome, Some(RunOutcome::Completed));
            assert_eq!(r.redo_rounds, 1);
            assert_eq!(r.entered.len(), 5, "a redo re-enters a node, it does not add one");

            let entered = h.entered.lock().unwrap();
            assert_eq!(
                entered.iter().filter(|id| *id == "t1").count(),
                2,
                "the rejected tier runs a second time"
            );

            let briefs = h.briefs.lock().unwrap();
            assert_eq!(briefs[0], "t1 ", "the first attempt had nothing to go on");
            assert_eq!(
                briefs[1], "t1 make the pipes green",
                "the second attempt is briefed with what the human said was wrong"
            );
            assert!(
                briefs[2..].iter().all(|b| !b.contains("make the pipes green")),
                "notes for one tier must not leak into the next"
            );
        }

        #[test]
        fn a_tier_the_human_sends_back_still_blocks_everything_downstream() {
            let wf = diamond();
            let h = ParHost { redo_after: Some("t1".into()), ..Default::default() };
            execute_parallel(&wf, &h, &BTreeSet::new(), 4).unwrap();

            let entered = h.entered.lock().unwrap();
            let second_t1 = entered.iter().rposition(|id| id == "t1").unwrap();
            for later in ["t2", "t3", "t4", "t5"] {
                let at = entered.iter().position(|id| id == later).unwrap();
                assert!(at > second_t1, "{later} ran before the redo of t1 finished");
            }
        }

        #[test]
        fn resuming_skips_finished_nodes_in_parallel_mode() {
            let wf = diamond();
            let done: BTreeSet<String> = ["t1".into()].into_iter().collect();
            let h = ParHost::default();
            let r = execute_parallel(&wf, &h, &done, 4).unwrap();
            assert_eq!(r.outcome, Some(RunOutcome::Completed));
            assert!(!r.entered.contains(&"t1".to_string()));
            assert_eq!(r.entered.len(), 4);
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
        assert_eq!(
            RunOutcome::Interrupted { node: "n".into(), reason: "r".into() }.tag(),
            "interrupted"
        );
    }
}
