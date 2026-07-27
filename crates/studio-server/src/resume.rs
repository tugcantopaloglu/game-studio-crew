use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use studio_workflow::Plan;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Unfinished {
    pub project: String,
    pub title: String,
    pub brief: String,
    pub plan: Plan,
    #[serde(default)]
    pub done: Vec<String>,
    #[serde(default)]
    pub left_at: String,
    #[serde(default)]
    pub why: String,
}

impl Unfinished {
    pub fn left(&self) -> Vec<&str> {
        let done: BTreeSet<&str> = self.done.iter().map(String::as_str).collect();
        self.plan
            .tasks
            .iter()
            .map(|t| t.id.as_str())
            .filter(|id| !done.contains(id))
            .collect()
    }

    pub fn finished(&self) -> bool {
        self.left().is_empty()
    }

    pub fn done_set(&self) -> BTreeSet<String> {
        self.done.iter().cloned().collect()
    }
}

pub fn path_for(studio_dir: &Path, project: &str) -> PathBuf {
    studio_dir.join(format!("unfinished-{}.json", safe(project)))
}

fn safe(project: &str) -> String {
    project
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

pub fn read(studio_dir: &Path, project: &str) -> Option<Unfinished> {
    let text = std::fs::read_to_string(path_for(studio_dir, project)).ok()?;
    let held: Unfinished = serde_json::from_str(&text).ok()?;
    if held.finished() {
        return None;
    }
    Some(held)
}

pub fn write(studio_dir: &Path, held: &Unfinished) -> std::io::Result<()> {
    std::fs::create_dir_all(studio_dir)?;
    let text = serde_json::to_string_pretty(held)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path_for(studio_dir, &held.project), text)
}

pub fn clear(studio_dir: &Path, project: &str) {
    let _ = std::fs::remove_file(path_for(studio_dir, project));
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_workflow::PlanTask;

    fn plan_of(ids: &[&str]) -> Plan {
        Plan {
            title: "Flappy Bird".into(),
            tasks: ids
                .iter()
                .map(|id| PlanTask {
                    id: (*id).into(),
                    role: "gameplay_engineer".into(),
                    brief: format!("do {id}"),
                    depends_on: Vec::new(),
                    say: String::new(),
                })
                .collect(),
        }
    }

    fn half_done() -> Unfinished {
        Unfinished {
            project: "proj_flappy".into(),
            title: "Flappy Bird".into(),
            brief: "build a 3d flappy bird".into(),
            plan: plan_of(&["t1", "t2", "t3", "t4"]),
            done: vec!["t1".into(), "t2".into()],
            left_at: "2026-07-26T19:40:00Z".into(),
            why: "the account is out of allowance".into(),
        }
    }

    #[test]
    fn what_is_left_is_the_plan_minus_what_finished() {
        assert_eq!(half_done().left(), vec!["t3", "t4"]);
    }

    #[test]
    fn a_run_that_stopped_survives_the_daemon_it_was_running_in() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &half_done()).unwrap();

        let back = read(dir.path(), "proj_flappy").expect("a stopped run is offered again");
        assert_eq!(back, half_done());
        assert_eq!(back.done_set().len(), 2);
        assert_eq!(
            back.plan.tasks[2].brief, "do t3",
            "the brief has to come back too, or resuming means re-planning"
        );
    }

    #[test]
    fn a_run_with_nothing_left_is_not_offered_even_if_the_file_is_still_there() {
        let dir = tempfile::tempdir().unwrap();
        let mut all = half_done();
        all.done = vec!["t1".into(), "t2".into(), "t3".into(), "t4".into()];
        write(dir.path(), &all).unwrap();
        assert!(read(dir.path(), "proj_flappy").is_none());
    }

    #[test]
    fn clearing_a_finished_run_leaves_nothing_to_offer() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &half_done()).unwrap();
        clear(dir.path(), "proj_flappy");
        assert!(read(dir.path(), "proj_flappy").is_none());
    }

    #[test]
    fn a_project_id_cannot_walk_out_of_the_studio_directory() {
        let dir = tempfile::tempdir().unwrap();
        let escaping = "../../../etc/passwd";
        let path = path_for(dir.path(), escaping);
        assert_eq!(
            path.parent(),
            Some(dir.path()),
            "a project id reaches this from the network; it may not name the file's folder"
        );
        assert!(!path.to_string_lossy().contains(".."));
    }

    #[test]
    fn nothing_stored_reads_as_nothing_to_resume_rather_than_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(dir.path(), "proj_never_ran").is_none());
    }

    #[test]
    fn a_corrupt_record_is_ignored_rather_than_stopping_the_studio() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(path_for(dir.path(), "proj_broken"), "{not json").unwrap();
        assert!(read(dir.path(), "proj_broken").is_none());
    }
}
