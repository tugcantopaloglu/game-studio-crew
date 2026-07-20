use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use studio_engine::{detect, install_helpers, EngineProfile, VerifyScope};
use studio_verify::{EngineDriver, ProfileDriver, ProjectPaths, RepairLoop, RepairStep, Verdict};

const BROKEN_SCRIPT: &str = r#"extends Node

var speed: float = 100.0
var dash_cooldown: float = 0.5

func dash() -> void:
	if speed > 0
		speed = speed * 2.0
"#;

const GOOD_SCRIPT: &str = r#"extends Node

var speed: float = 100.0

func _ready() -> void:
	pass
"#;

const PROJECT_GODOT: &str = r#"config_version=5

[application]
config/name="StudioM3"
config/features=PackedStringArray("4.2")
"#;

pub fn scaffold(root: &Path) -> Result<()> {
    fs::create_dir_all(root.join("systems"))?;
    fs::write(root.join("project.godot"), PROJECT_GODOT)?;
    fs::write(root.join("player.gd"), GOOD_SCRIPT)?;
    fs::write(root.join("systems/dash.gd"), BROKEN_SCRIPT)?;
    Ok(())
}

pub fn run(
    project_root: PathBuf,
    out_dir: PathBuf,
    spawn_repair: impl Fn(&str, &Path) -> Result<String>,
) -> Result<()> {
    println!("M3 acceptance proof");
    println!("  project       {}", project_root.display());

    if !project_root.join("project.godot").exists() {
        scaffold(&project_root)?;
        println!("  scaffolded a Godot project with one deliberately broken script");
    }

    let profiles = EngineProfile::builtin();
    let detected = detect(&project_root, &profiles);
    let engine_id = match detected.first() {
        Some(d) => d.id.clone(),
        None => bail!("no engine detected at {}", project_root.display()),
    };
    println!("  detected      {engine_id}");

    let profile = profiles
        .iter()
        .find(|p| p.id == engine_id)
        .cloned()
        .context("detected engine has no profile")?;

    let installed = install_helpers(&profile, &project_root)?;
    for p in &installed {
        println!("  helper        {}", p.display());
    }

    let driver = ProfileDriver::resolve(profile).context(
        "could not resolve the engine binary; set GODOT_BIN or put godot on PATH",
    )?;
    println!("  engine binary {}", driver.engine_binary.display());
    println!();

    let paths = ProjectPaths::new(&project_root, &out_dir);
    let mut loop_state = RepairLoop::new();
    let mut failures = Vec::new();

    let first = driver.verify(VerifyScope::Compile, &paths);
    println!(
        "  verify #0     {:?} in {}ms",
        first.verdict, first.duration_ms
    );
    if let Some(r) = &first.inconclusive_reason {
        println!("                reason: {r}");
    }
    for f in &first.failures {
        println!(
            "                {} {}",
            f.file.clone().unwrap_or_default(),
            f.message
        );
    }

    check("a", "verify detected the broken script", first.verdict == Verdict::Fail, &mut failures);
    check(
        "b",
        "the failure names the offending file",
        first
            .failures
            .iter()
            .any(|f| f.file.as_deref().map(|p| p.contains("dash.gd")).unwrap_or(false)),
        &mut failures,
    );

    let mut final_verdict = first.verdict;
    let mut rounds_used = 0u32;

    let mut current = first;
    loop {
        match loop_state.observe(&current) {
            RepairStep::Done => {
                final_verdict = Verdict::Pass;
                break;
            }
            RepairStep::RouteToInfra { reason } => {
                println!("  routed to infra: {reason}");
                break;
            }
            RepairStep::Escalate { rounds_spent, .. } => {
                println!("  escalated after {rounds_spent} rounds");
                rounds_used = rounds_spent;
                break;
            }
            RepairStep::Reinvoke { round, brief, failure_count } => {
                rounds_used = round;
                println!();
                println!("  repair round {round} ({failure_count} failure(s))");
                println!("  brief handed to the worker:");
                for line in brief.lines() {
                    println!("    | {line}");
                }

                let reply = spawn_repair(&brief, &project_root)?;
                println!("  worker replied: {}", reply.lines().next().unwrap_or(""));

                current = driver.verify(VerifyScope::Compile, &paths);
                println!(
                    "  verify #{round}     {:?} in {}ms",
                    current.verdict, current.duration_ms
                );
                for f in &current.failures {
                    println!(
                        "                {} {}",
                        f.file.clone().unwrap_or_default(),
                        f.message
                    );
                }
                final_verdict = current.verdict;
            }
        }
    }

    println!();
    check(
        "c",
        "the repair loop drove the project back to green",
        final_verdict == Verdict::Pass,
        &mut failures,
    );
    check(
        "d",
        "it took fewer than the maximum repair rounds",
        rounds_used < studio_verify::MAX_REPAIR_ROUNDS,
        &mut failures,
    );

    println!();
    println!("  rounds used   {rounds_used}");

    if failures.is_empty() {
        println!();
        println!("M3 PASSED");
        Ok(())
    } else {
        bail!("M3 FAILED: {}", failures.join(", "));
    }
}

fn check(tag: &str, what: &str, ok: bool, failures: &mut Vec<String>) {
    println!("  ({tag}) {:<46} {}", what, if ok { "PASS" } else { "FAIL" });
    if !ok {
        failures.push(tag.to_string());
    }
}
