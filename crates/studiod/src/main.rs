mod charters;

use anyhow::{bail, Context, Result};
use std::fs;
use std::path::PathBuf;
use studio_context::{freeze, FrozenPrefix, Model};
use studio_core::{Effort, SessionMode, Worker, WorkerLimits, WorkerSpec};
use studio_events::{EventType, Outcome, Scene, WorkerState};
use studio_store::{LedgerEntry, RoleRow, SessionRow, Store, TaskRow};

const ROLE: &str = "m1_probe";
const TOOLS: [&str; 3] = ["Read", "Grep", "Glob"];

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

fn id(prefix: &str) -> String {
    format!("{prefix}_{}", ulid::Ulid::new())
}

fn studio_dir() -> PathBuf {
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
    report("a", "usage captured from the terminal result", usage_ok, &mut failures);

    let cache_ok = cold.usage.cache_creation > 0 && warm.usage.cache_read > 0;
    report("b", "second same-prefix spawn read from cache", cache_ok, &mut failures);

    let reap_ok = cold.session_id.is_some() && warm.session_id.is_some();
    report("c", "workers reaped cleanly with session ids recorded", reap_ok, &mut failures);

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

fn report(tag: &str, what: &str, ok: bool, failures: &mut Vec<String>) {
    println!("  ({tag}) {:<48} {}", what, if ok { "PASS" } else { "FAIL" });
    if !ok {
        failures.push(tag.to_string());
    }
}

fn main() -> Result<()> {
    let cmd = std::env::args().nth(1).unwrap_or_else(|| "help".into());
    match cmd.as_str() {
        "m1" => m1_proof(),
        _ => {
            println!("usage: studiod m1");
            println!();
            println!("  m1   run the M1 acceptance proof: spawn two same-prefix workers,");
            println!("       record the ledger, and verify usage capture, cache reuse and reaping.");
            Ok(())
        }
    }
}
