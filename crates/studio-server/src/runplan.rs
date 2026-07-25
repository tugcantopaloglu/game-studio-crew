use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use serde::Deserialize;
use studio_workflow::StepEdit;

use crate::{AppState, BuildRequest, Interrupt, PlanVerdict, StepVerdict, StudioCommand};

pub const MIN_BRIEF: usize = 8;

#[derive(Debug, Clone, Deserialize)]
pub struct GuidedRun {
    pub project: String,
    pub prompt: String,
    #[serde(default)]
    pub step_confirm: bool,
    #[serde(default)]
    pub ask_above: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StartPlan {
    pub plan_id: String,
    #[serde(default)]
    pub steps: Vec<StepEdit>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CancelPlan {
    pub plan_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StepReply {
    pub approval_id: String,
    pub verdict: String,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InterruptRequest {
    #[serde(default)]
    pub stop: bool,
    #[serde(default)]
    pub note: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/run/plan", post(plan_run))
        .route("/run/start", post(start_run))
        .route("/run/cancel", post(cancel_run))
        .route("/run/step", post(answer_step))
        .route("/run/interrupt", post(interrupt_run))
}

pub fn verdict_of(reply: &StepReply) -> Option<StepVerdict> {
    let note = reply
        .note
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string);

    match reply.verdict.as_str() {
        "approve" => Some(StepVerdict { approve: true, note: None }),
        "improve" => Some(StepVerdict { approve: true, note }),
        "redo" => Some(StepVerdict { approve: false, note }),
        _ => None,
    }
}

async fn plan_run(
    State(state): State<AppState>,
    axum::Json(req): axum::Json<GuidedRun>,
) -> Response {
    if req.project.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "pick where the game lives before the crew starts on it".to_string(),
        )
            .into_response();
    }
    if req.prompt.trim().len() < MIN_BRIEF {
        return (
            StatusCode::BAD_REQUEST,
            "say a bit more about what you want built".to_string(),
        )
            .into_response();
    }

    let build = BuildRequest {
        prompt: req.prompt,
        project: Some(req.project),
        ask_above: req.ask_above,
        guided: true,
        step_confirm: req.step_confirm,
    };

    match state.dispatch(StudioCommand::Build(build)) {
        Ok(()) => (StatusCode::ACCEPTED, "planning".to_string()).into_response(),
        Err(e) => (StatusCode::SERVICE_UNAVAILABLE, e).into_response(),
    }
}

async fn start_run(
    State(state): State<AppState>,
    axum::Json(req): axum::Json<StartPlan>,
) -> Response {
    let verdict = PlanVerdict::Start { steps: req.steps };
    if state.resolve_plan(&req.plan_id, verdict) {
        (StatusCode::ACCEPTED, "starting".to_string()).into_response()
    } else {
        (
            StatusCode::CONFLICT,
            "no plan is waiting to start; it may already be running".to_string(),
        )
            .into_response()
    }
}

async fn cancel_run(
    State(state): State<AppState>,
    axum::Json(req): axum::Json<CancelPlan>,
) -> Response {
    if state.resolve_plan(&req.plan_id, PlanVerdict::Cancel) {
        (StatusCode::ACCEPTED, "dropped".to_string()).into_response()
    } else {
        (
            StatusCode::CONFLICT,
            "no plan is waiting; nothing to drop".to_string(),
        )
            .into_response()
    }
}

async fn answer_step(
    State(state): State<AppState>,
    axum::Json(req): axum::Json<StepReply>,
) -> Response {
    let Some(verdict) = verdict_of(&req) else {
        return (
            StatusCode::BAD_REQUEST,
            format!("{} is not one of approve, improve or redo", req.verdict),
        )
            .into_response();
    };

    if state.resolve_step(&req.approval_id, verdict) {
        (StatusCode::ACCEPTED, "recorded".to_string()).into_response()
    } else {
        (
            StatusCode::CONFLICT,
            "nothing is waiting on that step; it may have already been answered".to_string(),
        )
            .into_response()
    }
}

async fn interrupt_run(
    State(state): State<AppState>,
    axum::Json(req): axum::Json<InterruptRequest>,
) -> Response {
    let note = req
        .note
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string);

    if !req.stop && note.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            "an interrupt either stops the run or carries a note for it".to_string(),
        )
            .into_response();
    }

    state.interrupt(Interrupt { stop: req.stop, note });

    let answer = if req.stop {
        "the run stops after the step it is on"
    } else {
        "your note goes into the next step"
    };
    (StatusCode::ACCEPTED, answer.to_string()).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use studio_store::Store;

    static NEXT_RUNPLAN_DIR: std::sync::atomic::AtomicUsize =
        std::sync::atomic::AtomicUsize::new(0);

    fn state() -> AppState {
        let nth = NEXT_RUNPLAN_DIR.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir()
            .join(format!("studio-runplan-{}-{nth}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        AppState::new(Arc::new(Store::open(dir.join("s.db")).unwrap()))
    }

    fn reply(verdict: &str, note: Option<&str>) -> StepReply {
        StepReply {
            approval_id: "step_1".into(),
            verdict: verdict.into(),
            note: note.map(str::to_string),
        }
    }

    #[test]
    fn a_plan_waiting_to_start_receives_the_steps_the_human_edited() {
        let s = state();
        let rx = s.await_plan("plan_1");
        let steps = vec![StepEdit {
            id: "t1".into(),
            role: "artist".into(),
            say: "Draw the bird as a paper plane".into(),
        }];
        assert!(s.resolve_plan("plan_1", PlanVerdict::Start { steps: steps.clone() }));
        assert_eq!(rx.recv().unwrap(), PlanVerdict::Start { steps });
    }

    #[test]
    fn dropping_a_plan_reaches_the_run_that_never_started() {
        let s = state();
        let rx = s.await_plan("plan_2");
        assert!(s.resolve_plan("plan_2", PlanVerdict::Cancel));
        assert_eq!(rx.recv().unwrap(), PlanVerdict::Cancel);
    }

    #[test]
    fn answering_a_plan_nobody_is_waiting_on_is_reported_not_silently_dropped() {
        let s = state();
        assert!(!s.resolve_plan("never_proposed", PlanVerdict::Cancel));
    }

    #[test]
    fn approving_a_step_carries_no_note_even_when_one_was_typed_and_discarded() {
        let v = verdict_of(&reply("approve", Some("ignore me"))).unwrap();
        assert_eq!(v, StepVerdict { approve: true, note: None });
    }

    #[test]
    fn approving_with_notes_lets_the_run_go_on_and_carries_them_forward() {
        let v = verdict_of(&reply("improve", Some("the pipes should be green"))).unwrap();
        assert!(v.approve);
        assert_eq!(v.note.as_deref(), Some("the pipes should be green"));
    }

    #[test]
    fn sending_a_step_back_keeps_the_notes_that_say_why() {
        let v = verdict_of(&reply("redo", Some("the bird never falls"))).unwrap();
        assert!(!v.approve);
        assert_eq!(v.note.as_deref(), Some("the bird never falls"));
    }

    #[test]
    fn a_blank_note_is_dropped_rather_than_briefed_as_whitespace() {
        assert!(verdict_of(&reply("improve", Some("   "))).unwrap().note.is_none());
    }

    #[test]
    fn an_unknown_verdict_is_refused_rather_than_guessed_at() {
        assert!(verdict_of(&reply("maybe", None)).is_none());
    }

    #[test]
    fn a_step_waiting_for_an_answer_gets_the_one_the_floor_sends() {
        let s = state();
        let rx = s.await_step("step_9");
        assert!(s.resolve_step("step_9", StepVerdict { approve: false, note: Some("again".into()) }));
        let got = rx.recv().unwrap();
        assert!(!got.approve);
        assert_eq!(got.note.as_deref(), Some("again"));
    }

    #[test]
    fn an_interrupt_waits_in_the_side_channel_until_the_run_looks() {
        let s = state();
        s.interrupt(Interrupt { stop: false, note: Some("make it night".into()) });
        s.interrupt(Interrupt { stop: true, note: None });

        let taken = s.take_interrupts();
        assert_eq!(taken.len(), 2, "an interrupt queued behind a busy run must not be lost");
        assert_eq!(taken[0].note.as_deref(), Some("make it night"));
        assert!(taken[1].stop);
        assert!(s.take_interrupts().is_empty(), "a run must not act on the same stop twice");
    }
}
