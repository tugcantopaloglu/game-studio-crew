use anyhow::{Context, Result};
use std::sync::Arc;
use studio_agents::{nearest_common_ancestor, role, Role};
use studio_events::{EventType, Scene};
use studio_server::{AppState, MeetingRequest, StudioCommand, TaskRequest, WorkflowRequest};
use studio_store::Store;

use crate::m4::{run_worker, Emitter};

pub fn run_command(em: &Emitter, cmd: StudioCommand, seq: &mut usize) -> Result<()> {
    match cmd {
        StudioCommand::Task(t) => run_task(em, t, seq),
        StudioCommand::Meeting(m) => run_meeting(em, m, seq),
        StudioCommand::Workflow(w) => run_flow(em, w, seq),
    }
}

fn run_flow(em: &Emitter, req: WorkflowRequest, seq: &mut usize) -> Result<()> {
    let wf = studio_workflow::Workflow::builtin()
        .into_iter()
        .find(|w| w.id == req.workflow)
        .with_context(|| format!("unknown workflow {}", req.workflow))?;

    println!("  workflow {} : {}", wf.id, first_line(&req.brief));
    let project = crate::studio_dir().join("m3-project");
    let project = if project.join("project.godot").exists() { Some(project) } else { None };
    crate::wf::run_workflow(em, &wf, &req.brief, project, seq)?;
    Ok(())
}

fn run_task(em: &Emitter, req: TaskRequest, seq: &mut usize) -> Result<()> {
    let r = role(&req.role).with_context(|| format!("unknown role {}", req.role))?;
    *seq += 1;
    println!("  task -> {} : {}", r.id, first_line(&req.brief));
    run_worker(em, r, &req.brief, *seq)
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

    let mut floor = Vec::new();
    for id in &req.participants {
        let r = match role(id) {
            Some(r) => r,
            None => continue,
        };
        *seq += 1;

        let actor = format!("{}#{}", r.id, seq);
        let scene = Scene::desk(r.department.id(), &actor).in_meeting(&meeting_id);
        em.emit(
            &actor,
            EventType::WorkerStateChanged,
            scene,
            serde_json::json!({"from": "running", "to": "meeting"}),
        )?;

        let brief = format!(
            "You are in a {} meeting about: {}\n\n\
             {}\n\nGive your position in one short sentence. Do not hedge.",
            req.kind,
            req.topic,
            if floor.is_empty() {
                "You speak first.".to_string()
            } else {
                format!("Already said:\n{}", floor.join("\n"))
            }
        );

        run_worker(em, r, &brief, *seq)?;
        floor.push(format!("- {}: (see capsule)", r.id));
    }

    let outcome = if let Some(c) = role(chair) {
        *seq += 1;
        let brief = format!(
            "You chair this {} meeting about: {}\n\n\
             The room has spoken. State the decision in one sentence, then stop.",
            req.kind, req.topic
        );
        run_worker(em, c, &brief, *seq)?;

        em.emit(
            "daemon",
            EventType::DecisionRecorded,
            Scene::daemon().in_meeting(&meeting_id),
            serde_json::json!({
                "decision_id": crate::id("adr"),
                "title": first_line(&req.topic),
                "chair": c.id,
            }),
        )?;
        "decided"
    } else {
        "adjourned"
    };

    em.emit(
        "daemon",
        EventType::MeetingEnded,
        Scene::daemon().in_meeting(&meeting_id),
        serde_json::json!({"meeting_id": meeting_id, "outcome": outcome}),
    )?;

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

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").chars().take(70).collect()
}

pub fn serve_studio(store: Arc<Store>, run: String, port: u16) -> Result<()> {
    let (tx, rx) = std::sync::mpsc::channel::<StudioCommand>();
    let state = AppState::new(store.clone()).with_commands(tx);

    let serve_state = state.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let _ = rt.block_on(studio_server::serve(serve_state, port));
    });

    let em = Emitter { store: store.clone(), state, run: run.clone() };
    em.emit(
        "daemon",
        EventType::RunStarted,
        Scene::daemon(),
        serde_json::json!({"title": "interactive studio"}),
    )?;

    println!("studio floor on http://127.0.0.1:{port}/?run={run}");
    println!("waiting for tasks and meetings from the floor");
    println!();

    let mut seq = 0usize;
    for cmd in rx {
        if let Err(e) = run_command(&em, cmd, &mut seq) {
            println!("  command failed: {e}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::chair_for;

    fn v(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|s| s.to_string()).collect()
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
