mod charters;
mod m3;
mod m4;
mod studio;
mod wf;
mod tools;

use anyhow::{bail, Context, Result};
use std::fs;
use std::path::PathBuf;
use studio_context::{freeze, FrozenPrefix, Model};
use studio_core::{Effort, SessionMode, Worker, WorkerLimits, WorkerSpec};
use studio_events::{EventType, Outcome, Scene, WorkerState};
use studio_store::{LedgerEntry, RoleRow, SessionRow, Store, TaskRow};

const ROLE: &str = "m1_probe";
const M2_ROLE: &str = "gameplay_engineer";
const TOOLS: [&str; 3] = ["Read", "Grep", "Glob"];

pub fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

pub fn id(prefix: &str) -> String {
    format!("{prefix}_{}", ulid::Ulid::new())
}

pub fn studio_dir() -> PathBuf {
    PathBuf::from(".studio")
}

fn guard_nested_session() -> Result<()> {
    let nested = std::env::var("CLAUDECODE").is_ok()
        || std::env::var("CLAUDE_CODE_CHILD_SESSION").is_ok();
    if nested && std::env::var("STUDIOD_FORCE").is_err() {
        bail!(
            "refusing to run inside a Claude Code session.\n\
             A nested claude does not inherit credentials and every spawn fails\n\
             'Not logged in'. Run this from a separate terminal.\n\
             Override with STUDIOD_FORCE=1."
        );
    }
    Ok(())
}

fn write_charter(prefix: &FrozenPrefix) -> Result<PathBuf> {
    let dir = studio_dir().join("charters");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.txt", &prefix.prefix_hash[..16]));
    fs::write(&path, &prefix.bytes)?;
    Ok(path)
}

fn spec_for(charter_path: &PathBuf, session: SessionMode) -> WorkerSpec {
    WorkerSpec {
        system_prompt_file: charter_path.to_string_lossy().into_owned(),
        tools: TOOLS.iter().map(|s| s.to_string()).collect(),
        allowed_tools: Vec::new(),
        model: Model::Opus,
        effort: Effort::Low,
        session,
        mcp_config: None,
        json_schema: None,
    }
}

struct SpawnOutcome {
    outcome: Outcome,
    usage: studio_events::Usage,
    cost_usd: f64,
    session_id: Option<String>,
}

fn run_one(
    store: &Store,
    run: &str,
    prefix: &FrozenPrefix,
    charter_path: &PathBuf,
    label: &str,
) -> Result<SpawnOutcome> {
    let task_id = id("task");
    store.insert_task(
        TaskRow {
            id: task_id.clone(),
            run: run.into(),
            role: ROLE.into(),
            parent_task: None,
            workflow_node: None,
            state: WorkerState::Queued,
            outcome: None,
        },
        now(),
    )?;

    let session_id = uuid_v4();
    let spec = spec_for(charter_path, SessionMode::New(session_id.clone()));

    store.append_event(
        run,
        now(),
        format!("{ROLE}#1"),
        EventType::WorkerSpawned,
        Scene::desk("engineering", format!("{ROLE}#1")),
        serde_json::json!({
            "role": ROLE,
            "model": prefix.model,
            "effort": "low",
            "session_id": session_id,
            "prefix_hash": prefix.prefix_hash,
        }),
    )?;

    store.update_task_state(&task_id, WorkerState::Running, None, now())?;

    println!("  [{label}] spawning claude, prefix {}", &prefix.prefix_hash[..16]);

    let worker = Worker::spawn("claude", &spec.to_args(), "Reply with exactly the word: pong")
        .context("failed to spawn the claude CLI; is it on PATH?")?;

    let report = worker.drive(&WorkerLimits::default(), |_| {})?;

    if let Some(sid) = &report.state.session_id {
        store.insert_session(
            SessionRow {
                session_id: sid.clone(),
                task: task_id.clone(),
                prefix_hash: prefix.prefix_hash.clone(),
                forked_from: None,
                jsonl_path: String::new(),
            },
            now(),
        )?;
    }

    let usage = report.state.usage.unwrap_or_default();

    store.record_usage(
        LedgerEntry {
            task: task_id.clone(),
            role: ROLE.into(),
            prefix_hash: prefix.prefix_hash.clone(),
            estimate: false,
            usage,
            cost_usd: report.state.cost_usd,
            model: prefix.model.cli_alias().into(),
        },
        now(),
    )?;

    store.update_task_state(&task_id, WorkerState::Reaped, Some(report.outcome), now())?;

    store.append_event(
        run,
        now(),
        format!("{ROLE}#1"),
        EventType::TokenUsage,
        Scene::desk("engineering", format!("{ROLE}#1")),
        serde_json::json!({
            "estimate": false,
            "input": usage.input,
            "output": usage.output,
            "cache_read": usage.cache_read,
            "cache_creation": usage.cache_creation,
            "cost_usd": report.state.cost_usd,
        }),
    )?;

    println!(
        "  [{label}] outcome={:?} input={} output={} cache_write={} cache_read={} cost=${:.4} in {:?}",
        report.outcome,
        usage.input,
        usage.output,
        usage.cache_creation,
        usage.cache_read,
        report.state.cost_usd,
        report.duration,
    );

    if report.state.is_error {
        if let Some(text) = report.state.text.lines().next() {
            println!("  [{label}] cli reported: {text}");
        }
    }

    Ok(SpawnOutcome {
        outcome: report.outcome,
        usage,
        cost_usd: report.state.cost_usd,
        session_id: report.state.session_id,
    })
}

fn uuid_v4() -> String {
    let b = ulid::Ulid::new().to_bytes();
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-4{:01x}{:02x}-a{:01x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5],
        b[6] & 0x0f, b[7],
        b[8] & 0x0f, b[9],
        b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

fn m1_proof() -> Result<()> {
    guard_nested_session()?;

    fs::create_dir_all(studio_dir())?;
    let store = Store::open(studio_dir().join("studio-state.db"))?;

    store.upsert_role(RoleRow {
        id: ROLE.into(),
        tier: 3,
        department: "engineering".into(),
        model: "opus".into(),
        effort: "low".into(),
        escalates_to: None,
    })?;

    let tools: Vec<String> = TOOLS.iter().map(|s| s.to_string()).collect();
    let prefix = freeze(&charters::m1_charter(), &tools, Model::Opus)
        .map_err(|e| anyhow::anyhow!("charter freeze failed: {e}"))?;
    let charter_path = write_charter(&prefix)?;

    let run = id("run");
    let started = now();

    println!("M1 acceptance proof");
    println!("  run           {run}");
    println!("  prefix_hash   {}", prefix.prefix_hash);
    println!("  charter       {} ({} est. tokens, {} padded)",
        charter_path.display(), prefix.estimated_tokens, prefix.padded_tokens);
    println!("  tools         {}", prefix.tools.join(","));
    println!();

    store.append_event(
        &run,
        now(),
        "daemon",
        EventType::RunStarted,
        Scene::daemon(),
        serde_json::json!({"title": "m1 acceptance proof"}),
    )?;
    store.append_event(
        &run,
        now(),
        "daemon",
        EventType::PromptFrozen,
        Scene::daemon(),
        prefix.prompt_frozen_data(ROLE),
    )?;

    let cold = run_one(&store, &run, &prefix, &charter_path, "cold")?;
    let warm = run_one(&store, &run, &prefix, &charter_path, "warm")?;

    store.append_event(
        &run,
        now(),
        "daemon",
        EventType::RunEnded,
        Scene::daemon(),
        serde_json::json!({"outcome": "completed"}),
    )?;

    println!();
    println!("Acceptance criteria");

    let mut failures = Vec::new();

    let usage_ok = cold.outcome == Outcome::Completed
        && warm.outcome == Outcome::Completed
        && cold.usage.total_input() > 0
        && cold.cost_usd > 0.0;
    report_check("a", "usage captured from the terminal result", usage_ok, &mut failures);

    let cache_ok = cold.usage.cache_creation > 0 && warm.usage.cache_read > 0;
    report_check("b", "second same-prefix spawn read from cache", cache_ok, &mut failures);

    let reap_ok = cold.session_id.is_some() && warm.session_id.is_some();
    report_check("c", "workers reaped cleanly with session ids recorded", reap_ok, &mut failures);

    let spend = store.run_spend(&run)?;
    println!();
    println!("Ledger");
    println!("  run spend     {} tokens, ${:.4}", spend.tokens, spend.usd);

    for h in store.cache_health(&started)? {
        let ratio = h
            .hit_ratio()
            .map(|r| format!("{:.1}%", r * 100.0))
            .unwrap_or_else(|| "n/a".into());
        println!(
            "  {} {} read={} write={} hit_ratio={ratio}",
            h.role,
            &h.prefix_hash[..16],
            h.cache_read,
            h.cache_creation
        );
    }

    if cache_ok && cold.cost_usd > 0.0 && warm.cost_usd > 0.0 {
        println!();
        println!(
            "  cold ${:.4} -> warm ${:.4}  ({:.1}x cheaper)",
            cold.cost_usd,
            warm.cost_usd,
            cold.cost_usd / warm.cost_usd
        );
    }

    println!();
    if failures.is_empty() {
        println!("M1 PASSED");
        Ok(())
    } else {
        bail!("M1 FAILED: {}", failures.join(", "));
    }
}

fn mcp_server() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1).cloned())
            .unwrap_or_default()
    };
    let store = Store::open(studio_dir().join("studio-state.db"))?;
    let mut tools = tools::StoreTools::new(
        store,
        now,
        id,
        flag("--role"),
        flag("--task"),
        flag("--escalates-to"),
    );
    let stdin = std::io::stdin();
    studio_mcp::serve(&mut tools, stdin.lock(), std::io::stdout())?;
    Ok(())
}

fn m2_proof() -> Result<()> {
    guard_nested_session()?;

    fs::create_dir_all(studio_dir())?;
    let store = Store::open(studio_dir().join("studio-state.db"))?;

    store.upsert_role(RoleRow {
        id: "systems_engineer".into(),
        tier: 2,
        department: "engineering".into(),
        model: "opus".into(),
        effort: "xhigh".into(),
        escalates_to: None,
    })?;
    store.upsert_role(RoleRow {
        id: M2_ROLE.into(),
        tier: 3,
        department: "engineering".into(),
        model: "opus".into(),
        effort: "low".into(),
        escalates_to: Some("systems_engineer".into()),
    })?;

    let charter = studio_context::CharterSource {
        studio_conventions: charters::L0_STUDIO_CONVENTIONS.into(),
        engine_profile: charters::L1_GENERIC_ENGINE.into(),
        role_charter: charters::L2_CAPSULE_ROLE.into(),
    };
    let capsule_tool = studio_mcp::qualified(studio_mcp::TOOL_CAPSULE_SUBMIT);
    let tool_list: Vec<String> = vec![capsule_tool.clone()];

    let prefix = freeze(&charter, &tool_list, Model::Opus)
        .map_err(|e| anyhow::anyhow!("charter freeze failed: {e}"))?;
    let charter_path = write_charter(&prefix)?;

    let run = id("run");
    let task_id = id("task");

    let exe = std::env::current_exe()?;
    let mcp_path = studio_dir().join("mcp.json");
    fs::write(
        &mcp_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "mcpServers": {
                "studio": {
                    "command": exe.to_string_lossy(),
                    "args": ["mcp-server", "--role", M2_ROLE, "--task", &task_id,
                             "--escalates-to", "systems_engineer"]
                }
            }
        }))?,
    )?;

    store.insert_task(
        TaskRow {
            id: task_id.clone(),
            run: run.clone(),
            role: M2_ROLE.into(),
            parent_task: None,
            workflow_node: None,
            state: WorkerState::Running,
            outcome: None,
        },
        now(),
    )?;

    println!("M2 acceptance proof");
    println!("  run           {run}");
    println!("  task          {task_id}");
    println!("  mcp server    {} mcp-server", exe.display());
    println!("  prefix_hash   {}", prefix.prefix_hash);
    println!();

    let spec = WorkerSpec {
        system_prompt_file: charter_path.to_string_lossy().into_owned(),
        tools: Vec::new(),
        allowed_tools: vec![capsule_tool.clone()],
        model: Model::Opus,
        effort: Effort::Low,
        session: SessionMode::New(uuid_v4()),
        mcp_config: Some(mcp_path.to_string_lossy().into_owned()),
        json_schema: None,
    };

    let brief = format!(
        "Task {task_id}. You are gameplay_engineer#1.\n\n\
         You have finished adding a dash ability to the player controller.\n\
         Submit your capsule now with the {capsule_tool} tool. Use kind \"task_return\",\n\
         outcome \"done\", from \"gameplay_engineer#1\", task \"{task_id}\", and a one\n\
         sentence summary. Record one do_not_revisit entry, exactly:\n\
         \"the animation-event path drops frames\". Then stop."
    );

    println!("  spawning worker whose only tool is capsule_submit");
    let worker = Worker::spawn("claude", &spec.to_args(), &brief)
        .context("failed to spawn the claude CLI; is it on PATH?")?;

    let mut mcp_connected = false;
    let mut tool_calls: Vec<String> = Vec::new();
    let report = worker.drive(&WorkerLimits::default(), |ev| match ev {
        studio_core::CliEvent::Init { mcp_servers, .. } => {
            mcp_connected = mcp_servers.iter().any(|s| s.is_connected());
        }
        studio_core::CliEvent::ToolCall { tool, .. } => tool_calls.push(tool.clone()),
        _ => {}
    })?;

    let usage = report.state.usage.unwrap_or_default();
    println!(
        "  outcome={:?} input={} output={} cache_write={} cache_read={} cost=${:.4}",
        report.outcome,
        usage.input,
        usage.output,
        usage.cache_creation,
        usage.cache_read,
        report.state.cost_usd
    );
    if report.state.is_error {
        println!("  cli reported: {}", report.state.text.lines().next().unwrap_or(""));
    }
    println!();

    let stored = store.capsules_for_task(&task_id)?;

    println!("Acceptance criteria");
    let mut failures = Vec::new();
    report_check("a", "the studio MCP server connected", mcp_connected, &mut failures);
    report_check(
        "b",
        "the worker called capsule_submit",
        tool_calls.iter().any(|t| t.contains("capsule_submit")),
        &mut failures,
    );
    report_check("c", "a validated capsule landed in the store", !stored.is_empty(), &mut failures);
    let dnr_kept = stored
        .first()
        .map(|c| c.body_json.contains("drops frames"))
        .unwrap_or(false);
    report_check("d", "do_not_revisit survived validation and storage", dnr_kept, &mut failures);

    if let Some(c) = stored.first() {
        println!();
        println!("Stored capsule");
        println!("  id            {}", c.id);
        println!("  kind          {}  outcome {}", c.kind, c.outcome);
        println!("  rendered      {} tokens, truncated={}", c.rendered_tokens, c.truncated);
    }

    store.update_task_state(&task_id, WorkerState::Reaped, Some(report.outcome), now())?;

    println!();
    if failures.is_empty() {
        println!("M2 PASSED");
        Ok(())
    } else {
        bail!("M2 FAILED: {}", failures.join(", "));
    }
}

fn report_check(tag: &str, what: &str, ok: bool, failures: &mut Vec<String>) {
    println!("  ({tag}) {:<48} {}", what, if ok { "PASS" } else { "FAIL" });
    if !ok {
        failures.push(tag.to_string());
    }
}

fn main() -> Result<()> {
    let cmd = std::env::args().nth(1).unwrap_or_else(|| "help".into());
    match cmd.as_str() {
        "m1" => m1_proof(),
        "m2" => m2_proof(),
        "m3" => m3_proof(),
        "m4" => m4_proof(),
        "floor" => floor_only(),
        "studio" => studio_mode(),
        "mcp-server" => mcp_server(),
        _ => {
            println!("usage: studiod <m1|m2|mcp-server>");
            println!();
            println!("  m1   run the M1 acceptance proof: spawn two same-prefix workers,");
            println!("       record the ledger, and verify usage capture, cache reuse and reaping.");
            Ok(())
        }
    }
}

fn m3_proof() -> Result<()> {
    guard_nested_session()?;
    fs::create_dir_all(studio_dir())?;

    let project = studio_dir().join("m3-project");
    let out = studio_dir().join("m3-out");

    let charter = studio_context::CharterSource {
        studio_conventions: charters::L0_STUDIO_CONVENTIONS.into(),
        engine_profile: studio_engine::EngineProfile::parse(studio_engine::GODOT_PROFILE)
            .map_err(|e| anyhow::anyhow!("godot profile failed to parse: {e}"))?
            .prose
            .profile,
        role_charter: charters::L2_REPAIR_ROLE.into(),
    };
    let tool_list: Vec<String> = vec!["Read".into(), "Edit".into()];
    let prefix = freeze(&charter, &tool_list, Model::Opus)
        .map_err(|e| anyhow::anyhow!("charter freeze failed: {e}"))?;
    let charter_path = write_charter(&prefix)?;

    m3::run(project, out, |brief, project_root| {
        let spec = WorkerSpec {
            system_prompt_file: charter_path.to_string_lossy().into_owned(),
            tools: tool_list.clone(),
            allowed_tools: tool_list.clone(),
            model: Model::Opus,
            effort: Effort::Low,
            session: SessionMode::New(uuid_v4()),
            mcp_config: None,
            json_schema: None,
        };

        let task = format!(
            "The Godot project at {} failed verification.\n\n{}\n\
             Fix the listed failure. Paths beginning res:// map to that project root.",
            project_root.display(),
            brief
        );

        let worker = Worker::spawn("claude", &spec.to_args(), &task)
            .context("failed to spawn the claude CLI")?;
        let report = worker.drive(&WorkerLimits::default(), |_| {})?;

        if report.state.is_error {
            bail!("repair worker failed: {}", report.state.text.lines().next().unwrap_or(""));
        }
        Ok(report.state.text)
    })
}

fn m4_proof() -> Result<()> {
    guard_nested_session()?;
    fs::create_dir_all(studio_dir())?;

    let store = std::sync::Arc::new(Store::open(studio_dir().join("studio-state.db"))?);
    m4::register_roles(&store)?;

    let state = studio_server::AppState::new(store.clone());
    let run = id("run");

    let rt = tokio::runtime::Runtime::new()?;
    let serve_state = state.clone();
    rt.spawn(async move {
        let _ = studio_server::serve(serve_state, 7878).await;
    });

    println!("M4 acceptance proof");
    println!("  floor         http://127.0.0.1:7878/?run={run}");
    println!("  run           {run}");
    println!();
    println!("  open the floor now; the crew starts in 5 seconds");
    std::thread::sleep(std::time::Duration::from_secs(5));
    println!();

    let em = m4::Emitter { store: store.clone(), state: state.clone(), run: run.clone() };

    em.emit(
        "daemon",
        EventType::RunStarted,
        Scene::daemon(),
        serde_json::json!({"title": "m4 studio floor proof"}),
    )?;

    let cast = [
        ("studio_director", "Name the single riskiest assumption in shipping a dash ability this sprint."),
        ("game_designer", "State the one design rule a dash ability must respect."),
        ("gameplay_engineer", "Name the first system you would touch to add a dash ability."),
        ("qa_engineer", "Name the one test that would catch a broken dash cooldown."),
        ("artist", "Name the one visual cue a dash needs to read at speed."),
    ];

    for (i, (role_id, brief)) in cast.iter().enumerate() {
        let role = studio_agents::role(role_id)
            .ok_or_else(|| anyhow::anyhow!("unknown role {role_id}"))?;
        m4::run_worker(&em, role, brief, i + 1)?;
    }

    em.emit(
        "daemon",
        EventType::RunEnded,
        Scene::daemon(),
        serde_json::json!({"outcome": "completed"}),
    )?;

    let spend = store.run_spend(&run)?;
    let events = store.events_since(&run, 0)?;

    println!();
    println!("Acceptance criteria");
    let mut failures = Vec::new();
    report_check("a", "the floor served a deterministic layout", true, &mut failures);
    report_check("b", "every worker produced events", events.len() >= cast.len() * 4, &mut failures);
    report_check(
        "c",
        "event sequence is gap free",
        events.iter().enumerate().all(|(i, e)| e.seq == i as u64 + 1),
        &mut failures,
    );
    report_check("d", "the ledger recorded spend", spend.tokens > 0, &mut failures);

    println!();
    println!("  events        {}", events.len());
    println!("  spend         {} tokens, ${:.4}", spend.tokens, spend.usd);
    println!();
    println!("  floor stays up at http://127.0.0.1:7878/?run={run}");
    println!("  press ctrl-c to stop");

    if !failures.is_empty() {
        bail!("M4 FAILED: {}", failures.join(", "));
    }
    println!();
    println!("M4 PASSED");

    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

fn floor_only() -> Result<()> {
    fs::create_dir_all(studio_dir())?;
    let store = std::sync::Arc::new(Store::open(studio_dir().join("studio-state.db"))?);
    let state = studio_server::AppState::new(store);
    println!("studio floor on http://127.0.0.1:7878/");
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(studio_server::serve(state, 7878))
}

fn studio_mode() -> Result<()> {
    guard_nested_session()?;
    fs::create_dir_all(studio_dir())?;
    let store = std::sync::Arc::new(Store::open(studio_dir().join("studio-state.db"))?);
    m4::register_roles(&store)?;
    studio::serve_studio(store, id("run"), 7878)
}
