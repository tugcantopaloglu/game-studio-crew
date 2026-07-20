use anyhow::Result;
use std::collections::BTreeSet;
use std::path::PathBuf;
use studio_agents::role;
use studio_budget::{Enforcer, Projection};
use studio_engine::{EngineProfile, VerifyScope};
use studio_events::{EventType, Scene};
use studio_verify::{EngineDriver, ProfileDriver, ProjectPaths, Verdict};
use studio_workflow::{
    execute, Admission, Gate, GateKind, GateOutcome, Node, NodeOutcome, RunOutcome, Workflow,
    WorkflowHost,
};

use crate::m4::{run_worker, Emitter};

pub struct Host<'a> {
    pub em: &'a Emitter,
    pub budget: Enforcer,
    pub driver: Option<ProfileDriver>,
    pub paths: ProjectPaths,
    pub brief: String,
    pub seq: usize,
    pub auto_approve: bool,
    pub plan: Option<studio_workflow::Plan>,
}

impl<'a> Host<'a> {
    fn scope_of(gate: &Gate) -> Option<VerifyScope> {
        let key = gate.scope.as_deref()?;
        VerifyScope::ALL.into_iter().find(|s| s.key() == key)
    }
}

impl<'a> WorkflowHost for Host<'a> {
    fn admit(&mut self, node: &Node) -> Admission {
        let projection = Projection {
            prefix_tokens: 8_000,
            brief_tokens: 1_500,
            output_reserve: 2_000,
            prefix_is_warm: self.seq > 0,
        };
        match self.budget.admit(projection) {
            studio_budget::Admission::Admit => Admission::Admit,
            studio_budget::Admission::Degrade { step, reason } => {
                let _ = self.em.emit(
                    "daemon",
                    EventType::DegradationApplied,
                    Scene::daemon(),
                    serde_json::json!({"step": step.number(), "action": format!("{step:?}"), "reason": reason, "node": node.id}),
                );
                Admission::Degrade { step: step.number() }
            }
            studio_budget::Admission::Refuse { reason } => {
                let _ = self.em.emit(
                    "daemon",
                    EventType::BudgetExhausted,
                    Scene::daemon(),
                    serde_json::json!({"scope": "sprint", "reason": reason, "node": node.id}),
                );
                Admission::Refuse { reason }
            }
        }
    }

    fn enter(&mut self, node: &Node, inputs: &[String]) -> NodeOutcome {
        let r = match role(&node.role) {
            Some(r) => r,
            None => return NodeOutcome::Failed { reason: format!("unknown role {}", node.role) },
        };

        let _ = self.em.emit(
            "daemon",
            EventType::NodeEntered,
            Scene::daemon(),
            serde_json::json!({"node": node.id, "role": node.role}),
        );

        self.seq += 1;
        if let Some(plan) = &self.plan {
            if let Some(brief) = plan.brief_for(&node.id) {
                let upstream = if inputs.is_empty() {
                    String::new()
                } else {
                    format!("\n\nUpstream capsules: {}", inputs.join(", "))
                };
                let full = format!("{brief}{upstream}");
                return match run_worker(self.em, r, &full, self.seq) {
                    Ok(()) => NodeOutcome::Completed { capsule: format!("cap_{}", node.id) },
                    Err(e) => NodeOutcome::Failed { reason: e.to_string() },
                };
            }
        }

        let context = if inputs.is_empty() {
            String::new()
        } else {
            format!("\n\nUpstream capsules: {}", inputs.join(", "))
        };
        let brief = format!(
            "Workflow node '{}'.\n\n{}{}\n\nAnswer in one or two sentences. Use no tools.",
            node.id, self.brief, context
        );

        match run_worker(self.em, r, &brief, self.seq) {
            Ok(()) => NodeOutcome::Completed { capsule: format!("cap_{}", node.id) },
            Err(e) => NodeOutcome::Failed { reason: e.to_string() },
        }
    }

    fn gate(&mut self, gate: &Gate, node: &Node) -> GateOutcome {
        if gate.kind == GateKind::Approval {
            let passed = self.auto_approve;
            let _ = self.em.emit(
                "daemon",
                EventType::GateEvaluated,
                Scene::daemon(),
                serde_json::json!({"gate": node.id, "kind": "approval", "passed": passed}),
            );
            return if passed {
                GateOutcome::Pass
            } else {
                GateOutcome::Rejected { reason: "no human approved this gate".into() }
            };
        }

        let scope = match Self::scope_of(gate) {
            Some(s) => s,
            None => {
                return GateOutcome::Inconclusive { reason: "gate names no valid scope".into() }
            }
        };

        let driver = match &self.driver {
            Some(d) => d,
            None => {
                return GateOutcome::Inconclusive {
                    reason: "no engine is bound to this run".into(),
                }
            }
        };

        let _ = self.em.emit(
            "daemon",
            EventType::VerifyStarted,
            Scene::daemon(),
            serde_json::json!({"scope": scope.key(), "engine": driver.profile.id}),
        );

        let result = driver.verify(scope, &self.paths);

        let _ = self.em.emit(
            "daemon",
            EventType::VerifyResult,
            Scene::daemon(),
            serde_json::json!({
                "verdict": format!("{:?}", result.verdict).to_lowercase(),
                "failures": result.failures.iter().map(|f| f.digest()).collect::<Vec<_>>(),
                "duration_ms": result.duration_ms,
            }),
        );

        let _ = self.em.emit(
            "daemon",
            EventType::GateEvaluated,
            Scene::daemon(),
            serde_json::json!({
                "gate": node.id,
                "kind": "verify",
                "passed": result.verdict == Verdict::Pass,
            }),
        );

        match result.verdict {
            Verdict::Pass => GateOutcome::Pass,
            Verdict::Fail => GateOutcome::Fail { failures: result.failures.len() },
            Verdict::Inconclusive => GateOutcome::Inconclusive {
                reason: result
                    .inconclusive_reason
                    .unwrap_or_else(|| "verification was inconclusive".into()),
            },
        }
    }

    fn repair(&mut self, node: &Node, gate: &Gate, round: u32) -> GateOutcome {
        let _ = self.em.emit(
            "daemon",
            EventType::RepairRound,
            Scene::daemon(),
            serde_json::json!({"round": round, "node": node.id}),
        );

        let r = match role(&node.role) {
            Some(r) => r,
            None => return GateOutcome::Fail { failures: 1 },
        };

        let driver = match &self.driver {
            Some(d) => d,
            None => return GateOutcome::Inconclusive { reason: "no engine bound".into() },
        };
        let scope = match Self::scope_of(gate) {
            Some(s) => s,
            None => return GateOutcome::Inconclusive { reason: "no scope".into() },
        };

        let failures = driver.verify(scope, &self.paths);
        if failures.verdict == Verdict::Pass {
            return GateOutcome::Pass;
        }

        self.seq += 1;
        let brief = format!(
            "The project at {} failed verification.\n\n{}\n\
             Fix exactly what the list names and nothing else.",
            self.paths.project.display(),
            failures.brief_for_worker()
        );
        let _ = run_worker(self.em, r, &brief, self.seq);

        self.gate(gate, node)
    }

    fn skip(&mut self, node: &Node) {
        let _ = self.em.emit(
            "daemon",
            EventType::NodeEntered,
            Scene::daemon(),
            serde_json::json!({"node": node.id, "role": node.role, "skipped": true}),
        );
    }
}

pub fn run_workflow(
    em: &Emitter,
    workflow: &Workflow,
    brief: &str,
    project: Option<PathBuf>,
    seq: &mut usize,
) -> Result<RunOutcome> {
    run_planned(em, workflow, brief, project, seq, None)
}

pub fn run_planned(
    em: &Emitter,
    workflow: &Workflow,
    brief: &str,
    project: Option<PathBuf>,
    seq: &mut usize,
    plan: Option<studio_workflow::Plan>,
) -> Result<RunOutcome> {
    em.emit(
        "daemon",
        EventType::WorkflowStarted,
        Scene::daemon(),
        serde_json::json!({
            "workflow": workflow.id,
            "title": workflow.title,
            "nodes": workflow.nodes.iter().map(|n| &n.id).collect::<Vec<_>>(),
            "budget_tokens": workflow.total_budget(),
        }),
    )?;

    let (driver, paths) = match project {
        Some(root) if root.join("project.godot").exists() => {
            let profile = EngineProfile::parse(studio_engine::GODOT_PROFILE)
                .map_err(|e| anyhow::anyhow!("godot profile: {e}"))?;
            let out = root.parent().unwrap_or(&root).join("wf-out");
            let d = ProfileDriver::resolve(profile).ok();
            (d, ProjectPaths::new(root, out))
        }
        _ => (None, ProjectPaths::new(".", ".studio/wf-out")),
    };

    let mut host = Host {
        em,
        budget: Enforcer::new(workflow.total_budget(), workflow.total_budget() * 2),
        driver,
        paths,
        brief: brief.to_string(),
        seq: *seq,
        auto_approve: true,
        plan: plan.clone(),
    };

    let report = execute(workflow, &mut host, &BTreeSet::new())
        .map_err(|e| anyhow::anyhow!("workflow failed to execute: {e}"))?;
    *seq = host.seq;

    let outcome = report.outcome.clone().unwrap_or(RunOutcome::Completed);

    em.emit(
        "daemon",
        EventType::WorkflowEnded,
        Scene::daemon(),
        serde_json::json!({
            "workflow": workflow.id,
            "outcome": outcome.tag(),
            "entered": report.entered,
            "skipped": report.skipped,
            "gates_passed": report.gates_passed,
            "gates_failed": report.gates_failed,
            "repair_rounds": report.repair_rounds,
            "degradations": report.degradations,
        }),
    )?;

    println!(
        "  workflow {} -> {} ({} nodes, {} gates passed, {} repair rounds)",
        workflow.id,
        outcome.tag(),
        report.entered.len(),
        report.gates_passed,
        report.repair_rounds
    );

    Ok(outcome)
}
