use anyhow::Result;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use studio_agents::role;
use studio_budget::{Enforcer, Projection};
use studio_engine::{EngineProfile, VerifyScope};
use studio_events::{EventType, Scene};
use studio_verify::{EngineDriver, ProfileDriver, ProjectPaths, Verdict};
use studio_workflow::{
    execute_parallel, Admission, Gate, GateKind, GateOutcome, Node, NodeOutcome,
    ParallelWorkflowHost, RunOutcome, Workflow,
};

use crate::m4::Emitter;

pub const DEFAULT_PARALLEL_WORKERS: usize = 4;

fn parallel_workers() -> usize {
    std::env::var("STUDIO_PARALLEL")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|n| (1..=16).contains(n))
        .unwrap_or(DEFAULT_PARALLEL_WORKERS)
}

pub struct Host<'a> {
    pub em: &'a Emitter,
    pub budget: Mutex<Enforcer>,
    pub driver: Option<ProfileDriver>,
    pub paths: ProjectPaths,
    pub brief: String,
    pub seq: AtomicUsize,
    pub auto_approve: bool,
    pub plan: Option<studio_workflow::Plan>,
    pub last_verify: Mutex<Option<studio_verify::VerifyResult>>,
    pub warmed: Mutex<BTreeSet<String>>,
    pub ask_above: Option<u64>,
    pub next_ask_at: Mutex<u64>,
    pub spent_usd: Mutex<f64>,
    pub engine_hint: String,
}

impl<'a> Host<'a> {
    fn scope_of(gate: &Gate) -> Option<VerifyScope> {
        let key = gate.scope.as_deref()?;
        VerifyScope::ALL.into_iter().find(|s| s.key() == key)
    }

    fn acting_hint(&self, r: &studio_agents::Role) -> String {
        if !acts(r) {
            return String::new();
        }
        format!(
            "\n\nYou are working in the project at {}. Create or edit the files this \
             node needs, using paths relative to that directory. Do not stop at \
             describing the work; the next node reads your files, not your prose. \
             Other crew members may be editing different files at the same time, so \
             touch only the files your own brief needs.{}",
            self.paths.project.display(),
            self.engine_hint
        )
    }
}

fn acts(r: &studio_agents::Role) -> bool {
    !r.tools().is_empty()
}

impl<'a> Host<'a> {
    fn spend_approved(&self, node: &Node) -> Result<(), String> {
        let step = match self.ask_above {
            Some(step) if step > 0 => step,
            _ => return Ok(()),
        };

        let spent = self.budget.lock().unwrap().task.spent;
        if spent < *self.next_ask_at.lock().unwrap() {
            return Ok(());
        }

        let approval_id = crate::id("ask");
        let usd = *self.spent_usd.lock().unwrap();
        println!(
            "  spend check: {spent} billed tokens (~${usd:.2}); waiting for you on the floor"
        );

        let rx = self.em.state.await_approval(&approval_id);
        let _ = self.em.emit(
            "daemon",
            EventType::BudgetApprovalNeeded,
            Scene::daemon(),
            serde_json::json!({
                "approval_id": approval_id,
                "spent": spent,
                "threshold": *self.next_ask_at.lock().unwrap(),
                "node": node.id,
                "usd": usd,
            }),
        );

        match rx.recv() {
            Ok(true) => {
                let next = spent + step;
                *self.next_ask_at.lock().unwrap() = next;
                println!("  spend approved; next check at {next} tokens");
                Ok(())
            }
            Ok(false) => Err(format!("you stopped the run at {spent} billed tokens")),
            Err(_) => Err("the floor went away while the run waited for approval".into()),
        }
    }
}

impl<'a> ParallelWorkflowHost for Host<'a> {
    fn admit(&self, node: &Node) -> Admission {
        if let Err(reason) = self.spend_approved(node) {
            return Admission::Refuse { reason };
        }

        let prefix_tokens = match role(&node.role) {
            Some(r) => crate::m4::prefix_tokens_for(r, false),
            None => 8_000,
        };
        let brief_tokens = studio_context::estimate_tokens(&self.brief) as u64
            + self
                .plan
                .as_ref()
                .and_then(|p| p.brief_for(&node.id))
                .map(|b| studio_context::estimate_tokens(b) as u64)
                .unwrap_or(0);
        let projection = Projection {
            prefix_tokens,
            brief_tokens,
            output_reserve: 2_000,
            prefix_is_warm: self.warmed.lock().unwrap().contains(&node.role),
        };
        match self.budget.lock().unwrap().admit(projection) {
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

    fn enter(&self, node: &Node, inputs: &[String]) -> NodeOutcome {
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

        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let upstream = if inputs.is_empty() {
            String::new()
        } else {
            format!("\n\nUpstream capsules: {}", inputs.join(", "))
        };
        let brief = match self.plan.as_ref().and_then(|p| p.brief_for(&node.id)) {
            Some(planned) => format!("{planned}{upstream}{}", self.acting_hint(r)),
            None => format!(
                "Workflow node '{}'.\n\n{}{}{}",
                node.id,
                self.brief,
                upstream,
                self.acting_hint(r)
            ),
        };

        match crate::m4::run_worker_metered_uncommitted(self.em, r, &brief, seq, acts(r)) {
            Ok(m) => {
                self.budget.lock().unwrap().record(m.billed_tokens);
                *self.spent_usd.lock().unwrap() += m.cost_usd;
                self.warmed.lock().unwrap().insert(node.role.clone());
                NodeOutcome::Completed { capsule: format!("cap_{}", node.id) }
            }
            Err(e) => NodeOutcome::Failed { reason: e.to_string() },
        }
    }

    fn wave_done(&self, completed: &[&Node]) {
        let entries: Vec<(&str, String)> = completed
            .iter()
            .map(|n| {
                let brief = self
                    .plan
                    .as_ref()
                    .and_then(|p| p.brief_for(&n.id))
                    .unwrap_or(&self.brief);
                (n.role.as_str(), brief.to_string())
            })
            .collect();
        crate::m4::commit_wave(self.em, &entries);
    }

    fn gate(&self, gate: &Gate, node: &Node) -> GateOutcome {
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

        let outcome = match result.verdict {
            Verdict::Pass => GateOutcome::Pass,
            Verdict::Fail => GateOutcome::Fail { failures: result.failures.len() },
            Verdict::Inconclusive => GateOutcome::Inconclusive {
                reason: result
                    .inconclusive_reason
                    .clone()
                    .unwrap_or_else(|| "verification was inconclusive".into()),
            },
        };
        *self.last_verify.lock().unwrap() = Some(result);
        outcome
    }

    fn repair(&self, node: &Node, gate: &Gate, round: u32) -> GateOutcome {
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

        if self.driver.is_none() {
            return GateOutcome::Inconclusive { reason: "no engine bound".into() };
        }
        if Self::scope_of(gate).is_none() {
            return GateOutcome::Inconclusive { reason: "no scope".into() };
        }

        let failures = match self.last_verify.lock().unwrap().take() {
            Some(v) => v,
            None => return GateOutcome::Inconclusive { reason: "no verify result to repair".into() },
        };
        if failures.verdict == Verdict::Pass {
            return GateOutcome::Pass;
        }

        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let brief = format!(
            "The project at {} failed verification.\n\n{}\n\
             Fix exactly what the list names and nothing else.",
            self.paths.project.display(),
            failures.brief_for_worker()
        );

        match crate::m4::run_worker_metered(self.em, r, &brief, seq, true) {
            Ok(m) => {
                self.budget.lock().unwrap().record(m.billed_tokens);
                *self.spent_usd.lock().unwrap() += m.cost_usd;
            }
            Err(e) => {
                return GateOutcome::Inconclusive {
                    reason: format!("the repair worker could not run: {e}"),
                }
            }
        }

        self.gate(gate, node)
    }

    fn skip(&self, node: &Node) {
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
    ask_above: Option<u64>,
) -> Result<RunOutcome> {
    run_planned(em, workflow, brief, project, seq, None, ask_above)
}

pub fn run_planned(
    em: &Emitter,
    workflow: &Workflow,
    brief: &str,
    project: Option<PathBuf>,
    seq: &mut usize,
    plan: Option<studio_workflow::Plan>,
    ask_above: Option<u64>,
) -> Result<RunOutcome> {
    let base_sha = project
        .as_deref()
        .and_then(studio_core::git::head_sha);
    if let Some(sha) = &base_sha {
        println!("  base commit {sha}; a bad run can be reverted from the floor");
    }

    em.emit(
        "daemon",
        EventType::WorkflowStarted,
        Scene::daemon(),
        serde_json::json!({
            "workflow": workflow.id,
            "title": workflow.title,
            "nodes": workflow.nodes.iter().map(|n| &n.id).collect::<Vec<_>>(),
            "budget_tokens": workflow.total_budget(),
            "base_sha": base_sha,
        }),
    )?;

    let (driver, paths, profile) = match project {
        Some(root) => {
            let profiles = EngineProfile::builtin();
            let profile = studio_engine::detect(&root, &profiles)
                .first()
                .and_then(|d| profiles.iter().find(|p| p.id == d.id))
                .cloned();
            let driver = profile.as_ref().and_then(|p| {
                if let Err(e) = studio_engine::install_helpers(p, &root) {
                    println!("  helper install failed for {}: {e}", p.id);
                }
                ProfileDriver::resolve(p.clone()).ok()
            });
            let out = root.join(".studio-out");
            (driver, ProjectPaths::new(root, out), profile)
        }
        None => anyhow::bail!(
            "no project selected; create or pick one on the floor before starting work"
        ),
    };

    if profile.is_some() {
        match crate::skills::ensure_img2threejs(&paths.project) {
            Ok(true) => println!(
                "  img2threejs {} installed for the art crew",
                crate::skills::IMG2THREEJS_TAG
            ),
            Ok(false) => {}
            Err(e) => println!("  img2threejs install failed: {e}"),
        }
    }

    let engine_hint = match profile.as_ref().map(|p| p.id.as_str()) {
        Some("web") => {
            "\n\nThe project is pure JavaScript with three.js: index.html loads ES modules \
             directly and 'three' resolves through the import map; never add a bundler or \
             npm dependency. Any 3D model that comes from a reference image is built with \
             the img2threejs skill (v1.3+, at .claude/skills/img2threejs), and generated \
             model factories live in src/models/. All sound is procedural through \
             vendor/sfx.js; never add binary audio. To see the running game, \
             `node tools/screenshot.mjs . .studio-out/shots/latest.png` writes a capture; \
             read that image and compare it against design/spec.md before calling visual \
             work done."
        }
        Some("python") => {
            "\n\nThe project is pure Python 3.10+ with main.py as the entry point. Stick \
             to the standard library unless pygame is already importable; tkinter is the \
             windowing fallback. Every file must survive python -m compileall, and \
             main.py must survive six seconds of runtime without raising."
        }
        Some("godot") | Some("unity") | Some("ue5") => {
            "\n\n3D models that come from a reference image are built with the img2threejs \
             skill (v1.3+, at .claude/skills/img2threejs) as procedural three.js factories, \
             then exported for the engine with `node tools/model_export.mjs <factory.mjs> \
             assets/models/<name>.glb`. The engine imports the .glb; never hand-author mesh \
             data. Keep factories texture-free: solid albedo and PBR scalars survive the \
             export, canvas textures do not. To capture gameplay frames for review, run \
             the engine binary from the GODOT_BIN environment variable with \
             `--path . --write-movie .studio-out/shots/shot.png --quit-after 60`."
        }
        _ => "",
    }
    .to_string();

    let host = Host {
        em,
        budget: Mutex::new(Enforcer::new(u64::MAX, u64::MAX)),
        driver,
        paths,
        brief: brief.to_string(),
        seq: AtomicUsize::new(*seq),
        auto_approve: true,
        plan: plan.clone(),
        last_verify: Mutex::new(None),
        warmed: Mutex::new(BTreeSet::new()),
        ask_above,
        next_ask_at: Mutex::new(ask_above.unwrap_or(u64::MAX)),
        spent_usd: Mutex::new(0.0),
        engine_hint,
    };

    let width = parallel_workers();
    println!("  running up to {width} workers in parallel");
    let report = execute_parallel(workflow, &host, &BTreeSet::new(), width)
        .map_err(|e| anyhow::anyhow!("workflow failed to execute: {e}"))?;
    *seq = host.seq.load(Ordering::SeqCst);

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
        "  workflow {} -> {} ({} of {} nodes, {} gates passed, {} repair rounds)",
        workflow.id,
        outcome.tag(),
        report.entered.len(),
        workflow.nodes.len(),
        report.gates_passed,
        report.repair_rounds
    );

    match &outcome {
        RunOutcome::Blocked { node, reason }
        | RunOutcome::Escalated { node, reason }
        | RunOutcome::RoutedToInfra { node, reason }
        | RunOutcome::Refused { node, reason } => {
            println!("    stopped at {node}: {reason}");
        }
        RunOutcome::Completed => {}
    }

    Ok(outcome)
}
