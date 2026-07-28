use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
use studio_agents::{nearest_common_ancestor, role};
use studio_events::{EventType, Scene};
use studio_server::{
    AppState, BuildRequest, MeetingRequest, PlanVerdict, ResumeRequest, StudioCommand,
    TaskRequest, Waited, WorkflowRequest,
};
use studio_store::Store;

use crate::m4::Emitter;

const INDEX_PATHS_SAMPLED: usize = 10;

pub struct ProjectIndex {
    index: studio_index::Index,
    root: PathBuf,
}

impl ProjectIndex {
    pub fn open(root: PathBuf, database: PathBuf) -> Result<Self> {
        let index = studio_index::Index::open(&database)?;
        Ok(Self { index, root })
    }

    pub fn refresh_quietly(&mut self, em: &Emitter) {
        if let Err(e) = self.refresh(em) {
            println!("  index refresh failed: {e}");
        }
    }

    pub fn refresh(&mut self, em: &Emitter) -> Result<()> {
        let report = self.index.scan(&self.root)?;
        if !report.touched_anything() {
            return Ok(());
        }

        let sample: Vec<&String> = report.changed_paths.iter().take(INDEX_PATHS_SAMPLED).collect();
        em.emit(
            "daemon",
            EventType::IndexUpdated,
            Scene::daemon(),
            serde_json::json!({
                "paths_changed": report.changed_paths.len(),
                "symbols_delta": report.symbols_delta,
                "paths": sample,
            }),
        )?;

        println!(
            "  index: {} path(s) changed, {:+} symbol(s)",
            report.changed_paths.len(),
            report.symbols_delta
        );
        let _ = self.index.checkpoint();
        Ok(())
    }
}

pub fn run_command(em: &Emitter, cmd: StudioCommand, seq: &mut usize) -> Result<()> {
    em.state.take_interrupts();
    em.state.nothing_is_being_stopped();

    match cmd {
        StudioCommand::Task(t) => run_task(em, t, seq),
        StudioCommand::Meeting(m) => run_meeting(em, m, seq),
        StudioCommand::Workflow(w) => run_flow(em, w, seq),
        StudioCommand::Build(b) => run_build(em, b, seq),
        StudioCommand::Summarize(s) => crate::games::summarize(em, &s, seq),
        StudioCommand::Resume(r) => run_resume(em, r, seq),
    }
}

fn run_build(em: &Emitter, req: BuildRequest, seq: &mut usize) -> Result<()> {
    println!("  build: {}", first_line(&req.prompt));

    let director = role("studio_director").context("the director is missing from the registry")?;
    let schema = studio_workflow::plan_schema().to_string();

    let engine_line = em
        .project
        .as_deref()
        .and_then(|root| {
            let profiles = studio_engine::EngineProfile::builtin();
            studio_engine::detect(root, &profiles)
                .first()
                .and_then(|d| profiles.iter().find(|p| p.id == d.id))
                .map(|p| format!("The project runs on {}. ", p.display_name))
        })
        .unwrap_or_default();

    let survey_block = em
        .project
        .as_deref()
        .and_then(crate::survey::survey)
        .map(|s| {
            format!(
                "\n\nThe project already has work in it. Survey:\n{s}\n\
                 This is an existing game: plan changes against the files above. Extend \
                 and modify what is there instead of rebuilding it, name the real files \
                 each task touches, and never plan a task that recreates something the \
                 survey already shows working.\n"
            )
        })
        .unwrap_or_default();

    let brief = format!(
        "A request has come in: {}\n{survey_block}\n\
         {engine_line}Decompose it into studio tasks. Give each task the role that should do it, \
         a brief detailed enough that the role needs no further decisions, and the ids \
         of the tasks whose output it needs. Give each task a 'say' line too: one sentence \
         a producer would use telling a colleague what this step is, naming the thing in \
         the game rather than the technique, so that 'Draw the player and the pipes it \
         flies through' reads back instead of 'implement sprite atlas'. The floor runs independent tasks in \
         parallel, so parallelize aggressively: only add depends_on when a task truly \
         reads another task's files, split large implementation work into several \
         non-overlapping tasks (the same role may appear more than once), and give \
         parallel tasks disjoint files so they never edit the same path. Keep the \
         graph as small as the work allows. Do not invent roles.",
        req.prompt.trim()
    );

    *seq += 1;
    let raw = crate::m4::run_worker_capturing(em, director, &brief, *seq, Some(schema))?;

    let cleaned = extract_json(&raw);
    let proposed = studio_workflow::Plan::parse(&cleaned)
        .map_err(|e| anyhow::anyhow!("the director returned a plan I cannot run: {e}"))?;

    println!("  plan '{}' with {} tasks:", proposed.title, proposed.tasks.len());
    for t in &proposed.tasks {
        println!("    {:<6} {:<20} {}", t.id, t.role, t.say());
    }

    let plan = match propose(em, proposed, req.guided)? {
        Some(plan) => plan,
        None => return Ok(()),
    };

    let mut wf = plan
        .to_workflow()
        .map_err(|e| anyhow::anyhow!("plan did not convert to a workflow: {e}"))?;

    if let Some(profile) = engine_for(em.project.as_deref()) {
        for scope in [studio_engine::VerifyScope::Compile, studio_engine::VerifyScope::Runtime] {
            if let Some(gate) = verify_gate(&profile, &wf, scope) {
                println!("  gate: {} after {}", scope.key(), gate.after);
                wf.gates.push(gate);
            }
        }
    }

    crate::wf::run_planned(
        em,
        &wf,
        &req.prompt,
        em.project.clone(),
        seq,
        Some(plan),
        req.ask_above,
        req.step_confirm,
        None,
    )?;
    Ok(())
}

fn run_resume(em: &Emitter, req: ResumeRequest, seq: &mut usize) -> Result<()> {
    let project = em
        .project_id
        .clone()
        .context("resuming needs a project; pick one on the floor first")?;

    let held = studio_server::resume::read(&em.state.studio_dir, &project)
        .context("there is no stopped run to pick up for this project")?;

    println!(
        "  resume '{}': {} of {} step(s) already done",
        held.title,
        held.done.len(),
        held.plan.tasks.len()
    );
    if !held.why.trim().is_empty() {
        println!("  it stopped because {}", held.why);
    }
    for id in held.left() {
        let say = held
            .plan
            .tasks
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.say())
            .unwrap_or_default();
        println!("    {id:<6} {say}");
    }

    let mut wf = held
        .plan
        .to_workflow()
        .map_err(|e| anyhow::anyhow!("the stopped run's plan will not convert: {e}"))?;

    if let Some(profile) = engine_for(em.project.as_deref()) {
        for scope in [studio_engine::VerifyScope::Compile, studio_engine::VerifyScope::Runtime] {
            if let Some(gate) = verify_gate(&profile, &wf, scope) {
                println!("  gate: {} after {}", scope.key(), gate.after);
                wf.gates.push(gate);
            }
        }
    }

    let brief = held.brief.clone();
    let plan = held.plan.clone();
    crate::wf::run_planned(
        em,
        &wf,
        &brief,
        em.project.clone(),
        seq,
        Some(plan),
        req.ask_above,
        req.step_confirm,
        Some(held),
    )?;
    Ok(())
}

fn plan_data(
    plan_id: &str,
    plan: &studio_workflow::Plan,
    editable: bool,
) -> serde_json::Value {
    serde_json::json!({
        "plan_id": plan_id,
        "title": plan.title,
        "steps": plan.steps(),
        "editable": editable,
    })
}

fn propose(
    em: &Emitter,
    plan: studio_workflow::Plan,
    guided: bool,
) -> Result<Option<studio_workflow::Plan>> {
    let plan_id = crate::id("plan");

    if !guided {
        em.emit(
            "daemon",
            EventType::PlanProposed,
            Scene::daemon(),
            plan_data(&plan_id, &plan, false),
        )?;
        return Ok(Some(plan));
    }

    let rx = em.state.await_plan(&plan_id);
    let proposal = plan_data(&plan_id, &plan, true);
    em.emit("daemon", EventType::PlanProposed, Scene::daemon(), proposal.clone())?;
    println!("  plan {plan_id} is on the floor; nothing runs until you start it");

    let announce = || {
        let _ = em.emit("daemon", EventType::PlanProposed, Scene::daemon(), proposal.clone());
    };

    let dropped = |reason: &str| -> Result<Option<studio_workflow::Plan>> {
        println!("  plan {plan_id} dropped before any worker was paid for");
        em.emit(
            "daemon",
            EventType::RunInterrupted,
            Scene::daemon(),
            serde_json::json!({
                "reason": reason,
                "note": null,
                "step": null,
            }),
        )?;
        Ok(None)
    };

    match em.state.wait_for(&rx, announce) {
        Waited::Answered(PlanVerdict::Start { steps }) if steps.is_empty() => Ok(Some(plan)),
        Waited::Answered(PlanVerdict::Start { steps }) => {
            let revised = plan
                .revise(&steps)
                .map_err(|e| anyhow::anyhow!("the plan you edited will not run: {e}"))?;
            println!("  starting the plan you edited: {} step(s)", revised.tasks.len());
            Ok(Some(revised))
        }
        Waited::Answered(PlanVerdict::Cancel) => dropped("plan dropped"),
        Waited::Stopped => dropped("you stopped the studio while the plan waited to start"),
        Waited::Gone => anyhow::bail!("the floor went away while the plan waited to start"),
    }
}

fn engine_for(project: Option<&std::path::Path>) -> Option<studio_engine::EngineProfile> {
    let root = project?;
    let profiles = studio_engine::EngineProfile::builtin();
    let detected = studio_engine::detect(root, &profiles);
    let first = detected.first()?;
    let profile = profiles.iter().find(|p| p.id == first.id)?.clone();

    match studio_engine::find_binary(&profile) {
        Some(found) => {
            println!(
                "  engine: {} at {} ({})",
                profile.id,
                found.path.display(),
                found.how
            );
            Some(profile)
        }
        None => {
            println!(
                "  gate: nothing to verify {} with; every gate is skipped this run",
                profile.id
            );
            for place in studio_engine::places_searched(&profile) {
                println!("        looked in {place}");
            }
            println!(
                "        point the studio at it under settings, or set {}",
                profile.tooling.binary_env
            );
            None
        }
    }
}

fn last_wave_node(wf: &studio_workflow::Workflow) -> Option<String> {
    let mut depth: std::collections::BTreeMap<&str, usize> =
        wf.nodes.iter().map(|n| (n.id.as_str(), 0)).collect();

    for _ in 0..wf.nodes.len() {
        let mut moved = false;
        for e in &wf.edges {
            let from = depth.get(e.from.as_str()).copied().unwrap_or(0);
            if let Some(to) = depth.get_mut(e.to.as_str()) {
                if *to < from + 1 {
                    *to = from + 1;
                    moved = true;
                }
            }
        }
        if !moved {
            break;
        }
    }

    let feeds_someone: std::collections::BTreeSet<&str> =
        wf.edges.iter().map(|e| e.from.as_str()).collect();

    wf.nodes
        .iter()
        .map(|n| n.id.as_str())
        .filter(|id| !feeds_someone.contains(id))
        .max_by_key(|id| depth.get(id).copied().unwrap_or(0))
        .map(str::to_string)
}

fn verify_gate(
    profile: &studio_engine::EngineProfile,
    wf: &studio_workflow::Workflow,
    scope: studio_engine::VerifyScope,
) -> Option<studio_workflow::Gate> {
    if profile.command(scope).is_err() {
        return None;
    }

    let after = last_wave_node(wf)?;

    Some(studio_workflow::Gate {
        after,
        kind: studio_workflow::GateKind::Verify,
        scope: Some(scope.key().to_string()),
        on_fail: studio_workflow::OnFail::Repair,
    })
}

fn run_flow(em: &Emitter, req: WorkflowRequest, seq: &mut usize) -> Result<()> {
    let wf = studio_workflow::Workflow::builtin()
        .into_iter()
        .find(|w| w.id == req.workflow)
        .with_context(|| format!("unknown workflow {}", req.workflow))?;

    println!("  workflow {} : {}", wf.id, first_line(&req.brief));
    crate::wf::run_workflow(em, &wf, &req.brief, em.project.clone(), seq, req.ask_above)?;
    Ok(())
}

fn run_task(em: &Emitter, req: TaskRequest, seq: &mut usize) -> Result<()> {
    let r = role(&req.role).with_context(|| format!("unknown role {}", req.role))?;
    *seq += 1;
    println!("  task -> {} : {}", r.id, first_line(&req.brief));

    let hands_on = !r.tools().is_empty();
    let brief = match (&em.project, hands_on) {
        (Some(root), true) => format!(
            "{}\n\nYou are working in the project at {}. Create or edit the files \
             this task needs, using paths relative to that directory.",
            req.brief,
            root.display()
        ),
        _ => req.brief.clone(),
    };

    crate::m4::run_worker_metered(em, r, &brief, *seq, hands_on, None)
        .map(|_| ())
        .map_err(|e| e.error)
}

struct Position {
    role: &'static str,
    text: String,
}

struct MeetingSpend {
    ask_above: Option<u64>,
    next_ask_at: u64,
    billed: u64,
    usd: f64,
}

impl MeetingSpend {
    fn new(ask_above: Option<u64>) -> Self {
        Self {
            ask_above,
            next_ask_at: ask_above.filter(|s| *s > 0).unwrap_or(u64::MAX),
            billed: 0,
            usd: 0.0,
        }
    }

    fn record(&mut self, m: &crate::m4::Metered) {
        self.charge(m.billed_tokens, m.cost_usd);
    }

    fn charge(&mut self, billed_tokens: u64, cost_usd: f64) {
        self.billed += billed_tokens;
        self.usd += cost_usd;
    }

    fn approved(&mut self, em: &Emitter, meeting_id: &str, speaker: &str) -> Result<(), String> {
        let step = match self.ask_above {
            Some(step) if step > 0 => step,
            _ => return Ok(()),
        };
        if self.billed < self.next_ask_at {
            return Ok(());
        }

        let approval_id = crate::id("ask");
        println!(
            "  spend check: {} billed tokens (~${:.2}); waiting for you on the floor",
            self.billed, self.usd
        );

        let rx = em.state.await_approval(&approval_id);
        let _ = em.emit(
            "daemon",
            EventType::BudgetApprovalNeeded,
            Scene::daemon().in_meeting(meeting_id),
            serde_json::json!({
                "approval_id": approval_id,
                "spent": self.billed,
                "threshold": self.next_ask_at,
                "node": speaker,
                "usd": self.usd,
            }),
        );

        match rx.recv() {
            Ok(true) => {
                self.next_ask_at = self.billed + step;
                println!("  spend approved; next check at {} tokens", self.next_ask_at);
                Ok(())
            }
            Ok(false) => Err(format!("you stopped the meeting at {} billed tokens", self.billed)),
            Err(_) => Err("the floor went away while the meeting waited for approval".into()),
        }
    }
}

fn position_brief(req: &MeetingRequest, floor: &[Position]) -> String {
    let mut s = format!("You are in a {} meeting about: {}\n\n", req.kind, req.topic.trim());

    if floor.is_empty() {
        s.push_str("You speak first, so there is nothing to answer yet.\n\n");
    } else {
        s.push_str("The room has already said this:\n\n");
        for p in floor {
            s.push_str(&format!("{}: {}\n\n", p.role, p.text));
        }
        s.push_str(
            "Answer those positions: say which one you back, and where one of them is \
             wrong, name what it got wrong.\n\n",
        );
    }

    s.push_str(
        "Now give your own position in one or two sentences. Speak for your own \
         discipline only, do not hedge, and do not restate the topic.",
    );
    s
}

fn chair_brief(req: &MeetingRequest, floor: &[Position]) -> String {
    let mut s = format!(
        "You chair this {} meeting about: {}\n\nThe room said this:\n\n",
        req.kind,
        req.topic.trim()
    );
    for p in floor {
        s.push_str(&format!("{}: {}\n\n", p.role, p.text));
    }
    s.push_str(
        "Decide. State the decision as a rule the studio follows from here on, not a \
         summary of what was said. Give the reason you chose it over the alternative the \
         room raised, and list the positions above that your decision overrules, each \
         naming the role that held it.",
    );
    s
}

fn adr_body(
    req: &MeetingRequest,
    title: &str,
    chair: &str,
    floor: &[Position],
    d: &studio_agents::Decision,
) -> String {
    let mut s = format!("# {title}\n\n");
    s.push_str(&format!("- Meeting: {}\n", req.kind));
    s.push_str(&format!("- Chair: {chair}\n"));
    s.push_str(&format!(
        "- Room: {}\n\n",
        floor.iter().map(|p| p.role).collect::<Vec<_>>().join(", ")
    ));

    s.push_str(&format!("## Decision\n\n{}\n\n", d.claim));
    s.push_str(&format!("## Why\n\n{}\n\n", d.rationale));

    if !d.dissent.is_empty() {
        s.push_str("## Overruled\n\n");
        for x in &d.dissent {
            s.push_str(&format!("- {x}\n"));
        }
        s.push('\n');
    }

    s.push_str("## The room\n\n");
    for p in floor {
        s.push_str(&format!("**{}** — {}\n\n", p.role, p.text));
    }
    s
}

fn spoke_data(meeting_id: &str, role: &str, seat: &str, position: &str) -> serde_json::Value {
    serde_json::json!({
        "meeting_id": meeting_id,
        "role": role,
        "seat": seat,
        "position": position,
    })
}

fn decision_data(
    decision_id: &str,
    meeting_id: &str,
    title: &str,
    chair: &str,
    d: &studio_agents::Decision,
    path: Option<&String>,
) -> serde_json::Value {
    serde_json::json!({
        "decision_id": decision_id,
        "meeting_id": meeting_id,
        "title": title,
        "chair": chair,
        "claim": d.claim,
        "rationale": d.rationale,
        "dissent": d.dissent,
        "path": path,
    })
}

fn record_decision(
    em: &Emitter,
    req: &MeetingRequest,
    meeting_id: &str,
    chair: &str,
    floor: &[Position],
    d: &studio_agents::Decision,
) -> Result<()> {
    let decision_id = crate::id("adr");
    let title = first_line(&req.topic);

    em.store.insert_decision(
        studio_store::DecisionRow {
            id: decision_id.clone(),
            title: title.clone(),
            claim: d.claim.clone(),
            rationale: d.rationale.clone(),
            origin_capsule: None,
            supersedes: None,
        },
        crate::now(),
    )?;

    println!("  decision {decision_id}: {}", d.claim);
    for x in &d.dissent {
        println!("    overrules: {x}");
    }

    let adr_path = match em.project.as_deref() {
        Some(root) => write_adr(root, &decision_id, req, &title, chair, floor, d)
            .map_err(|e| println!("  the decision was stored but not written to the repo: {e}"))
            .ok(),
        None => None,
    };

    em.emit(
        "daemon",
        EventType::DecisionRecorded,
        Scene::daemon().in_meeting(meeting_id),
        decision_data(&decision_id, meeting_id, &title, chair, d, adr_path.as_ref()),
    )?;

    if let (Some(root), Some(_)) = (em.project.as_deref(), &adr_path) {
        let subject = studio_core::git::subject(chair, &format!("decide {title}"));
        match studio_core::git::commit(root, &subject) {
            Ok(Some(sha)) => {
                println!("  commit {sha}  {subject}");
                em.emit(
                    "daemon",
                    EventType::CommitRecorded,
                    Scene::daemon().in_meeting(meeting_id),
                    serde_json::json!({
                        "project": root.to_string_lossy(),
                        "role": chair,
                        "sha": sha,
                        "subject": subject,
                    }),
                )?;
            }
            Ok(None) => {}
            Err(e) => println!("  commit skipped: {e}"),
        }
    }

    Ok(())
}

fn write_adr(
    root: &std::path::Path,
    decision_id: &str,
    req: &MeetingRequest,
    title: &str,
    chair: &str,
    floor: &[Position],
    d: &studio_agents::Decision,
) -> Result<String> {
    let relative = format!(
        "docs/decisions/{decision_id}-{}.md",
        studio_agents::meeting::slug(title)
    );
    let path = root.join(&relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, adr_body(req, title, chair, floor, d))?;
    println!("  wrote {relative}");
    Ok(relative)
}

fn run_meeting(em: &Emitter, req: MeetingRequest, seq: &mut usize) -> Result<()> {
    let meeting_id = crate::id("meeting");
    let chair = chair_for(&req.participants);

    println!(
        "  meeting {} ({}) chaired by {} : {}",
        meeting_id,
        req.kind,
        chair,
        first_line(&req.topic)
    );

    em.emit(
        "daemon",
        EventType::MeetingStarted,
        Scene::daemon().in_meeting(&meeting_id),
        serde_json::json!({
            "meeting_id": meeting_id,
            "kind": req.kind,
            "participants": req.participants,
            "chair": chair,
            "topic": req.topic,
        }),
    )?;

    let mut spend = MeetingSpend::new(req.ask_above);
    let mut floor: Vec<Position> = Vec::new();
    let mut stopped: Option<String> = None;

    for id in &req.participants {
        let r = match role(id) {
            Some(r) => r,
            None => continue,
        };

        if let Err(reason) = spend.approved(em, &meeting_id, r.id) {
            stopped = Some(reason);
            break;
        }

        *seq += 1;
        let actor = format!("{}#{}", r.id, seq);
        let scene = Scene::desk(r.department.id(), &actor).in_meeting(&meeting_id);
        em.emit(
            &actor,
            EventType::WorkerStateChanged,
            scene.clone(),
            serde_json::json!({"from": "running", "to": "meeting"}),
        )?;

        let brief = position_brief(&req, &floor);
        let m = match crate::m4::run_worker_metered_uncommitted(em, r, &brief, *seq, false, None) {
            Ok(m) => m,
            Err(e) => {
                spend.charge(e.spend.billed_tokens, e.spend.cost_usd);
                println!("  {} did not speak: {e}", r.id);
                continue;
            }
        };
        spend.record(&m);

        let text = m.text.trim().to_string();
        if text.is_empty() {
            println!("  {} returned nothing to put on the floor", r.id);
            continue;
        }

        em.emit(
            &actor,
            EventType::MeetingSpoke,
            scene,
            spoke_data(&meeting_id, r.id, "participant", &text),
        )?;

        println!("    {}: {}", r.id, first_line(&text));
        floor.push(Position { role: r.id, text });
    }

    let outcome = match (stopped.as_deref(), role(chair), floor.is_empty()) {
        (Some(_), _, _) => "stopped",
        (None, _, true) => {
            println!("  nobody stated a position, so there is nothing to decide");
            "adjourned"
        }
        (None, None, _) => "adjourned",
        (None, Some(c), false) => {
            *seq += 1;
            let actor = format!("{}#{}", c.id, seq);
            let scene = Scene::desk(c.department.id(), &actor).in_meeting(&meeting_id);

            let brief = chair_brief(&req, &floor);
            let schema = studio_agents::decision_schema().to_string();

            match crate::m4::run_worker_metered_json(em, c, &brief, *seq, schema) {
                Ok(m) => {
                    spend.record(&m);
                    match studio_agents::Decision::parse(&m.text) {
                        Ok(d) => {
                            em.emit(
                                &actor,
                                EventType::MeetingSpoke,
                                scene,
                                spoke_data(&meeting_id, c.id, "chair", &d.claim),
                            )?;
                            record_decision(em, &req, &meeting_id, c.id, &floor, &d)?;
                            "decided"
                        }
                        Err(e) => {
                            println!("  the chair returned no usable decision: {e}");
                            "adjourned"
                        }
                    }
                }
                Err(e) => {
                    println!("  the chair could not rule: {e}");
                    "adjourned"
                }
            }
        }
    };

    em.emit(
        "daemon",
        EventType::MeetingEnded,
        Scene::daemon().in_meeting(&meeting_id),
        serde_json::json!({
            "meeting_id": meeting_id,
            "outcome": outcome,
            "positions": floor.len(),
            "billed_tokens": spend.billed,
            "usd": spend.usd,
            "reason": stopped,
        }),
    )?;

    println!(
        "  meeting {outcome}: {} position(s), {} billed tokens, ${:.4}",
        floor.len(),
        spend.billed,
        spend.usd
    );

    if let Some(reason) = stopped {
        anyhow::bail!(reason);
    }
    Ok(())
}

fn chair_for(participants: &[String]) -> &'static str {
    let mut chair = participants
        .first()
        .and_then(|p| role(p))
        .map(|r| r.id)
        .unwrap_or("studio_director");

    for p in participants.iter().skip(1) {
        if let Some(common) = nearest_common_ancestor(chair, p) {
            chair = common;
        }
    }
    chair
}

fn extract_json(raw: &str) -> String {
    let t = raw.trim();
    if t.starts_with('{') {
        return t.to_string();
    }
    match (t.find('{'), t.rfind('}')) {
        (Some(a), Some(b)) if b > a => t[a..=b].to_string(),
        _ => t.to_string(),
    }
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").chars().take(70).collect()
}

pub fn serve_studio(store: Arc<Store>, run: String, port: u16) -> Result<()> {
    let (tx, rx) = std::sync::mpsc::channel::<StudioCommand>();
    studio_server::settings::apply_engine_paths_from(&crate::studio_dir());
    let state = AppState::new(store.clone())
        .with_studio_dir(crate::studio_dir())
        .with_commands(tx);

    let serve_state = state.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let _ = rt.block_on(studio_server::serve(serve_state, port));
    });

    let bare = Emitter {
        store: store.clone(),
        state: state.clone(),
        run: run.clone(),
        project: None,
        project_id: None,
    };
    bare.emit(
        "daemon",
        EventType::RunStarted,
        Scene::daemon(),
        serde_json::json!({"title": "interactive studio"}),
    )?;

    println!("studio floor on http://127.0.0.1:{port}/?run={run}");
    match store.projects() {
        Ok(p) if p.is_empty() => {
            println!("no projects yet; create one from the floor before assigning work");
        }
        Ok(p) => println!("{} project(s) known", p.len()),
        Err(e) => println!("could not read projects: {e}"),
    }
    println!("waiting for tasks and meetings from the floor");
    println!();
    let _ = store.checkpoint();

    let mut indexes: std::collections::HashMap<String, ProjectIndex> =
        std::collections::HashMap::new();
    let mut seq = 0usize;

    for cmd in rx {
        let selected = match resolve_project(&store, project_of(&cmd)) {
            Ok(p) => p,
            Err(e) => {
                println!("  command rejected: {e}");
                continue;
            }
        };

        let em = Emitter {
            store: store.clone(),
            state: state.clone(),
            run: run.clone(),
            project: selected.as_ref().map(|p| PathBuf::from(&p.root)),
            project_id: selected.as_ref().map(|p| p.id.clone()),
        };

        if let Some(p) = &selected {
            let _ = store.touch_project(&p.id, crate::now());
            let index = match indexes.entry(p.id.clone()) {
                std::collections::hash_map::Entry::Occupied(e) => Some(e.into_mut()),
                std::collections::hash_map::Entry::Vacant(slot) => {
                    let db = crate::studio_dir().join(format!("index-{}.db", p.id));
                    match ProjectIndex::open(PathBuf::from(&p.root), db) {
                        Ok(idx) => Some(slot.insert(idx)),
                        Err(e) => {
                            println!("  index unavailable for {}: {e}", p.name);
                            None
                        }
                    }
                }
            };
            if let Some(idx) = index {
                idx.refresh_quietly(&em);
            }
        }

        if let Err(e) = run_command(&em, cmd, &mut seq) {
            println!("  command failed: {e}");
        }

        if let Some(p) = &selected {
            if let Some(idx) = indexes.get_mut(&p.id) {
                idx.refresh_quietly(&em);
            }
        }

        if let Err(e) = store.checkpoint() {
            println!("  the event log could not be folded back into its file: {e}");
        }
    }

    studio_server::stop_playing_everything();
    Ok(())
}

fn project_of(cmd: &StudioCommand) -> Option<&str> {
    match cmd {
        StudioCommand::Task(t) => t.project.as_deref(),
        StudioCommand::Workflow(w) => w.project.as_deref(),
        StudioCommand::Build(b) => b.project.as_deref(),
        StudioCommand::Meeting(m) => m.project.as_deref(),
        StudioCommand::Summarize(s) => Some(s.project.as_str()),
        StudioCommand::Resume(r) => Some(r.project.as_str()),
    }
}

fn resolve_project(
    store: &Store,
    requested: Option<&str>,
) -> Result<Option<studio_store::ProjectRow>> {
    let Some(id) = requested else {
        return Ok(None);
    };
    match store.project(id)? {
        Some(p) => {
            let root = PathBuf::from(&p.root);
            if !root.is_dir() {
                anyhow::bail!("project {} points at {}, which is gone", p.name, p.root);
            }
            Ok(Some(p))
        }
        None => anyhow::bail!("unknown project {id}"),
    }
}

#[cfg(test)]
mod index_tests {
    use super::ProjectIndex;
    use crate::m4::Emitter;
    use std::sync::Arc;
    use studio_server::AppState;
    use studio_store::Store;

    struct Harness {
        project: ProjectIndex,
        emitter: Emitter,
        store: Arc<Store>,
        run: String,
        _dirs: (tempfile::TempDir, tempfile::TempDir),
    }

    fn harness() -> Harness {
        let project_dir = tempfile::tempdir().unwrap();
        let state_dir = tempfile::tempdir().unwrap();

        let store = Arc::new(Store::open(state_dir.path().join("studio-state.db")).unwrap());
        let run = "run_test".to_string();
        let emitter = Emitter {
            store: store.clone(),
            state: AppState::new(store.clone()),
            run: run.clone(),
            project: Some(project_dir.path().to_path_buf()),
            project_id: None,
        };

        let project = ProjectIndex::open(
            project_dir.path().to_path_buf(),
            state_dir.path().join("studio-index.db"),
        )
        .unwrap();

        Harness { project, emitter, store, run, _dirs: (project_dir, state_dir) }
    }

    impl Harness {
        fn write(&self, relative: &str, body: &str) {
            let path = self.project.root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }

        fn index_events(&self) -> Vec<serde_json::Value> {
            self.store
                .events_since(&self.run, 0)
                .unwrap()
                .into_iter()
                .filter(|e| e.event_type == studio_events::EventType::IndexUpdated)
                .map(|e| e.data)
                .collect()
        }
    }

    #[test]
    fn a_refresh_that_finds_new_code_announces_it() {
        let mut h = harness();
        h.write("scripts/player.gd", "class_name Player\n\nfunc go():\n\tpass\n");
        h.project.refresh(&h.emitter).unwrap();

        let events = h.index_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["paths_changed"], 1);
        assert_eq!(events[0]["symbols_delta"], 1);
        assert_eq!(events[0]["paths"][0], "scripts/player.gd");
    }

    #[test]
    fn a_refresh_that_changes_nothing_stays_silent() {
        let mut h = harness();
        h.write("scripts/player.gd", "class_name Player\n\nfunc go():\n\tpass\n");
        h.project.refresh(&h.emitter).unwrap();
        h.project.refresh(&h.emitter).unwrap();
        h.project.refresh(&h.emitter).unwrap();

        assert_eq!(h.index_events().len(), 1);
    }

    #[test]
    fn a_worker_editing_a_file_makes_the_next_lookup_see_the_edit() {
        let mut h = harness();
        h.write("scripts/player.gd", "class_name Player\n\nfunc go():\n\tpass\n");
        h.project.refresh(&h.emitter).unwrap();
        assert_eq!(h.project.index.lookup("Player.go", 5).unwrap().len(), 1);

        h.write("scripts/player.gd", "class_name Player\n\nfunc sprint():\n\tpass\n");
        h.project.refresh(&h.emitter).unwrap();

        assert!(h.project.index.lookup("Player.go", 5).unwrap().is_empty());
        assert_eq!(h.project.index.lookup("Player.sprint", 5).unwrap().len(), 1);
        assert_eq!(h.index_events().len(), 2);
    }

    #[test]
    fn an_edit_made_while_the_studio_was_idle_is_indexed_before_the_next_command_runs() {
        let mut h = harness();
        h.write("scripts/player.gd", "class_name Player\n\nfunc go():\n\tpass\n");
        h.project.refresh(&h.emitter).unwrap();

        h.write("scripts/player.gd", "class_name Player\n\nfunc go():\n\tpass\n\nfunc dash():\n\tpass\n");
        h.project.refresh_quietly(&h.emitter);

        assert_eq!(h.project.index.lookup("Player.dash", 5).unwrap().len(), 1);
    }

    #[test]
    fn refreshing_twice_around_a_command_announces_the_change_only_once() {
        let mut h = harness();
        h.write("scripts/player.gd", "class_name Player\n\nfunc go():\n\tpass\n");

        h.project.refresh_quietly(&h.emitter);
        h.project.refresh_quietly(&h.emitter);

        assert_eq!(h.index_events().len(), 1);
    }

    #[test]
    fn a_deletion_is_announced_with_a_negative_symbol_delta() {
        let mut h = harness();
        h.write("scripts/player.gd", "class_name Player\n\nfunc go():\n\tpass\n");
        h.project.refresh(&h.emitter).unwrap();

        std::fs::remove_file(h.project.root.join("scripts/player.gd")).unwrap();
        h.project.refresh(&h.emitter).unwrap();

        let events = h.index_events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1]["symbols_delta"], -1);
    }
}

#[cfg(test)]
mod guided_tests {
    use super::*;
    use studio_workflow::{Plan, PlanTask};

    fn step(id: &str, role: &str, brief: &str, say: &str, deps: &[&str]) -> PlanTask {
        PlanTask {
            id: id.into(),
            role: role.into(),
            brief: brief.into(),
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
            say: say.into(),
        }
    }

    fn plan() -> Plan {
        Plan {
            title: "Flappy".into(),
            tasks: vec![
                step(
                    "t1",
                    "artist",
                    "Author a 16x16 sprite atlas for the avatar and obstacle columns.",
                    "Draw the player and the pipes it flies through",
                    &[],
                ),
                step(
                    "t2",
                    "gameplay_engineer",
                    "Implement the flap impulse and gravity integration.",
                    "Make the bird flap when a key is pressed",
                    &["t1"],
                ),
            ],
        }
    }

    #[test]
    fn the_plan_reaches_the_floor_in_words_a_person_would_use() {
        let d = plan_data("plan_1", &plan(), true);
        assert_eq!(d["steps"][0]["say"], "Draw the player and the pipes it flies through");
        assert_eq!(d["steps"][1]["say"], "Make the bird flap when a key is pressed");
        assert!(
            !d["steps"][0]["say"].as_str().unwrap().contains("atlas"),
            "a producer does not say sprite atlas to a colleague"
        );
    }

    #[test]
    fn the_proposal_carries_every_key_doc_05_promises() {
        let d = plan_data("plan_1", &plan(), true);
        for key in ["plan_id", "title", "steps", "editable"] {
            assert!(d.get(key).is_some(), "plan_proposed does not emit {key}");
        }
        assert_eq!(d["plan_id"], "plan_1");
        assert_eq!(d["title"], "Flappy");
    }

    #[test]
    fn a_guided_plan_is_marked_editable_and_an_old_style_build_is_not() {
        assert_eq!(plan_data("plan_1", &plan(), true)["editable"], true);
        assert_eq!(
            plan_data("plan_1", &plan(), false)["editable"],
            false,
            "a run nobody is holding must not look like it is waiting"
        );
    }

    #[test]
    fn the_floor_gets_the_detailed_brief_alongside_the_plain_line() {
        let d = plan_data("plan_1", &plan(), true);
        assert!(d["steps"][0]["brief"].as_str().unwrap().contains("sprite atlas"));
        assert_eq!(d["steps"][1]["depends_on"][0], "t1");
        assert_eq!(d["steps"][1]["role"], "gameplay_engineer");
    }

    #[test]
    fn a_guided_build_runs_against_the_place_the_human_picked() {
        let cmd = StudioCommand::Build(BuildRequest {
            prompt: "make flappy bird".into(),
            project: Some("proj_flappy".into()),
            ask_above: None,
            guided: true,
            step_confirm: true,
        });
        assert_eq!(project_of(&cmd), Some("proj_flappy"));
    }
}

#[cfg(test)]
mod meeting_tests {
    use super::*;
    use studio_agents::Decision;

    fn req() -> MeetingRequest {
        MeetingRequest {
            kind: "arbitration".into(),
            participants: vec!["gameplay_engineer".into(), "qa_engineer".into()],
            topic: "Should dash cancel an attack?".into(),
            project: None,
            ask_above: None,
        }
    }

    fn floor() -> Vec<Position> {
        vec![
            Position {
                role: "gameplay_engineer",
                text: "Dash cancels the attack; input responsiveness beats commitment.".into(),
            },
            Position {
                role: "qa_engineer",
                text: "Cancelling opens a desync between animation and hitbox state.".into(),
            },
        ]
    }

    fn decision() -> Decision {
        Decision {
            claim: "Dash cancels an attack only after the active hitbox frames end.".into(),
            rationale: "It keeps input responsive without leaving a hitbox alive mid-dash."
                .into(),
            dissent: vec!["gameplay_engineer wanted an unconditional cancel".into()],
        }
    }

    #[test]
    fn the_first_speaker_is_told_the_room_is_empty_rather_than_given_a_blank_list() {
        let b = position_brief(&req(), &[]);
        assert!(b.contains("You speak first"));
        assert!(!b.contains("The room has already said"));
    }

    #[test]
    fn a_later_speaker_is_handed_what_was_actually_said() {
        let b = position_brief(&req(), &floor());
        assert!(
            b.contains("input responsiveness beats commitment"),
            "a participant that cannot read the room is not in a meeting: {b}"
        );
        assert!(b.contains("desync between animation and hitbox state"));
        assert!(b.contains("gameplay_engineer:"));
        assert!(b.contains("qa_engineer:"));
    }

    #[test]
    fn no_brief_ever_tells_a_worker_to_go_look_at_a_capsule_it_cannot_reach() {
        for b in [position_brief(&req(), &floor()), chair_brief(&req(), &floor())] {
            assert!(!b.contains("see capsule"), "workers have no way to read a capsule: {b}");
        }
    }

    #[test]
    fn the_chair_is_handed_every_position_verbatim() {
        let b = chair_brief(&req(), &floor());
        for p in floor() {
            assert!(b.contains(&p.text), "the chair cannot rule on what it was not told: {b}");
        }
        assert!(!b.contains("The room has spoken."));
    }

    #[test]
    fn the_chair_is_asked_for_a_rule_and_not_a_summary() {
        let b = chair_brief(&req(), &floor());
        assert!(b.contains("rule the studio follows"));
        assert!(b.contains("not a \nsummary") || b.contains("not a summary"));
    }

    #[test]
    fn the_adr_carries_the_decision_the_reason_and_the_room() {
        let body = adr_body(&req(), "Should dash cancel an attack?", "studio_director", &floor(), &decision());
        assert!(body.starts_with("# Should dash cancel an attack?"));
        assert!(body.contains("## Decision\n\nDash cancels an attack only after"));
        assert!(body.contains("## Why\n\nIt keeps input responsive"));
        assert!(body.contains("## Overruled"));
        assert!(body.contains("gameplay_engineer wanted an unconditional cancel"));
        assert!(body.contains("## The room"));
        assert!(body.contains("desync between animation and hitbox state"));
        assert!(body.contains("- Chair: studio_director"));
    }

    #[test]
    fn an_agreed_decision_writes_no_empty_overruled_section() {
        let mut d = decision();
        d.dissent.clear();
        let body = adr_body(&req(), "t", "studio_director", &floor(), &d);
        assert!(!body.contains("## Overruled"));
    }

    #[test]
    fn a_meeting_with_no_cap_never_stops_to_ask() {
        let mut spend = MeetingSpend::new(None);
        spend.billed = 10_000_000;
        assert_eq!(spend.next_ask_at, u64::MAX);
        assert!(spend.billed < spend.next_ask_at);
    }

    #[test]
    fn a_zero_cap_is_treated_as_no_cap_rather_than_asking_before_every_word() {
        assert_eq!(MeetingSpend::new(Some(0)).next_ask_at, u64::MAX);
    }

    #[test]
    fn a_cap_arms_the_first_check_at_the_threshold() {
        let mut spend = MeetingSpend::new(Some(5_000));
        assert_eq!(spend.next_ask_at, 5_000);
        spend.record(&crate::m4::Metered {
            text: String::new(),
            billed_tokens: 4_999,
            cost_usd: 0.1,
        });
        assert!(spend.billed < spend.next_ask_at, "under the cap must not interrupt the room");
        spend.record(&crate::m4::Metered {
            text: String::new(),
            billed_tokens: 1,
            cost_usd: 0.1,
        });
        assert!(spend.billed >= spend.next_ask_at);
    }

    #[test]
    fn spend_accumulates_across_speakers() {
        let mut spend = MeetingSpend::new(Some(100));
        for _ in 0..3 {
            spend.record(&crate::m4::Metered {
                text: String::new(),
                billed_tokens: 10,
                cost_usd: 0.5,
            });
        }
        assert_eq!(spend.billed, 30);
        assert!((spend.usd - 1.5).abs() < 1e-9);
    }

    #[test]
    fn the_spoken_payload_carries_the_keys_the_floor_reads() {
        let d = spoke_data("meeting_1", "qa_engineer", "participant", "hitboxes desync");
        assert_eq!(d["meeting_id"], "meeting_1");
        assert_eq!(d["role"], "qa_engineer");
        assert_eq!(d["seat"], "participant");
        assert_eq!(d["position"], "hitboxes desync");
    }

    #[test]
    fn the_decision_payload_carries_the_ruling_and_not_just_a_title() {
        let d = decision_data(
            "adr_1",
            "meeting_1",
            "Should dash cancel an attack?",
            "studio_director",
            &decision(),
            Some(&"docs/decisions/adr_1-dash.md".to_string()),
        );
        assert_eq!(d["decision_id"], "adr_1");
        assert_eq!(d["meeting_id"], "meeting_1");
        assert_eq!(d["chair"], "studio_director");
        assert_eq!(d["claim"], decision().claim);
        assert_eq!(d["rationale"], decision().rationale);
        assert_eq!(d["dissent"][0], decision().dissent[0]);
        assert_eq!(d["path"], "docs/decisions/adr_1-dash.md");
    }

    #[test]
    fn a_decision_with_no_repo_reports_a_null_path_rather_than_an_empty_string() {
        let d = decision_data("adr_1", "meeting_1", "t", "studio_director", &decision(), None);
        assert!(d["path"].is_null(), "the floor hides the path when there is no repo");
    }

    #[test]
    fn every_key_the_floor_reads_off_these_events_is_one_we_emit() {
        let floor = include_str!("../../studio-server/web/floor.html");

        let spoke = spoke_data("m", "r", "participant", "p");
        let ruling =
            decision_data("adr_1", "m", "t", "studio_director", &decision(), None);

        let start = floor
            .find("case \"meeting_spoke\"")
            .expect("the floor must handle meeting_spoke or the room stays empty");
        let end = floor[start..]
            .find("case \"meeting_ended\"")
            .map(|i| start + i)
            .expect("meeting_ended follows the handlers under test");
        let handlers = &floor[start..end];

        for key in ["meeting_id", "role", "seat", "position"] {
            assert!(
                handlers.contains(&format!("ev.data.{key}")),
                "the floor ignores meeting_spoke.{key}"
            );
            assert!(spoke.get(key).is_some(), "meeting_spoke does not emit {key}");
        }

        for key in ["claim", "rationale", "dissent", "path"] {
            assert!(
                handlers.contains(&format!("ev.data.{key}")),
                "the floor ignores decision_recorded.{key}"
            );
            assert!(ruling.get(key).is_some(), "decision_recorded does not emit {key}");
        }
    }

    #[test]
    fn a_meeting_can_name_a_project_so_its_decision_lands_in_that_repo() {
        let mut r = req();
        r.project = Some("proj_flappy".into());
        let cmd = StudioCommand::Meeting(r);
        assert_eq!(
            project_of(&cmd),
            Some("proj_flappy"),
            "a meeting whose project is dropped can never write an ADR to the repo"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{chair_for, last_wave_node};

    fn v(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    fn wf_of(nodes: &[&str], edges: &[(&str, &str)]) -> studio_workflow::Workflow {
        studio_workflow::Workflow {
            schema_version: 1,
            id: "t".into(),
            title: "T".into(),
            nodes: nodes
                .iter()
                .map(|id| studio_workflow::Node {
                    id: (*id).into(),
                    role: "gameplay_engineer".into(),
                    inputs: Vec::new(),
                    budget_tokens: 1,
                    optional: false,
                })
                .collect(),
            edges: edges
                .iter()
                .map(|(from, to)| studio_workflow::Edge {
                    from: (*from).into(),
                    to: (*to).into(),
                    carries: "task_return".into(),
                })
                .collect(),
            gates: Vec::new(),
        }
    }

    #[test]
    fn the_gate_lands_on_the_node_that_finishes_last_not_the_one_declared_last() {
        let wf = wf_of(
            &["t1", "t2", "t3", "t8"],
            &[("t1", "t2"), ("t2", "t3"), ("t1", "t8")],
        );
        assert_eq!(
            last_wave_node(&wf).as_deref(),
            Some("t3"),
            "t8 is declared last but runs in wave 2 of 3; verifying there compiles a half-written \
             project and burns repair workers on code later waves were going to write"
        );
    }

    #[test]
    fn a_single_leaf_still_takes_the_gate() {
        let wf = wf_of(&["t1", "t2"], &[("t1", "t2")]);
        assert_eq!(last_wave_node(&wf).as_deref(), Some("t2"));
    }

    #[test]
    fn a_flat_plan_with_no_edges_gates_after_a_node_in_its_only_wave() {
        let wf = wf_of(&["t1", "t2", "t3"], &[]);
        let picked = last_wave_node(&wf).expect("a flat plan still has a final wave");
        assert!(["t1", "t2", "t3"].contains(&picked.as_str()));
    }

    #[test]
    fn two_designers_are_chaired_by_their_lead() {
        assert_eq!(chair_for(&v(&["level_designer", "narrative_designer"])), "game_designer");
    }

    #[test]
    fn cross_department_meetings_escalate_to_the_common_ancestor() {
        assert_eq!(chair_for(&v(&["gameplay_engineer", "artist"])), "systems_engineer");
        assert_eq!(chair_for(&v(&["qa_engineer", "gameplay_engineer"])), "studio_director");
    }

    #[test]
    fn a_lead_chairs_a_meeting_with_its_own_report() {
        assert_eq!(chair_for(&v(&["game_designer", "level_designer"])), "game_designer");
    }

    #[test]
    fn three_participants_still_resolve_to_one_chair() {
        let chair = chair_for(&v(&["level_designer", "narrative_designer", "ux_designer"]));
        assert_eq!(chair, "game_designer");
    }

    #[test]
    fn a_meeting_spanning_the_whole_studio_is_chaired_by_the_director() {
        let chair = chair_for(&v(&["artist", "qa_engineer", "narrative_designer"]));
        assert_eq!(chair, "studio_director");
    }

    #[test]
    fn an_unknown_participant_does_not_break_the_chair_choice() {
        assert_eq!(chair_for(&v(&["level_designer", "no_such_role"])), "level_designer");
    }
}
