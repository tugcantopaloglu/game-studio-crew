use anyhow::{Context, Result};
use std::sync::Arc;
use studio_agents::{Role, REGISTRY};
use studio_context::{freeze, CharterSource};
use studio_core::{Effort, SessionMode, Worker, WorkerLimits, WorkerSpec};
use studio_core::map_cli_event;
use studio_events::{EventType, Outcome, Scene, WorkerState};
use studio_server::AppState;
use studio_store::{LedgerEntry, RoleRow, Store, TaskRow};

pub struct Emitter {
    pub store: Arc<Store>,
    pub state: AppState,
    pub run: String,
    pub project: Option<std::path::PathBuf>,
}

impl Emitter {
    pub fn emit(
        &self,
        actor: &str,
        event_type: EventType,
        scene: Scene,
        data: serde_json::Value,
    ) -> Result<()> {
        let env = self
            .store
            .append_event(&self.run, crate::now(), actor, event_type, scene, data)?;
        self.state.publish(env);
        Ok(())
    }
}

pub fn register_roles(store: &Store) -> Result<()> {
    for r in &REGISTRY {
        store.upsert_role(RoleRow {
            id: r.id.into(),
            tier: r.tier,
            department: r.department.id().into(),
            model: r.model.cli_alias().into(),
            effort: r.effort.as_str().into(),
            escalates_to: None,
        })?;
    }
    for r in &REGISTRY {
        if let Some(parent) = r.escalates_to {
            store.upsert_role(RoleRow {
                id: r.id.into(),
                tier: r.tier,
                department: r.department.id().into(),
                model: r.model.cli_alias().into(),
                effort: r.effort.as_str().into(),
                escalates_to: Some(parent.into()),
            })?;
        }
    }
    Ok(())
}

fn commit_worker_output(em: &Emitter, role: &Role, brief: &str, actor: &str) -> Result<()> {
    let Some(root) = em.project.as_deref() else {
        return Ok(());
    };
    if !studio_core::git::is_repo(root) {
        return Ok(());
    }

    let subject = studio_core::git::subject(role.id, brief);
    let sha = match studio_core::git::commit(root, &subject) {
        Ok(Some(sha)) => sha,
        Ok(None) => return Ok(()),
        Err(e) => {
            println!("  commit skipped: {e}");
            return Ok(());
        }
    };

    println!("  commit {sha}  {subject}");
    em.emit(
        actor,
        EventType::CommitRecorded,
        Scene::daemon(),
        serde_json::json!({
            "project": root.to_string_lossy(),
            "role": role.id,
            "sha": sha,
            "subject": subject,
        }),
    )?;
    Ok(())
}

pub fn run_worker(em: &Emitter, role: &Role, brief: &str, index: usize) -> Result<()> {
    run_worker_capturing(em, role, brief, index, None).map(|_| ())
}

pub fn run_worker_capturing(
    em: &Emitter,
    role: &Role,
    brief: &str,
    index: usize,
    json_schema: Option<String>,
) -> Result<String> {
    run_worker_inner(em, role, brief, index, json_schema, false).map(|m| m.text)
}

pub fn run_worker_metered(
    em: &Emitter,
    role: &Role,
    brief: &str,
    index: usize,
    acting: bool,
) -> Result<Metered> {
    run_worker_inner(em, role, brief, index, None, acting)
}

pub struct Metered {
    pub text: String,
    pub billed_tokens: u64,
    pub cost_usd: f64,
}

pub fn charter_for(role: &Role, acting: bool) -> CharterSource {
    CharterSource {
        studio_conventions: crate::charters::L0_STUDIO_CONVENTIONS.into(),
        engine_profile: crate::charters::L1_GENERIC_ENGINE.into(),
        role_charter: format!(
            "You are the {}. {}\n\n{}",
            role.title,
            match role.tier {
                1 => "You set studio direction and arbitrate across departments.",
                2 => "You lead your department and decompose work for it.",
                _ => "You do hands-on work in your department.",
            },
            if acting {
                "Use your tools to make the change the brief asks for, then report what you changed in one short sentence."
            } else {
                "Answer the brief in one short sentence. Use no tools."
            }
        ),
    }
}

pub fn prefix_tokens_for(role: &Role, acting: bool) -> u64 {
    let charter = charter_for(role, acting);
    freeze(&charter, &role.tools(), role.model)
        .map(|p| p.estimated_tokens as u64)
        .unwrap_or(8_000)
}

fn run_worker_inner(
    em: &Emitter,
    role: &Role,
    brief: &str,
    index: usize,
    json_schema: Option<String>,
    acting: bool,
) -> Result<Metered> {
    let actor = format!("{}#{}", role.id, index);
    let task_id = crate::id("task");

    em.store.insert_task(
        TaskRow {
            id: task_id.clone(),
            run: em.run.clone(),
            role: role.id.into(),
            parent_task: None,
            workflow_node: None,
            state: WorkerState::Queued,
            outcome: None,
        },
        crate::now(),
    )?;

    let charter = charter_for(role, acting);
    let tools = role.tools();
    let prefix = freeze(&charter, &tools, role.model)
        .map_err(|e| anyhow::anyhow!("charter freeze failed for {}: {e}", role.id))?;
    let charter_path = crate::write_charter(&prefix)?;

    em.emit(
        "daemon",
        EventType::PromptFrozen,
        Scene::daemon(),
        prefix.prompt_frozen_data(role.id),
    )?;

    let scene = Scene::desk(role.department.id(), &actor);
    em.emit(
        &actor,
        EventType::WorkerSpawned,
        scene.clone(),
        serde_json::json!({
            "role": role.id,
            "model": prefix.model,
            "effort": role.effort.as_str(),
            "prefix_hash": prefix.prefix_hash,
        }),
    )?;
    em.store.update_task_state(&task_id, WorkerState::Running, None, crate::now())?;
    em.emit(
        &actor,
        EventType::WorkerStateChanged,
        scene.clone(),
        serde_json::json!({"from": "queued", "to": "running"}),
    )?;

    let spec = WorkerSpec {
        system_prompt_file: charter_path.to_string_lossy().into_owned(),
        tools: tools.clone(),
        allowed_tools: if acting { tools.clone() } else { Vec::new() },
        model: role.model,
        effort: match role.effort {
            studio_agents::Effort::Low => Effort::Low,
            studio_agents::Effort::Medium => Effort::Medium,
            studio_agents::Effort::High => Effort::High,
            studio_agents::Effort::XHigh => Effort::XHigh,
            studio_agents::Effort::Max => Effort::Max,
        },
        session: SessionMode::New(crate::uuid_v4()),
        mcp_config: None,
        json_schema,
    };

    let worker = Worker::spawn_in("claude", &spec.to_args(), brief, em.project.as_deref())
        .with_context(|| format!("failed to spawn a worker for {}", role.id))?;

    let report = worker.drive(&WorkerLimits::default(), |ev| {
        if let Some((ty, data)) = map_cli_event(ev) {
            let _ = em.emit(&actor, ty, scene.clone(), data);
        }
    })?;

    let usage = report.state.usage.unwrap_or_default();
    em.store.record_usage(
        LedgerEntry {
            task: task_id.clone(),
            role: role.id.into(),
            prefix_hash: prefix.prefix_hash.clone(),
            estimate: false,
            usage,
            cost_usd: report.state.cost_usd,
            model: prefix.model.cli_alias().into(),
        },
        crate::now(),
    )?;

    em.emit(
        &actor,
        EventType::TokenUsage,
        scene.clone(),
        serde_json::json!({
            "estimate": false,
            "input": usage.input,
            "output": usage.output,
            "cache_read": usage.cache_read,
            "cache_creation": usage.cache_creation,
            "cost_usd": report.state.cost_usd,
        }),
    )?;

    if usage.cache_read > 0 {
        em.emit(
            &actor,
            EventType::CacheHit,
            scene.clone(),
            serde_json::json!({
                "role": role.id,
                "prefix_hash": prefix.prefix_hash,
                "cache_read": usage.cache_read,
                "cache_creation": usage.cache_creation,
            }),
        )?;
    }

    em.emit(
        &actor,
        EventType::CapsuleSubmitted,
        scene.clone(),
        serde_json::json!({
            "kind": "task_return",
            "summary": report.state.text.trim(),
            "rendered_tokens": usage.output,
            "truncated": false,
        }),
    )?;

    em.store.update_task_state(&task_id, WorkerState::Reaped, Some(report.outcome), crate::now())?;
    em.emit(
        &actor,
        EventType::WorkerExited,
        scene,
        serde_json::json!({
            "outcome": format!("{:?}", report.outcome).to_lowercase(),
            "exit_code": report.exit_code,
        }),
    )?;

    println!(
        "  {:<20} {:?} {} tokens ${:.4}",
        role.id,
        report.outcome,
        usage.input + usage.output,
        report.state.cost_usd
    );

    if report.outcome != Outcome::Completed {
        anyhow::bail!(
            "{} did not complete ({:?}): {}",
            role.id,
            report.outcome,
            report.state.text.lines().next().unwrap_or("no output")
        );
    }

    if report.state.is_error {
        anyhow::bail!(
            "{} failed: {}",
            role.id,
            report.state.text.lines().next().unwrap_or("unknown error")
        );
    }

    commit_worker_output(em, role, brief, &actor)?;

    let text = match &report.state.result_message {
        Some(m) if !m.trim().is_empty() => m.clone(),
        _ => report.state.text.clone(),
    };
    let billed = studio_budget::billable_tokens(studio_budget::Usage {
        input: usage.input,
        output: usage.output,
        cache_read: usage.cache_read,
        cache_creation: usage.cache_creation,
    });
    Ok(Metered { text, billed_tokens: billed, cost_usd: report.state.cost_usd })
}
