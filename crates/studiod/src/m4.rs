use anyhow::{Context, Result};
use std::path::Path;
use std::sync::Arc;
use studio_agents::{Role, REGISTRY};
use studio_context::{freeze, CharterSource, Model};
use studio_core::map_cli_event;
use studio_core::{
    BriefDelivery, CliEvent, Effort, Provider, RoleNeeds, SessionMode, Worker, WorkerLimits,
    WorkerSpec,
};
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
    run_worker_inner(em, role, brief, index, json_schema, false, true).map(|m| m.text)
}

pub fn run_worker_metered(
    em: &Emitter,
    role: &Role,
    brief: &str,
    index: usize,
    acting: bool,
) -> Result<Metered> {
    run_worker_inner(em, role, brief, index, None, acting, true)
}

pub fn run_worker_metered_uncommitted(
    em: &Emitter,
    role: &Role,
    brief: &str,
    index: usize,
    acting: bool,
) -> Result<Metered> {
    run_worker_inner(em, role, brief, index, None, acting, false)
}

pub fn run_worker_metered_json(
    em: &Emitter,
    role: &Role,
    brief: &str,
    index: usize,
    json_schema: String,
) -> Result<Metered> {
    run_worker_inner(em, role, brief, index, Some(json_schema), false, false)
}

pub fn commit_wave(em: &Emitter, entries: &[(&str, String)]) {
    let Some(root) = em.project.as_deref() else {
        return;
    };
    if entries.is_empty() || !studio_core::git::is_repo(root) {
        return;
    }

    let subject = if entries.len() == 1 {
        match studio_agents::role(entries[0].0) {
            Some(r) => studio_core::git::subject(r.id, &entries[0].1),
            None => studio_core::git::subject(entries[0].0, &entries[0].1),
        }
    } else {
        let mut roles: Vec<&str> = entries.iter().map(|(r, _)| *r).collect();
        roles.dedup();
        format!("crew: {} finish parallel work", roles.join(" + "))
    };

    let sha = match studio_core::git::commit(root, &subject) {
        Ok(Some(sha)) => sha,
        Ok(None) => return,
        Err(e) => {
            println!("  commit skipped: {e}");
            return;
        }
    };

    println!("  commit {sha}  {subject}");
    let _ = em.emit(
        "daemon",
        EventType::CommitRecorded,
        Scene::daemon(),
        serde_json::json!({
            "project": root.to_string_lossy(),
            "role": entries.iter().map(|(r, _)| *r).collect::<Vec<_>>().join("+"),
            "sha": sha,
            "subject": subject,
        }),
    );
}

pub struct Metered {
    pub text: String,
    pub billed_tokens: u64,
    pub cost_usd: f64,
}

fn limits_for(acting: bool) -> WorkerLimits {
    if !acting {
        return WorkerLimits::default();
    }
    WorkerLimits {
        stall_timeout: std::time::Duration::from_secs(300),
        wall_clock: std::time::Duration::from_secs(2700),
    }
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

#[derive(Debug, Clone, PartialEq)]
pub struct Seat {
    pub provider: Provider,
    pub model: Model,
    pub model_alias: String,
    pub effort: Effort,
    pub overridden: bool,
}

fn model_named(alias: &str) -> Option<Model> {
    match alias {
        "fable" => Some(Model::Fable),
        "opus" => Some(Model::Opus),
        "haiku" => Some(Model::Haiku),
        _ => None,
    }
}

fn effort_named(name: &str) -> Option<Effort> {
    match name {
        "low" => Some(Effort::Low),
        "medium" => Some(Effort::Medium),
        "high" => Some(Effort::High),
        "xhigh" => Some(Effort::XHigh),
        "max" => Some(Effort::Max),
        _ => None,
    }
}

fn shipped_effort(role: &Role) -> Effort {
    match role.effort {
        studio_agents::Effort::Low => Effort::Low,
        studio_agents::Effort::Medium => Effort::Medium,
        studio_agents::Effort::High => Effort::High,
        studio_agents::Effort::XHigh => Effort::XHigh,
        studio_agents::Effort::Max => Effort::Max,
    }
}

pub fn seat_from(settings: &studio_settings::Settings, role: &Role) -> Seat {
    let choice = settings.role_choice(role.id, role.tier);
    let provider = Provider::from_id(&choice.provider).unwrap_or(Provider::Claude);
    let chosen_model = choice.model.filter(|m| !m.is_empty());

    let (model, model_alias) = match provider {
        Provider::Claude => match chosen_model.as_deref().and_then(model_named) {
            Some(m) => (m, m.cli_alias().to_string()),
            None => (role.model, role.model.cli_alias().to_string()),
        },
        _ => (role.model, chosen_model.clone().unwrap_or_default()),
    };

    let effort = choice
        .effort
        .as_deref()
        .and_then(effort_named)
        .unwrap_or_else(|| shipped_effort(role));

    Seat {
        overridden: provider != Provider::Claude
            || model != role.model
            || effort != shipped_effort(role),
        provider,
        model,
        model_alias,
        effort,
    }
}

pub fn seat_for(role: &Role, studio_dir: &Path) -> Seat {
    let settings = studio_settings::Settings::load(&studio_settings::Settings::path_in(studio_dir))
        .unwrap_or_default();
    seat_from(&settings, role)
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
    commit: bool,
) -> Result<Metered> {
    let actor = format!("{}#{}", role.id, index);
    let seat = seat_for(role, &em.state.studio_dir);
    let needs = RoleNeeds {
        structured_output: json_schema.is_some(),
        restricted_tools: true,
    };
    if let Some(reason) = seat.provider.blockers(needs).into_iter().next() {
        anyhow::bail!(
            "{} is set to run on {} and cannot: {reason}",
            role.id,
            seat.provider.id()
        );
    }

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
    let prefix = freeze(&charter, &tools, seat.model)
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
            "effort": seat.effort.as_str(),
            "provider": seat.provider.id(),
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
        model: seat.model,
        effort: seat.effort,
        session: SessionMode::New(crate::uuid_v4()),
        mcp_config: None,
        json_schema,
    };

    let mut args = spec.to_args_for(seat.provider, &seat.model_alias);
    let stdin_brief = match seat.provider.brief_delivery() {
        BriefDelivery::Stdin => brief,
        BriefDelivery::PromptArgument => {
            args.push("-p".into());
            args.push(brief.to_string());
            ""
        }
    };

    let worker = Worker::spawn_in(
        seat.provider.program(),
        &args,
        stdin_brief,
        em.project.as_deref(),
    )
    .with_context(|| {
        format!(
            "failed to spawn a worker for {} on {}; is {} on PATH?",
            role.id,
            seat.provider.id(),
            seat.provider.program()
        )
    })?;

    let thoughts = std::sync::Mutex::new(crate::thought::Stream::new());
    let report = worker.drive(&limits_for(acting), |ev| {
        if let CliEvent::RateLimit { raw } = ev {
            studio_server::settings::observe_rate_limit(raw);
        }
        if let Some((ty, data)) = map_cli_event(ev) {
            let _ = em.emit(&actor, ty, scene.clone(), data);
        }
        if let Some((ty, data)) = thoughts.lock().unwrap().observe(ev, role.id) {
            let _ = em.emit(&actor, ty, scene.clone(), data);
        }
    })?;

    if let Some((ty, data)) = thoughts.lock().unwrap().flush(role.id) {
        let _ = em.emit(&actor, ty, scene.clone(), data);
    }

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

    if commit {
        commit_worker_output(em, role, brief, &actor)?;
    }

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

#[cfg(test)]
mod seat_tests {
    use super::*;
    use studio_settings::Settings;

    fn settings(pairs: &[(&str, &str)]) -> Settings {
        let mut s = Settings::new();
        for (k, v) in pairs {
            s.set(k, serde_json::Value::String((*v).into()));
        }
        s
    }

    fn role_named(id: &str) -> &'static Role {
        studio_agents::role(id).unwrap()
    }

    #[test]
    fn an_unconfigured_studio_seats_every_role_exactly_where_the_registry_ships_it() {
        for r in &REGISTRY {
            let seat = seat_from(&Settings::new(), r);
            assert_eq!(seat.provider, Provider::Claude);
            assert_eq!(seat.model, r.model);
            assert_eq!(seat.model_alias, r.model.cli_alias());
            assert!(!seat.overridden, "{} reads as overridden with no settings", r.id);
        }
    }

    #[test]
    fn a_tier_default_moves_every_seat_in_that_tier_off_the_shipped_model() {
        let s = settings(&[("models.tier3", "haiku")]);
        assert_eq!(seat_from(&s, role_named("artist")).model, Model::Haiku);
        assert_eq!(seat_from(&s, role_named("producer")).model, Model::Opus);
        assert_eq!(seat_from(&s, role_named("studio_director")).model, Model::Fable);
    }

    #[test]
    fn one_role_can_be_moved_without_disturbing_its_neighbours() {
        let s = settings(&[("models.role.qa_engineer", "haiku")]);
        assert_eq!(seat_from(&s, role_named("qa_engineer")).model, Model::Haiku);
        assert_eq!(seat_from(&s, role_named("artist")).model, Model::Opus);
    }

    #[test]
    fn the_overridden_model_is_the_one_the_prefix_is_frozen_against() {
        let s = settings(&[("models.role.gameplay_engineer", "haiku")]);
        let role = role_named("gameplay_engineer");
        let seat = seat_from(&s, role);

        let charter = charter_for(role, false);
        let shipped = freeze(&charter, &role.tools(), role.model).unwrap();
        let chosen = freeze(&charter, &role.tools(), seat.model).unwrap();

        assert_ne!(
            shipped.prefix_hash, chosen.prefix_hash,
            "the model is part of the cache key; freezing against the registry would mint a hash the worker never used"
        );
    }

    #[test]
    fn a_model_name_the_cli_does_not_take_leaves_the_seat_on_its_shipped_model() {
        let s = settings(&[("models.role.artist", "gpt-5.4")]);
        let seat = seat_from(&s, role_named("artist"));
        assert_eq!(seat.model, Model::Opus);
        assert_eq!(seat.model_alias, "opus");
    }

    #[test]
    fn effort_can_be_raised_per_tier_and_again_per_role() {
        let s = settings(&[("effort.tier3", "low"), ("effort.role.qa_engineer", "max")]);
        assert_eq!(seat_from(&s, role_named("artist")).effort, Effort::Low);
        assert_eq!(seat_from(&s, role_named("qa_engineer")).effort, Effort::Max);
    }

    #[test]
    fn another_provider_carries_its_own_model_name_through_untouched() {
        let s = settings(&[
            ("provider.role.artist", "gemini"),
            ("models.gemini.role.artist", "gemini-3-pro"),
        ]);
        let seat = seat_from(&s, role_named("artist"));
        assert_eq!(seat.provider, Provider::Gemini);
        assert_eq!(seat.model_alias, "gemini-3-pro");
    }

    #[test]
    fn a_coordination_seat_still_needs_tool_restriction_because_its_list_is_empty_on_purpose() {
        let director = role_named("studio_director");
        assert!(director.tools().is_empty());
        assert!(
            !Provider::Gemini.can_serve(RoleNeeds {
                structured_output: false,
                restricted_tools: true,
            }),
            "an empty allowlist is the strongest restriction there is, not the absence of one"
        );
    }

    #[test]
    fn a_provider_that_cannot_hold_the_frozen_charter_is_refused_rather_than_degraded() {
        let s = settings(&[("provider", "gemini")]);
        let seat = seat_from(&s, role_named("gameplay_engineer"));
        let blockers = seat.provider.blockers(RoleNeeds {
            structured_output: false,
            restricted_tools: true,
        });
        assert!(blockers.iter().any(|r| r.contains("system prompt")));
    }

    #[test]
    fn a_seat_is_read_from_the_file_the_floor_writes_without_restarting_the_daemon() {
        let dir = std::env::temp_dir().join("studio-seat-from-disk");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let role = role_named("tech_artist");
        assert_eq!(seat_for(role, &dir).model, role.model);

        settings(&[("models.role.tech_artist", "haiku")])
            .save(&Settings::path_in(&dir))
            .unwrap();
        assert_eq!(seat_for(role, &dir).model, Model::Haiku);
    }

    #[test]
    fn a_settings_file_that_names_no_such_provider_falls_back_to_claude() {
        let s = settings(&[("provider", "wishful")]);
        assert_eq!(seat_from(&s, role_named("artist")).provider, Provider::Claude);
    }
}

#[cfg(test)]
mod limit_tests {
    use super::*;

    #[test]
    fn an_advisory_worker_keeps_the_short_default() {
        let l = limits_for(false);
        let d = WorkerLimits::default();
        assert_eq!(l.wall_clock, d.wall_clock);
        assert_eq!(l.stall_timeout, d.stall_timeout);
    }

    #[test]
    fn an_acting_worker_gets_long_enough_to_write_a_test_suite() {
        let acting = limits_for(true);
        let advisory = WorkerLimits::default();

        assert!(
            acting.wall_clock >= std::time::Duration::from_secs(1800),
            "a qa_engineer writing several suites was killed at the ten minute default"
        );
        assert!(acting.wall_clock > advisory.wall_clock);
        assert!(
            acting.stall_timeout > advisory.stall_timeout,
            "a worker thinking between tool calls must not read as hung"
        );
    }
}
