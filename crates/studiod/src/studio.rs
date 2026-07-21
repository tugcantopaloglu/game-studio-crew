use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
use studio_agents::{nearest_common_ancestor, role};
use studio_events::{EventType, Scene};
use studio_server::{
    AppState, BuildRequest, MeetingRequest, StudioCommand, TaskRequest, WorkflowRequest,
};
use studio_store::Store;

use crate::m4::{run_worker, Emitter};

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
        Ok(())
    }
}

pub fn run_command(em: &Emitter, cmd: StudioCommand, seq: &mut usize) -> Result<()> {
    match cmd {
        StudioCommand::Task(t) => run_task(em, t, seq),
        StudioCommand::Meeting(m) => run_meeting(em, m, seq),
        StudioCommand::Workflow(w) => run_flow(em, w, seq),
        StudioCommand::Build(b) => run_build(em, b, seq),
    }
}

fn run_build(em: &Emitter, req: BuildRequest, seq: &mut usize) -> Result<()> {
    println!("  build: {}", first_line(&req.prompt));

    let director = role("studio_director").context("the director is missing from the registry")?;
    let schema = studio_workflow::plan_schema().to_string();

    let brief = format!(
        "A request has come in: {}\n\n         Decompose it into studio tasks. Give each task the role that should do it, \n         a brief detailed enough that the role needs no further decisions, and the ids \n         of the tasks whose output it needs. Keep the graph as small as the work allows. \n         Do not invent roles.",
        req.prompt.trim()
    );

    *seq += 1;
    let raw = crate::m4::run_worker_capturing(em, director, &brief, *seq, Some(schema))?;

    let cleaned = extract_json(&raw);
    let plan = studio_workflow::Plan::parse(&cleaned)
        .map_err(|e| anyhow::anyhow!("the director returned a plan I cannot run: {e}"))?;

    println!("  plan '{}' with {} tasks:", plan.title, plan.tasks.len());
    for t in &plan.tasks {
        println!(
            "    {:<6} {:<20} deps={:?}",
            t.id, t.role, t.depends_on
        );
    }

    let wf = plan
        .to_workflow()
        .map_err(|e| anyhow::anyhow!("plan did not convert to a workflow: {e}"))?;

    let project = crate::studio_dir().join("m3-project");
    let project = if project.join("project.godot").exists() { Some(project) } else { None };

    crate::wf::run_planned(em, &wf, &req.prompt, project, seq, Some(plan))?;
    Ok(())
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

    let mut project = ProjectIndex::open(
        PathBuf::from("."),
        crate::studio_dir().join("studio-index.db"),
    )?;
    project.refresh(&em)?;

    println!("studio floor on http://127.0.0.1:{port}/?run={run}");
    println!("waiting for tasks and meetings from the floor");
    println!();

    let mut seq = 0usize;
    for cmd in rx {
        project.refresh_quietly(&em);
        if let Err(e) = run_command(&em, cmd, &mut seq) {
            println!("  command failed: {e}");
        }
        project.refresh_quietly(&em);
    }
    Ok(())
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
