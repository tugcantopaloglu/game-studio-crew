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
    pub project_id: Option<String>,
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

pub fn uncacheable(usage: studio_events::Usage) -> bool {
    usage.total_input() > 0 && usage.cache_creation == 0 && usage.cache_read == 0
}

fn limits_for(acting: bool) -> WorkerLimits {
    if !acting {
        return WorkerLimits::default();
    }
    WorkerLimits {
        stall_timeout: std::time::Duration::from_secs(300),
        wall_clock: std::time::Duration::from_secs(2700),
        stop_asked: None,
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
    pub unusable_model: Option<String>,
}

pub const CLAUDE_MODELS_THE_STUDIO_CAN_EXPRESS: [&str; 4] = ["fable", "opus", "sonnet", "haiku"];

impl Seat {
    pub fn describe(&self) -> String {
        let named = if self.model_alias.is_empty() {
            self.model.cli_alias()
        } else {
            &self.model_alias
        };
        format!("{} {}", self.provider.id(), named)
    }
}

fn first_line(state: &studio_core::StreamStateSnapshot) -> String {
    let spoken = state
        .result_message
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .unwrap_or(&state.text);
    spoken
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("no output")
        .to_string()
}

fn model_named(alias: &str) -> Option<Model> {
    let alias = alias.trim();
    if let Some(exact) = Model::ALL.into_iter().find(|m| m.cli_alias() == alias) {
        return Some(exact);
    }
    for (family, model) in [
        ("claude-fable", Model::Fable),
        ("claude-opus", Model::Opus),
        ("claude-sonnet", Model::Sonnet),
        ("claude-haiku", Model::Haiku),
    ] {
        if alias.starts_with(family) {
            return Some(model);
        }
    }
    None
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

    let shipped = (role.model, role.model.cli_alias().to_string(), None);
    let (model, model_alias, unusable_model) = match provider {
        Provider::Claude => match chosen_model.as_deref() {
            None => shipped,
            Some(named) => match model_named(named) {
                Some(m) => (m, m.cli_alias().to_string(), None),
                None => (role.model, role.model.cli_alias().to_string(), Some(named.to_string())),
            },
        },
        _ => (role.model, chosen_model.clone().unwrap_or_default(), None),
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
        unusable_model,
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

struct Desk<'a> {
    em: &'a Emitter,
    task: String,
    actor: String,
    scene: Scene,
    lit: bool,
    settled: bool,
}

impl Desk<'_> {
    fn settle(&mut self) {
        self.settled = true;
    }
}

impl Drop for Desk<'_> {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        let _ = self.em.store.update_task_state(
            &self.task,
            WorkerState::Reaped,
            Some(Outcome::Crashed),
            crate::now(),
        );
        if self.lit {
            let _ = self.em.emit(
                &self.actor,
                EventType::WorkerExited,
                self.scene.clone(),
                serde_json::json!({"outcome": "crashed", "exit_code": null}),
            );
        }
    }
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
    if let Some(named) = &seat.unusable_model {
        anyhow::bail!(
            "{} is set to the claude model {named}, which the studio cannot express. \
             The prefix cache is keyed on the model, so the studio only spawns models it can name in its own hash, and it knows {}. \
             Change that seat in settings, or widen studio_context::Model before using {named}.",
            role.id,
            CLAUDE_MODELS_THE_STUDIO_CAN_EXPRESS.join(", ")
        );
    }
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

    let mut desk = Desk {
        em,
        task: task_id.clone(),
        actor: actor.clone(),
        scene: Scene::desk(role.department.id(), &actor),
        lit: false,
        settled: false,
    };

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
    desk.lit = true;
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
        BriefDelivery::Positional => {
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
    let limits = limits_for(acting).until(em.state.stop_asked());
    let report = worker.drive(&limits, |ev| {
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

    if uncacheable(usage) {
        println!(
            "  {} cached nothing: {} estimated tokens against a {} floor for {}",
            role.id,
            prefix.estimated_tokens,
            seat.model.documented_min_cacheable_tokens(),
            seat.model.cli_alias()
        );
        em.emit(
            &actor,
            EventType::BudgetWarning,
            scene.clone(),
            serde_json::json!({
                "reason": "the prefix neither wrote nor read cache; it is under the model's minimum cacheable length",
                "role": role.id,
                "prefix_hash": prefix.prefix_hash,
                "estimated_tokens": prefix.estimated_tokens,
                "documented_min_cacheable_tokens": seat.model.documented_min_cacheable_tokens(),
                "model": seat.model.cli_alias(),
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

    desk.settle();
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
            "{} on {} did not complete ({:?}): {}",
            role.id,
            seat.describe(),
            report.outcome,
            first_line(&report.state)
        );
    }

    if report.state.is_error {
        anyhow::bail!("{} on {} failed: {}", role.id, seat.describe(), first_line(&report.state));
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
    fn a_claude_model_the_studio_cannot_express_is_flagged_rather_than_quietly_replaced() {
        let s = settings(&[("models.role.artist", "clyde")]);
        let seat = seat_from(&s, role_named("artist"));
        assert_eq!(
            seat.unusable_model.as_deref(),
            Some("clyde"),
            "falling back to opus would run a model the user did not ask for and never say so"
        );
    }

    #[test]
    fn sonnet_can_now_be_chosen_because_the_cli_has_always_accepted_it() {
        let s = settings(&[("models.role.artist", "sonnet")]);
        let seat = seat_from(&s, role_named("artist"));
        assert_eq!(seat.model, Model::Sonnet);
        assert_eq!(seat.model_alias, "sonnet");
        assert_eq!(seat.unusable_model, None);
    }

    #[test]
    fn widening_the_enum_moved_no_role_off_the_model_it_ships_on() {
        for r in &REGISTRY {
            assert_ne!(
                r.model,
                Model::Sonnet,
                "{} adopted sonnet by default; it is offered, not assigned",
                r.id
            );
        }
        let on_fable: Vec<&str> =
            REGISTRY.iter().filter(|r| r.model == Model::Fable).map(|r| r.id).collect();
        assert_eq!(on_fable, vec!["studio_director"]);
    }

    #[test]
    fn every_model_the_studio_can_express_actually_resolves() {
        for alias in CLAUDE_MODELS_THE_STUDIO_CAN_EXPRESS {
            let s = settings(&[("models.role.artist", alias)]);
            let seat = seat_from(&s, role_named("artist"));
            assert_eq!(seat.unusable_model, None, "{alias} is advertised but does not resolve");
            assert_eq!(seat.model_alias, alias);
        }
    }

    #[test]
    fn a_full_model_name_resolves_to_the_family_the_cache_key_is_built_from() {
        for (typed, expected) in [
            ("claude-opus-5", Model::Opus),
            ("claude-fable-5", Model::Fable),
            ("claude-sonnet-5", Model::Sonnet),
            ("claude-haiku-4-5-20251001", Model::Haiku),
        ] {
            let s = settings(&[("models.role.artist", typed)]);
            let seat = seat_from(&s, role_named("artist"));
            assert_eq!(seat.model, expected, "{typed} should resolve");
            assert_eq!(seat.unusable_model, None);
        }
    }

    #[test]
    fn a_seat_names_the_provider_and_the_model_so_a_refusal_is_not_a_generic_failure() {
        let s = settings(&[
            ("provider.role.artist", "codex"),
            ("models.codex.role.artist", "gpt-5.2-codex"),
        ]);
        assert_eq!(seat_from(&s, role_named("artist")).describe(), "codex gpt-5.2-codex");
        assert_eq!(seat_from(&Settings::new(), role_named("artist")).describe(), "claude opus");
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
mod uncacheable_tests {
    use super::uncacheable;
    use studio_events::Usage;

    #[test]
    fn a_spawn_that_neither_wrote_nor_read_cache_is_reported_rather_than_read_as_no_data() {
        assert!(uncacheable(Usage { input: 900, output: 20, cache_read: 0, cache_creation: 0 }));
    }

    #[test]
    fn a_cold_write_is_not_a_failure() {
        assert!(!uncacheable(Usage { input: 2, output: 4, cache_read: 0, cache_creation: 926 }));
    }

    #[test]
    fn a_warm_read_is_not_a_failure() {
        assert!(!uncacheable(Usage { input: 2, output: 4, cache_read: 926, cache_creation: 0 }));
    }

    #[test]
    fn a_worker_that_billed_nothing_at_all_is_a_dead_spawn_not_an_uncacheable_prefix() {
        assert!(
            !uncacheable(Usage::default()),
            "a refused or crashed worker reports no usage; blaming the prefix for that would \
             point every investigation at the wrong thing"
        );
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

#[cfg(test)]
mod desk_tests {
    use super::*;
    use std::sync::Arc;
    use studio_server::AppState;

    fn emitter(tag: &str) -> (tempfile::TempDir, Emitter) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(studio_store::Store::open(dir.path().join("s.db")).unwrap());
        register_roles(&store).unwrap();
        let state = AppState::new(store.clone()).with_studio_dir(dir.path().to_path_buf());
        let em = Emitter {
            store,
            state,
            run: format!("run_{tag}"),
            project: None,
            project_id: None,
        };
        (dir, em)
    }

    fn queued(em: &Emitter, task: &str) {
        em.store
            .insert_task(
                studio_store::TaskRow {
                    id: task.into(),
                    run: em.run.clone(),
                    role: "artist".into(),
                    parent_task: None,
                    workflow_node: None,
                    state: WorkerState::Queued,
                    outcome: None,
                },
                crate::now(),
            )
            .unwrap();
    }

    fn exits_in(em: &Emitter) -> usize {
        em.store
            .events_between(&em.run, 0, 100)
            .unwrap()
            .iter()
            .filter(|e| e.event_type == EventType::WorkerExited)
            .count()
    }

    #[test]
    fn a_worker_that_never_started_still_lets_go_of_its_desk() {
        let (_d, em) = emitter("unlit");
        queued(&em, "task_unlit");
        {
            let _desk = Desk {
                em: &em,
                task: "task_unlit".into(),
                actor: "artist#1".into(),
                scene: Scene::desk("art", "artist#1"),
                lit: false,
                settled: false,
            };
        }
        assert_eq!(
            exits_in(&em),
            0,
            "the floor never drew this worker, so telling it the worker exited would light a              desk that was never lit"
        );
    }

    #[test]
    fn a_spawn_that_fails_after_the_floor_drew_the_worker_clears_it_again() {
        let (_d, em) = emitter("lit");
        queued(&em, "task_lit");
        {
            let _desk = Desk {
                em: &em,
                task: "task_lit".into(),
                actor: "artist#1".into(),
                scene: Scene::desk("art", "artist#1"),
                lit: true,
                settled: false,
            };
        }
        assert_eq!(
            exits_in(&em),
            1,
            "worker_spawned lights the desk and only worker_exited clears it; an error between              the two used to leave that crew member working for the rest of the session"
        );
    }

    #[test]
    fn a_worker_that_reported_its_own_outcome_is_not_reported_twice() {
        let (_d, em) = emitter("settled");
        queued(&em, "task_settled");
        {
            let mut desk = Desk {
                em: &em,
                task: "task_settled".into(),
                actor: "artist#1".into(),
                scene: Scene::desk("art", "artist#1"),
                lit: true,
                settled: false,
            };
            desk.settle();
        }
        assert_eq!(exits_in(&em), 0);
    }
}
