use anyhow::Result;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use studio_agents::role;
use studio_budget::{Enforcer, Projection};
use studio_engine::{EngineProfile, VerifyScope};
use studio_events::{EventType, Scene};
use studio_server::Waited;
use studio_verify::{EngineDriver, ProfileDriver, ProjectPaths, Verdict};
use studio_workflow::{
    execute_parallel, Admission, Gate, GateKind, GateOutcome, Node, NodeOutcome,
    ParallelWorkflowHost, RunOutcome, WaveVerdict, Workflow,
};

use crate::m4::Emitter;

pub const DEFAULT_PARALLEL_WORKERS: usize = 4;
pub const MAX_STEP_REDOS: usize = 3;

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
    pub warmed: Mutex<std::collections::BTreeMap<String, std::time::Instant>>,
    pub ask_above: Option<u64>,
    pub next_ask_at: Mutex<u64>,
    pub spent_usd: Mutex<f64>,
    pub engine_hint: String,
    pub step_confirm: bool,
    pub notes: Mutex<Vec<String>>,
    pub tiers_done: AtomicUsize,
    pub redos_at_step: AtomicUsize,
    pub unfinished: Option<Mutex<studio_server::resume::Unfinished>>,
    pub ceiling_usd: f64,
    pub nodes_charged: AtomicUsize,
    pub held: Mutex<std::collections::BTreeMap<String, u64>>,
}

pub const DEFAULT_RUN_CEILING_USD: f64 = 25.0;
const CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(60 * 60);

pub fn ceiling_for(studio_dir: &std::path::Path, workflow: &Workflow) -> (u64, f64) {
    let stored = studio_settings::Settings::load(&studio_settings::Settings::path_in(studio_dir))
        .unwrap_or_default();

    let tokens = match stored.number("budget.tokens", 0.0) {
        n if n > 0.0 => n as u64,
        _ => workflow.total_budget().max(1),
    };
    let usd = match stored.number("budget.usd", -1.0) {
        n if n >= 0.0 => n,
        _ => DEFAULT_RUN_CEILING_USD,
    };
    (tokens, usd)
}

fn over_the_token_ceiling(tightest: studio_budget::Budget) -> Option<String> {
    if tightest.remaining() > 0 {
        return None;
    }
    Some(format!(
        "this run has spent {} of its {} billed tokens; raise budget.tokens in settings and pick \
         the run up again",
        tightest.spent, tightest.limit
    ))
}

pub fn capsule_name(node_id: &str) -> String {
    format!("cap_{node_id}")
}

pub fn redos_left(used: usize) -> usize {
    MAX_STEP_REDOS.saturating_sub(used)
}

pub fn node_brief(
    planned: Option<&str>,
    run_brief: &str,
    node_id: &str,
    upstream: &[String],
    notes: &[String],
    hint: &str,
) -> String {
    let head = match planned {
        Some(planned) => planned.to_string(),
        None => format!("Workflow node '{node_id}'.\n\n{run_brief}"),
    };

    let upstream = if upstream.is_empty() {
        String::new()
    } else {
        format!("\n\nUpstream capsules: {}", upstream.join(", "))
    };

    let steer = if notes.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nThe human steering this run has asked for this, and it outranks the \
             brief above where they disagree:\n{}",
            notes
                .iter()
                .map(|n| format!("- {n}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };

    format!("{head}{upstream}{steer}{hint}")
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

pub fn tier_title(says: &[String]) -> String {
    match says {
        [] => "the crew has nothing to show".to_string(),
        [only] => only.clone(),
        [first, rest @ ..] => format!("{first} (and {} more)", rest.len()),
    }
}

pub fn tier_summary(lines: &[(String, String)]) -> String {
    lines
        .iter()
        .map(|(role, say)| format!("{role}: {say}"))
        .collect::<Vec<_>>()
        .join("\n")
}

impl<'a> Host<'a> {
    fn record_progress(&self, completed: &[&Node]) {
        let Some(held) = &self.unfinished else {
            return;
        };
        let Ok(mut held) = held.lock() else {
            return;
        };
        for node in completed {
            if !held.done.contains(&node.id) {
                held.done.push(node.id.clone());
            }
        }
        held.left_at = crate::now();
        if let Err(e) = studio_server::resume::write(&self.em.state.studio_dir, &held) {
            println!("  the run could not record where it got to: {e}");
        }
    }

    fn say_for(&self, id: &str) -> String {
        self.plan
            .as_ref()
            .and_then(|p| p.say_for(id))
            .unwrap_or_else(|| id.to_string())
    }

    fn step_title(&self, completed: &[&Node]) -> String {
        let says: Vec<String> = completed.iter().map(|n| self.say_for(&n.id)).collect();
        tier_title(&says)
    }

    fn step_summary(&self, completed: &[&Node]) -> String {
        let lines: Vec<(String, String)> = completed
            .iter()
            .map(|n| (n.role.clone(), self.say_for(&n.id)))
            .collect();
        tier_summary(&lines)
    }

    fn rearmed_helpers(&self, profile: &EngineProfile) -> Option<String> {
        let rewritten = studio_engine::restore_helpers(profile, &self.paths.project).ok()?;
        let forged = rewritten
            .into_iter()
            .find(|p| studio_verify::driver::invokes_studio_helper(&p.to_string_lossy()))?;
        Some(
            forged
                .strip_prefix(&self.paths.project)
                .unwrap_or(&forged)
                .to_string_lossy()
                .into_owned(),
        )
    }

    fn applied_step(&self) -> Option<studio_budget::Step> {
        self.budget.lock().unwrap_or_else(|held| held.into_inner()).applied
    }

    fn prefix_of(&self, node: &Node) -> Option<(String, u64)> {
        let r = role(&node.role)?;
        crate::m4::prefix_identity(self.em, r, acts(r), self.applied_step())
    }

    fn prefix_is_warm(&self, prefix_hash: &str) -> bool {
        self.warmed
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .get(prefix_hash)
            .is_some_and(|written| written.elapsed() < CACHE_TTL)
    }

    fn mark_warm(&self, prefix_hash: String) {
        self.warmed
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .insert(prefix_hash, std::time::Instant::now());
    }

    fn charge(&self, billed_tokens: u64, cost_usd: f64) {
        if billed_tokens == 0 && cost_usd == 0.0 {
            return;
        }
        self.budget.lock().unwrap_or_else(|held| held.into_inner()).record(billed_tokens);
        *self.spent_usd.lock().unwrap_or_else(|held| held.into_inner()) += cost_usd;
        self.nodes_charged.fetch_add(1, Ordering::Relaxed);
    }

    fn what_a_node_has_been_costing(&self) -> u64 {
        let charged = self.nodes_charged.load(Ordering::Relaxed) as u64;
        if charged == 0 {
            return 0;
        }
        let spent = self.budget.lock().unwrap_or_else(|held| held.into_inner()).task.spent;
        spent / charged
    }

    fn reserve_for(&self, node: &Node) -> u64 {
        node.budget_tokens.max(self.what_a_node_has_been_costing())
    }

    fn hold_for(&self, node: &Node, tokens: u64) {
        self.held
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .insert(node.id.clone(), tokens);
    }

    fn release_hold(&self, node: &Node) {
        let held = self
            .held
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .remove(&node.id);
        if let Some(tokens) = held {
            self.budget
                .lock()
                .unwrap_or_else(|held| held.into_inner())
                .release(tokens);
        }
    }

    fn spend_approved(&self, node: &Node) -> Result<(), String> {
        let step = match self.ask_above {
            Some(step) if step > 0 => step,
            _ => return Ok(()),
        };

        let spent = self.budget.lock().unwrap_or_else(|held| held.into_inner()).task.spent;
        if spent < *self.next_ask_at.lock().unwrap_or_else(|held| held.into_inner()) {
            return Ok(());
        }

        let approval_id = crate::id("ask");
        let usd = *self.spent_usd.lock().unwrap_or_else(|held| held.into_inner());
        println!(
            "  spend check: {spent} billed tokens (~${usd:.2}); waiting for you on the floor"
        );

        let rx = self.em.state.await_approval(&approval_id);
        let ask = serde_json::json!({
            "approval_id": approval_id,
            "spent": spent,
            "threshold": *self.next_ask_at.lock().unwrap_or_else(|held| held.into_inner()),
            "node": node.id,
            "usd": usd,
        });
        let announce = || {
            let _ = self.em.emit(
                "daemon",
                EventType::BudgetApprovalNeeded,
                Scene::daemon(),
                ask.clone(),
            );
        };
        announce();

        match self.em.state.wait_for(&rx, announce) {
            Waited::Answered(true) => {
                let next = spent + step;
                *self.next_ask_at.lock().unwrap_or_else(|held| held.into_inner()) = next;
                println!("  spend approved; next check at {next} tokens");
                Ok(())
            }
            Waited::Answered(false) => Err(format!("you stopped the run at {spent} billed tokens")),
            Waited::Stopped => Err(format!("you stopped the run at {spent} billed tokens")),
            Waited::Gone => Err("the floor went away while the run waited for approval".into()),
        }
    }
}

impl<'a> ParallelWorkflowHost for Host<'a> {
    fn cold_prefix_of(&self, node: &Node) -> Option<String> {
        let (hash, _) = self.prefix_of(node)?;
        if self.prefix_is_warm(&hash) {
            return None;
        }
        Some(hash)
    }

    fn admit(&self, node: &Node) -> Admission {
        let tightest = self
            .budget
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .tightest()
            .1;
        if let Some(reason) = over_the_token_ceiling(tightest) {
            let _ = self.em.emit(
                "daemon",
                EventType::BudgetExhausted,
                Scene::daemon(),
                serde_json::json!({
                    "scope": "run",
                    "reason": reason,
                    "node": node.id,
                    "spent": tightest.spent,
                    "ceiling": tightest.limit,
                }),
            );
            return Admission::Refuse { reason };
        }

        if let Err(reason) = self.spend_approved(node) {
            return Admission::Refuse { reason };
        }

        let spent = *self.spent_usd.lock().unwrap_or_else(|held| held.into_inner());
        if self.ceiling_usd > 0.0 && spent >= self.ceiling_usd {
            let reason = format!(
                "this run has spent ${spent:.2} and its ceiling is ${:.2}; raise budget.usd in \
                 settings and pick the run up again",
                self.ceiling_usd
            );
            let _ = self.em.emit(
                "daemon",
                EventType::BudgetExhausted,
                Scene::daemon(),
                serde_json::json!({
                    "scope": "run",
                    "reason": reason,
                    "node": node.id,
                    "spent_usd": spent,
                    "ceiling_usd": self.ceiling_usd,
                }),
            );
            return Admission::Refuse { reason };
        }

        let prefix = self.prefix_of(node);
        let prefix_tokens = prefix.as_ref().map(|(_, tokens)| *tokens).unwrap_or(8_000);
        let prefix_is_warm = prefix
            .as_ref()
            .is_some_and(|(hash, _)| self.prefix_is_warm(hash));
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
            prefix_is_warm,
            node_reserve: self.reserve_for(node),
        };
        match self.budget.lock().unwrap_or_else(|held| held.into_inner()).admit(projection) {
            studio_budget::Admission::Admit => {
                self.hold_for(node, projection.total());
                Admission::Admit
            }
            studio_budget::Admission::Degrade { step, reason } => {
                self.hold_for(node, projection.total());
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
        let notes = self.notes.lock().unwrap_or_else(|held| held.into_inner()).clone();
        let brief = node_brief(
            self.plan.as_ref().and_then(|p| p.brief_for(&node.id)),
            &self.brief,
            &node.id,
            inputs,
            &notes,
            &self.acting_hint(r),
        );

        let degrade = self.applied_step();
        match crate::m4::run_worker_metered_uncommitted(self.em, r, &brief, seq, acts(r), degrade) {
            Ok(m) => {
                self.release_hold(node);
                self.charge(m.billed_tokens, m.cost_usd);
                if let Some((hash, _)) = self.prefix_of(node) {
                    self.mark_warm(hash);
                }
                NodeOutcome::Completed { capsule: format!("cap_{}", node.id) }
            }
            Err(e) => {
                self.release_hold(node);
                self.charge(e.spend.billed_tokens, e.spend.cost_usd);
                NodeOutcome::Failed { reason: e.to_string() }
            }
        }
    }

    fn nothing_further_can_run(&self, failure: &str) -> Option<String> {
        if self.em.state.stopping.load(Ordering::Relaxed) {
            return Some("you stopped the run from the floor".to_string());
        }

        let said = studio_core::account_is_out_of_allowance(failure)?;
        let reason = format!(
            "the coding CLI refused on the account's own limit, so every worker after it \
             would refuse too: {said}"
        );
        println!("  {reason}");
        println!("  nothing further is spawned; start the run again once the window resets");
        let _ = self.em.emit(
            "daemon",
            EventType::RunInterrupted,
            Scene::daemon(),
            serde_json::json!({
                "reason": "out of allowance",
                "note": said,
                "step": null,
            }),
        );
        Some(reason)
    }

    fn before_wave(&self, ready: &[&Node]) -> WaveVerdict {
        let landing = ready.first().map(|n| self.say_for(&n.id));

        for interrupt in self.em.state.take_interrupts() {
            if let Some(note) = interrupt.note.filter(|n| !n.trim().is_empty()) {
                println!("  a note landed on the next step: {note}");
                let _ = self.em.emit(
                    "daemon",
                    EventType::RunInterrupted,
                    Scene::daemon(),
                    serde_json::json!({
                        "reason": "note",
                        "note": note,
                        "step": landing,
                    }),
                );
                self.notes.lock().unwrap_or_else(|held| held.into_inner()).push(note);
            }

            if interrupt.stop {
                println!("  you stopped the run; the workers already running were killed");
                let _ = self.em.emit(
                    "daemon",
                    EventType::RunInterrupted,
                    Scene::daemon(),
                    serde_json::json!({
                        "reason": "stopped",
                        "note": null,
                        "step": landing,
                    }),
                );
                return WaveVerdict::Stop {
                    reason: "you stopped the run from the floor".to_string(),
                };
            }
        }

        WaveVerdict::Continue
    }

    fn after_wave(&self, completed: &[&Node]) -> WaveVerdict {
        self.notes.lock().unwrap_or_else(|held| held.into_inner()).clear();
        if !self.step_confirm {
            self.tiers_done.fetch_add(1, Ordering::SeqCst);
            return WaveVerdict::Continue;
        }

        let step = self.tiers_done.load(Ordering::SeqCst) + 1;
        let used = self.redos_at_step.load(Ordering::SeqCst);
        let left = redos_left(used);
        let approval_id = crate::id("step");
        let rx = self.em.state.await_step(&approval_id);

        let modes: Vec<&str> = if left == 0 {
            vec!["approve", "improve"]
        } else {
            vec!["approve", "improve", "redo"]
        };

        let ask = serde_json::json!({
            "approval_id": approval_id,
            "step": step,
            "title": self.step_title(completed),
            "summary": self.step_summary(completed),
            "modes": modes,
            "redos_used": used,
            "redos_left": left,
        });
        let announce = || {
            let _ = self.em.emit(
                "daemon",
                EventType::StepApprovalNeeded,
                Scene::daemon(),
                ask.clone(),
            );
        };
        announce();
        println!("  step {step} is done and waiting for you on the floor");
        if used > 0 {
            println!("  step {step} has been sent back {used} time(s); {left} left before the run stops");
        }

        match self.em.state.wait_for(&rx, announce) {
            Waited::Answered(verdict) if verdict.approve => {
                self.tiers_done.fetch_add(1, Ordering::SeqCst);
                self.redos_at_step.store(0, Ordering::SeqCst);
                if let Some(note) = verdict.note.filter(|n| !n.trim().is_empty()) {
                    println!("  step {step} approved; the next step is briefed with: {note}");
                    self.notes.lock().unwrap_or_else(|held| held.into_inner()).push(note);
                } else {
                    println!("  step {step} approved");
                }
                WaveVerdict::Continue
            }
            Waited::Answered(_) if left == 0 => {
                println!(
                    "  step {step} has already been run {} times and is still not right; stopping",
                    used + 1
                );
                WaveVerdict::Stop {
                    reason: format!(
                        "step {step} was sent back {MAX_STEP_REDOS} times and is still not right; \
                         the run stopped rather than spending more on the same step"
                    ),
                }
            }
            Waited::Answered(verdict) => {
                let note = verdict
                    .note
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or_else(|| "Do this step again and make it better.".to_string());
                self.redos_at_step.fetch_add(1, Ordering::SeqCst);
                println!("  step {step} sent back: {note}");
                self.notes.lock().unwrap_or_else(|held| held.into_inner()).push(note);
                WaveVerdict::Redo
            }
            Waited::Stopped => WaveVerdict::Stop {
                reason: format!("you stopped the run while step {step} waited for you"),
            },
            Waited::Gone => WaveVerdict::Stop {
                reason: "the floor went away while the run waited on a step".to_string(),
            },
        }
    }

    fn capsule_of(&self, node_id: &str) -> Option<String> {
        Some(capsule_name(node_id))
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

    fn wave_committed(&self, completed: &[&Node]) {
        self.record_progress(completed);
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

        if let Some(rewritten) = self.rearmed_helpers(&driver.profile) {
            let failure = studio_verify::Failure {
                id: "studio_ci".into(),
                kind: studio_verify::FailureKind::Compile,
                symbol: None,
                file: Some(rewritten.clone()),
                line: None,
                message: format!(
                    "{rewritten} is the studio's own check and it had been changed. It has been \
                     restored from the shipped copy. Leave it alone and fix the project instead."
                ),
                detail: None,
            };
            let _ = self.em.emit(
                "daemon",
                EventType::GateEvaluated,
                Scene::daemon(),
                serde_json::json!({
                    "gate": node.id,
                    "kind": "verify",
                    "passed": false,
                    "reason": "the check the gate runs had been rewritten",
                }),
            );
            *self.last_verify.lock().unwrap_or_else(|held| held.into_inner()) =
                Some(studio_verify::VerifyResult {
                    verdict: Verdict::Fail,
                    failures: vec![failure],
                    scope,
                    engine: driver.profile.id.clone(),
                    duration_ms: 0,
                    raw_report_path: None,
                    inconclusive_reason: None,
                });
            return GateOutcome::Fail { failures: 1 };
        }

        let _ = self.em.emit(
            "daemon",
            EventType::VerifyStarted,
            Scene::daemon(),
            serde_json::json!({"scope": scope.key(), "engine": driver.profile.id}),
        );

        let mut result = driver.verify(scope, &self.paths);

        let unrigged = crate::assets::rig_failures(&self.em.state.studio_dir, &self.paths.project);
        if !unrigged.is_empty() {
            result.verdict = Verdict::Fail;
            result.failures.extend(unrigged);
        }

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
        *self.last_verify.lock().unwrap_or_else(|held| held.into_inner()) = Some(result);
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

        let failures = match self.last_verify.lock().unwrap_or_else(|held| held.into_inner()).take() {
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

        match crate::m4::run_worker_metered(self.em, r, &brief, seq, true, self.applied_step()) {
            Ok(m) => self.charge(m.billed_tokens, m.cost_usd),
            Err(e) => {
                self.charge(e.spend.billed_tokens, e.spend.cost_usd);
                return GateOutcome::Inconclusive {
                    reason: format!("the repair worker could not run: {e}"),
                };
            }
        }

        self.gate(gate, node)
    }

    fn skip(&self, node: &Node) {
        self.release_hold(node);
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
    run_planned(em, workflow, brief, project, seq, None, ask_above, false, None)
}

#[allow(clippy::too_many_arguments)]
pub fn run_planned(
    em: &Emitter,
    workflow: &Workflow,
    brief: &str,
    project: Option<PathBuf>,
    seq: &mut usize,
    plan: Option<studio_workflow::Plan>,
    ask_above: Option<u64>,
    step_confirm: bool,
    resuming: Option<studio_server::resume::Unfinished>,
) -> Result<RunOutcome> {
    let base_sha = project
        .as_deref()
        .and_then(studio_core::git::head_sha);
    if let Some(sha) = &base_sha {
        println!("  base commit {sha}; a bad run can be reverted from the floor");
    }

    let already_done: BTreeSet<String> =
        resuming.as_ref().map(|r| r.done_set()).unwrap_or_default();
    if !already_done.is_empty() {
        println!(
            "  resuming: {} step(s) already done, {} left",
            already_done.len(),
            workflow.nodes.len().saturating_sub(already_done.len())
        );
    }

    let unfinished = match (&resuming, &plan, &em.project_id) {
        (Some(held), _, _) => Some(held.clone()),
        (None, Some(plan), Some(id)) => Some(studio_server::resume::Unfinished {
            project: id.clone(),
            title: plan.title.clone(),
            brief: brief.to_string(),
            plan: plan.clone(),
            done: Vec::new(),
            left_at: crate::now(),
            why: String::new(),
        }),
        _ => None,
    };
    if let Some(held) = &unfinished {
        if let Err(e) = studio_server::resume::write(&em.state.studio_dir, held) {
            println!("  this run cannot be resumed if it stops: {e}");
        }
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
        crate::assets::announce(&em.state.studio_dir, &paths.project);
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

    let engine_hint = format!(
        "{engine_hint}{}",
        crate::assets::crew_hint(&em.state.studio_dir, &paths.project)
    );

    let (ceiling_tokens, ceiling_usd) = ceiling_for(&em.state.studio_dir, workflow);
    let host = Host {
        em,
        budget: Mutex::new(Enforcer::new(ceiling_tokens, ceiling_tokens)),
        driver,
        paths,
        brief: brief.to_string(),
        seq: AtomicUsize::new(*seq),
        auto_approve: true,
        plan: plan.clone(),
        last_verify: Mutex::new(None),
        warmed: Mutex::new(std::collections::BTreeMap::new()),
        ask_above,
        next_ask_at: Mutex::new(ask_above.unwrap_or(u64::MAX)),
        spent_usd: Mutex::new(0.0),
        engine_hint,
        step_confirm,
        notes: Mutex::new(Vec::new()),
        tiers_done: AtomicUsize::new(0),
        redos_at_step: AtomicUsize::new(0),
        unfinished: unfinished.map(Mutex::new),
        ceiling_usd,
        nodes_charged: AtomicUsize::new(0),
        held: Mutex::new(std::collections::BTreeMap::new()),
    };

    if step_confirm {
        println!("  step confirmation is on; the run holds after every step");
    }
    em.state.take_interrupts();
    em.state.nothing_is_being_stopped();

    let width = parallel_workers();
    println!("  running up to {width} workers in parallel");
    println!(
        "  ceiling: {ceiling_tokens} billed tokens{}",
        if ceiling_usd > 0.0 {
            format!(" or ${ceiling_usd:.2}, whichever comes first")
        } else {
            ", and no dollar ceiling".to_string()
        }
    );
    let report = execute_parallel(workflow, &host, &already_done, width)
        .map_err(|e| anyhow::anyhow!("workflow failed to execute: {e}"))?;
    *seq = host.seq.load(Ordering::SeqCst);

    for missed in em.state.take_interrupts() {
        let note = missed.note.filter(|n| !n.trim().is_empty());
        println!(
            "  an interrupt arrived after the last step and had nowhere to land{}",
            note.as_deref().map(|n| format!(": {n}")).unwrap_or_default()
        );
        em.emit(
            "daemon",
            EventType::RunInterrupted,
            Scene::daemon(),
            serde_json::json!({
                "reason": "too late; the run had already finished",
                "note": note,
                "step": null,
            }),
        )?;
    }

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
            "redo_rounds": report.redo_rounds,
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

    if report.redo_rounds > 0 {
        println!("    {} step(s) were sent back and run again", report.redo_rounds);
    }

    match &outcome {
        RunOutcome::Blocked { node, reason }
        | RunOutcome::Escalated { node, reason }
        | RunOutcome::RoutedToInfra { node, reason }
        | RunOutcome::Refused { node, reason }
        | RunOutcome::Interrupted { node, reason }
        | RunOutcome::Halted { node, reason } => {
            println!("    stopped at {node}: {reason}");
        }
        RunOutcome::Completed => {}
    }

    settle_the_resume_point(&host, &outcome);
    Ok(outcome)
}

fn settle_the_resume_point(host: &Host, outcome: &RunOutcome) {
    let Some(held) = &host.unfinished else {
        return;
    };
    let Ok(mut held) = held.lock() else {
        return;
    };

    let studio_dir = &host.em.state.studio_dir;
    if held.finished() || outcome.is_clean() {
        studio_server::resume::clear(studio_dir, &held.project);
        return;
    }

    held.why = match outcome {
        RunOutcome::Blocked { reason, .. }
        | RunOutcome::Escalated { reason, .. }
        | RunOutcome::RoutedToInfra { reason, .. }
        | RunOutcome::Refused { reason, .. }
        | RunOutcome::Interrupted { reason, .. }
        | RunOutcome::Halted { reason, .. } => reason.clone(),
        RunOutcome::Completed => String::new(),
    };
    held.left_at = crate::now();
    let _ = studio_server::resume::write(studio_dir, &held);

    println!(
        "    {} step(s) never ran; the floor can pick this run up where it stopped",
        held.left().len()
    );
}

#[cfg(test)]
mod redo_cap_tests {
    use super::{redos_left, MAX_STEP_REDOS};

    #[test]
    fn a_step_nobody_has_sent_back_offers_every_redo_the_cap_allows() {
        assert_eq!(redos_left(0), MAX_STEP_REDOS);
    }

    #[test]
    fn each_send_back_spends_one_of_the_allowance() {
        assert_eq!(redos_left(1), MAX_STEP_REDOS - 1);
        assert_eq!(redos_left(MAX_STEP_REDOS - 1), 1);
    }

    #[test]
    fn the_allowance_runs_out_rather_than_going_negative() {
        assert_eq!(redos_left(MAX_STEP_REDOS), 0);
        assert_eq!(
            redos_left(MAX_STEP_REDOS + 9),
            0,
            "a saturating count is what stops the run instead of wrapping into a fresh allowance"
        );
    }

    #[test]
    fn the_cap_leaves_room_to_improve_a_step_more_than_once_before_giving_up() {
        assert!(
            MAX_STEP_REDOS >= 2,
            "one rejection is a typo, not a pattern; the cap must not stop a run on the first"
        );
    }
}

#[cfg(test)]
mod guided_tests {
    use super::{node_brief, tier_summary, tier_title};

    fn says(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_step_with_no_notes_is_briefed_exactly_as_it_was_planned() {
        let brief = node_brief(Some("Draw the bird."), "build flappy", "t1", &[], &[], "");
        assert_eq!(brief, "Draw the bird.");
    }

    #[test]
    fn a_step_sent_back_is_briefed_with_the_notes_that_say_why() {
        let brief = node_brief(
            Some("Draw the bird."),
            "build flappy",
            "t1",
            &[],
            &says(&["the bird reads as a blob at this size"]),
            "",
        );
        assert!(brief.starts_with("Draw the bird."), "the plan still leads the brief");
        assert!(brief.contains("- the bird reads as a blob at this size"));
        assert!(
            brief.contains("outranks the brief above"),
            "a worker that treats the note as optional will hand back the same thing"
        );
    }

    #[test]
    fn several_notes_all_reach_the_crew() {
        let brief = node_brief(
            Some("Draw the bird."),
            "b",
            "t1",
            &[],
            &says(&["make it a paper plane", "the pipes should be green"]),
            "",
        );
        assert!(brief.contains("- make it a paper plane"));
        assert!(brief.contains("- the pipes should be green"));
    }

    #[test]
    fn notes_sit_between_the_plan_and_the_working_directory_hint() {
        let brief = node_brief(
            Some("Draw the bird."),
            "b",
            "t1",
            &["cap_t0".to_string()],
            &says(&["make it a paper plane"]),
            "\n\nYou are working in the project at /games/flappy.",
        );
        let note_at = brief.find("paper plane").unwrap();
        let upstream_at = brief.find("cap_t0").unwrap();
        let hint_at = brief.find("You are working in").unwrap();
        assert!(upstream_at < note_at && note_at < hint_at);
    }

    #[test]
    fn a_workflow_node_with_no_plan_still_carries_the_run_brief() {
        let brief = node_brief(None, "build flappy", "implement", &[], &[], "");
        assert!(brief.contains("implement"));
        assert!(brief.contains("build flappy"));
    }

    #[test]
    fn a_single_step_tier_is_titled_with_what_the_crew_just_did() {
        assert_eq!(tier_title(&says(&["Draw the player"])), "Draw the player");
    }

    #[test]
    fn a_tier_that_ran_three_things_at_once_says_so_instead_of_naming_one() {
        let title = tier_title(&says(&["Draw the player", "Write the score", "Add the flap"]));
        assert_eq!(title, "Draw the player (and 2 more)");
    }

    #[test]
    fn the_step_summary_names_who_did_what() {
        let summary = tier_summary(&[
            ("artist".to_string(), "Draw the player".to_string()),
            ("gameplay_engineer".to_string(), "Add the flap".to_string()),
        ]);
        assert_eq!(summary, "artist: Draw the player\ngameplay_engineer: Add the flap");
    }
}

#[cfg(test)]
mod ceiling_tests {
    use super::*;
    use serde_json::Value;

    fn eleven_nodes() -> Workflow {
        let plan = studio_workflow::Plan {
            title: "t".into(),
            tasks: (1..=11)
                .map(|i| studio_workflow::PlanTask {
                    id: format!("t{i}"),
                    role: "gameplay_engineer".into(),
                    brief: "do it".into(),
                    depends_on: Vec::new(),
                    say: String::new(),
                })
                .collect(),
        };
        plan.to_workflow().unwrap()
    }

    fn dir_with(pairs: &[(&str, Value)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let mut s = studio_settings::Settings::new();
        for (k, v) in pairs {
            s.set(k, v.clone());
        }
        s.save(&studio_settings::Settings::path_in(dir.path())).unwrap();
        dir
    }

    #[test]
    fn a_run_nobody_configured_still_gets_a_ceiling() {
        let dir = tempfile::tempdir().unwrap();
        let (tokens, usd) = ceiling_for(dir.path(), &eleven_nodes());
        assert_eq!(tokens, eleven_nodes().total_budget());
        assert_eq!(usd, DEFAULT_RUN_CEILING_USD);
        assert!(
            tokens < u64::MAX && usd > 0.0,
            "the enforcer used to be built with u64::MAX, so a plan could spend without limit"
        );
    }

    #[test]
    fn the_ceiling_can_be_raised_or_turned_off_from_settings() {
        let dir = dir_with(&[
            ("budget.tokens", Value::from(50_000)),
            ("budget.usd", Value::from(4.5)),
        ]);
        assert_eq!(ceiling_for(dir.path(), &eleven_nodes()), (50_000, 4.5));

        let off = dir_with(&[("budget.usd", Value::from(0))]);
        assert_eq!(ceiling_for(off.path(), &eleven_nodes()).1, 0.0);
    }

    #[test]
    fn a_ceiling_of_zero_tokens_falls_back_rather_than_refusing_everything() {
        let dir = dir_with(&[("budget.tokens", Value::from(0))]);
        let (tokens, _) = ceiling_for(dir.path(), &eleven_nodes());
        assert_eq!(tokens, eleven_nodes().total_budget());
    }

    #[test]
    fn the_token_enforcer_refuses_a_node_that_would_cross_the_ceiling() {
        let mut e = Enforcer::new(100_000, 100_000);
        e.record(99_000);
        let projection = Projection {
            prefix_tokens: 8_000,
            brief_tokens: 500,
            output_reserve: 2_000,
            prefix_is_warm: false,
            node_reserve: 0,
        };
        assert!(
            matches!(e.admit(projection), studio_budget::Admission::Refuse { .. }),
            "the ladder the budget crate ships was unreachable while the limit was u64::MAX"
        );
    }

    #[test]
    fn a_node_reserves_the_budget_its_own_plan_gave_it_rather_than_the_size_of_its_first_prompt() {
        let workflow = eleven_nodes();
        let node = workflow.nodes.first().expect("eleven nodes is not empty");
        assert!(
            node.budget_tokens > 10_000,
            "a planned node is worth far more than an opening prompt: {}",
            node.budget_tokens
        );

        let opening = Projection {
            prefix_tokens: 1_000,
            brief_tokens: 173,
            output_reserve: 2_000,
            prefix_is_warm: true,
            node_reserve: node.budget_tokens,
        };
        assert_eq!(
            opening.total(),
            node.budget_tokens,
            "the gate has to hold room for the whole worker session, not for its first turn"
        );
    }

    #[test]
    fn a_run_that_has_already_outspent_its_ceiling_is_told_so_rather_than_asked_to_approve_more() {
        let mut spent_out = Enforcer::new(1_440_000, 1_440_000);
        spent_out.record(2_061_867);
        let reason = over_the_token_ceiling(spent_out.tightest().1)
            .expect("a run past its ceiling has nothing left to admit");
        assert!(reason.contains("2061867"), "the spend has to be named: {reason}");
        assert!(reason.contains("1440000"), "the ceiling has to be named: {reason}");
        assert!(
            reason.contains("budget.tokens"),
            "the message has to name the setting that lifts it: {reason}"
        );

        let mut healthy = Enforcer::new(1_440_000, 1_440_000);
        healthy.record(400_000);
        assert_eq!(over_the_token_ceiling(healthy.tightest().1), None);
    }

    #[test]
    fn the_ceiling_is_checked_before_the_human_is_asked_to_approve_more_spend() {
        let source = include_str!("wf.rs");
        let body = source
            .split("fn admit(&self, node: &Node) -> Admission {")
            .nth(1)
            .expect("Host::admit is where both gates live");
        let ceiling = body.find("over_the_token_ceiling").expect("the ceiling gate is gone");
        let ask = body.find("spend_approved").expect("the approval gate is gone");
        assert!(
            ceiling < ask,
            "asking first means a run whose ceiling is already spent gets a `continue?` prompt \
             that cannot lead anywhere: whatever the human answers, the very next gate refuses it"
        );
    }
}
