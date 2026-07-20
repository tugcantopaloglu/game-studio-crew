use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use std::sync::Arc;
use studio_events::{plan_resume, Coalescer, Envelope, ResumePlan};
use studio_store::Store;
use tokio::sync::broadcast;

pub const FLOOR_HTML: &str = include_str!("../web/floor.html");
pub const VOXEL_JS: &str = include_str!("../web/voxel.js");
pub const SCENE_JS: &str = include_str!("../web/scene.js");
pub const THREE_JS: &str = include_str!("../web/vendor/three.module.js");

#[derive(Debug, Clone, Deserialize)]
pub struct TaskRequest {
    pub role: String,
    pub brief: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MeetingRequest {
    pub kind: String,
    pub participants: Vec<String>,
    pub topic: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowRequest {
    pub workflow: String,
    pub brief: String,
}

#[derive(Debug, Clone)]
pub enum StudioCommand {
    Task(TaskRequest),
    Meeting(MeetingRequest),
    Workflow(WorkflowRequest),
}

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>,
    pub live: broadcast::Sender<Envelope>,
    pub commands: Option<std::sync::mpsc::Sender<StudioCommand>>,
}

impl AppState {
    pub fn new(store: Arc<Store>) -> Self {
        let (live, _) = broadcast::channel(1024);
        Self { store, live, commands: None }
    }

    pub fn with_commands(mut self, tx: std::sync::mpsc::Sender<StudioCommand>) -> Self {
        self.commands = Some(tx);
        self
    }

    pub fn dispatch(&self, cmd: StudioCommand) -> Result<(), String> {
        match &self.commands {
            None => Err("this server is read only; start it with studiod studio".to_string()),
            Some(tx) => tx.send(cmd).map_err(|_| "the studio runner is gone".to_string()),
        }
    }

    pub fn publish(&self, event: Envelope) {
        let _ = self.live.send(event);
    }
}

pub fn compact_for_snapshot(events: Vec<Envelope>) -> Vec<Envelope> {
    let mut c = Coalescer::new();
    for e in events {
        c.push(e);
    }
    c.flush()
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/voxel.js", get(voxel_js))
        .route("/scene.js", get(scene_js))
        .route("/vendor/three.module.js", get(three_js))
        .route("/floor", get(floor))
        .route("/runs/:run/snapshot", get(snapshot))
        .route("/runs/:run/events", get(events))
        .route("/ws", get(ws_upgrade))
        .route("/task", post(submit_task))
        .route("/meeting", post(convene_meeting))
        .route("/roles", get(roles))
        .route("/workflows", get(workflows))
        .route("/workflow", post(start_workflow))
        .with_state(state)
}

async fn index() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], FLOOR_HTML)
}

async fn voxel_js() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8")], VOXEL_JS)
}

async fn scene_js() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8")], SCENE_JS)
}

async fn three_js() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        THREE_JS,
    )
}

async fn roles() -> impl IntoResponse {
    let rows: Vec<serde_json::Value> = studio_agents::REGISTRY
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id,
                "title": r.title,
                "tier": r.tier,
                "department": r.department.id(),
                "escalates_to": r.escalates_to,
            })
        })
        .collect();
    axum::Json(rows)
}

async fn submit_task(
    State(state): State<AppState>,
    axum::Json(req): axum::Json<TaskRequest>,
) -> Response {
    if studio_agents::role(&req.role).is_none() {
        return (StatusCode::BAD_REQUEST, format!("unknown role {}", req.role)).into_response();
    }
    if req.brief.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "a task needs a brief".to_string()).into_response();
    }
    match state.dispatch(StudioCommand::Task(req)) {
        Ok(()) => (StatusCode::ACCEPTED, "queued".to_string()).into_response(),
        Err(e) => (StatusCode::SERVICE_UNAVAILABLE, e).into_response(),
    }
}

async fn convene_meeting(
    State(state): State<AppState>,
    axum::Json(req): axum::Json<MeetingRequest>,
) -> Response {
    if req.participants.len() < 2 {
        return (
            StatusCode::BAD_REQUEST,
            "a meeting needs at least two participants".to_string(),
        )
            .into_response();
    }
    for p in &req.participants {
        if studio_agents::role(p).is_none() {
            return (StatusCode::BAD_REQUEST, format!("unknown role {p}")).into_response();
        }
    }
    match state.dispatch(StudioCommand::Meeting(req)) {
        Ok(()) => (StatusCode::ACCEPTED, "queued".to_string()).into_response(),
        Err(e) => (StatusCode::SERVICE_UNAVAILABLE, e).into_response(),
    }
}

async fn workflows() -> impl IntoResponse {
    let rows: Vec<serde_json::Value> = studio_workflow::Workflow::builtin()
        .iter()
        .map(|w| {
            serde_json::json!({
                "id": w.id,
                "title": w.title,
                "nodes": w.nodes.iter().map(|n| &n.id).collect::<Vec<_>>(),
                "gates": w.gates.len(),
                "budget_tokens": w.total_budget(),
            })
        })
        .collect();
    axum::Json(rows)
}

async fn start_workflow(
    State(state): State<AppState>,
    axum::Json(req): axum::Json<WorkflowRequest>,
) -> Response {
    let known = studio_workflow::Workflow::builtin()
        .iter()
        .any(|w| w.id == req.workflow);
    if !known {
        return (
            StatusCode::BAD_REQUEST,
            format!("unknown workflow {}", req.workflow),
        )
            .into_response();
    }
    if req.brief.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "a workflow needs a brief".to_string()).into_response();
    }
    match state.dispatch(StudioCommand::Workflow(req)) {
        Ok(()) => (StatusCode::ACCEPTED, "queued".to_string()).into_response(),
        Err(e) => (StatusCode::SERVICE_UNAVAILABLE, e).into_response(),
    }
}

async fn floor() -> impl IntoResponse {
    axum::Json(studio_agents::layout::studio_floor())
}

#[derive(Debug, Deserialize)]
pub struct SinceQuery {
    #[serde(default)]
    pub since_seq: u64,
    pub run: Option<String>,
}

async fn snapshot(
    State(state): State<AppState>,
    Path(run): Path<String>,
) -> Result<Response, StatusCode> {
    let all = state
        .store
        .events_since(&run, 0)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let head = all.last().map(|e| e.seq).unwrap_or(0);
    let compacted = compact_for_snapshot(all);
    Ok(axum::Json(serde_json::json!({
        "run": run,
        "head": head,
        "events": compacted,
    }))
    .into_response())
}

async fn events(
    State(state): State<AppState>,
    Path(run): Path<String>,
    Query(q): Query<SinceQuery>,
) -> Result<Response, StatusCode> {
    let all = state
        .store
        .events_since(&run, 0)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let head = all.last().map(|e| e.seq).unwrap_or(0);

    let body = match plan_resume(q.since_seq, head) {
        ResumePlan::UpToDate => serde_json::json!({
            "run": run, "head": head, "mode": "up_to_date", "events": Vec::<Envelope>::new()
        }),
        ResumePlan::Snapshot { head } => serde_json::json!({
            "run": run, "head": head, "mode": "snapshot",
            "events": compact_for_snapshot(all)
        }),
        ResumePlan::Replay { from_seq, .. } => {
            let tail: Vec<Envelope> = all.into_iter().filter(|e| e.seq >= from_seq).collect();
            serde_json::json!({
                "run": run, "head": head, "mode": "replay", "events": tail
            })
        }
    };

    Ok(axum::Json(body).into_response())
}

async fn ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(q): Query<SinceQuery>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| ws_run(socket, state, q))
}

async fn ws_run(mut socket: WebSocket, state: AppState, q: SinceQuery) {
    let mut rx = state.live.subscribe();

    if let Some(run) = &q.run {
        if let Ok(all) = state.store.events_since(run, 0) {
            let head = all.last().map(|e| e.seq).unwrap_or(0);
            let backlog = match plan_resume(q.since_seq, head) {
                ResumePlan::UpToDate => Vec::new(),
                ResumePlan::Snapshot { .. } => compact_for_snapshot(all),
                ResumePlan::Replay { from_seq, .. } => {
                    all.into_iter().filter(|e| e.seq >= from_seq).collect()
                }
            };
            for e in backlog {
                if send_event(&mut socket, &e).await.is_err() {
                    return;
                }
            }
        }
    }

    loop {
        tokio::select! {
            incoming = socket.recv() => match incoming {
                None | Some(Err(_)) => return,
                Some(Ok(Message::Close(_))) => return,
                Some(Ok(_)) => {}
            },
            broadcast = rx.recv() => match broadcast {
                Err(broadcast::error::RecvError::Closed) => return,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Ok(e) => {
                    if q.run.as_deref().is_some_and(|r| r != e.run) {
                        continue;
                    }
                    if send_event(&mut socket, &e).await.is_err() {
                        return;
                    }
                }
            },
        }
    }
}

async fn send_event(socket: &mut WebSocket, e: &Envelope) -> Result<(), ()> {
    let text = serde_json::to_string(e).map_err(|_| ())?;
    socket.send(Message::Text(text)).await.map_err(|_| ())
}

pub async fn serve(state: AppState, port: u16) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    axum::serve(listener, router(state)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_events::{EventType, Scene};

    fn ev(seq: u64, actor: &str, ty: EventType) -> Envelope {
        Envelope::new(seq, "t", "run_1", actor, Scene::daemon(), ty, serde_json::json!({}))
    }

    #[test]
    fn a_snapshot_collapses_a_noisy_log_but_keeps_every_terminal_event() {
        let mut log = Vec::new();
        for seq in 1..=100 {
            log.push(ev(seq, "gameplay_engineer#1", EventType::TokenUsage));
        }
        log.push(ev(101, "gameplay_engineer#1", EventType::VerifyResult));
        log.push(ev(102, "gameplay_engineer#1", EventType::DecisionRecorded));

        let out = compact_for_snapshot(log);
        assert_eq!(out.len(), 3, "100 token updates collapse to one");
        assert!(out.iter().any(|e| e.event_type == EventType::VerifyResult));
        assert!(out.iter().any(|e| e.event_type == EventType::DecisionRecorded));
        assert_eq!(
            out.iter().find(|e| e.event_type == EventType::TokenUsage).unwrap().seq,
            100
        );
    }

    #[test]
    fn a_snapshot_of_an_empty_run_is_empty_rather_than_an_error() {
        assert!(compact_for_snapshot(Vec::new()).is_empty());
    }

    #[test]
    fn the_snapshot_stays_in_sequence_order_so_the_client_can_reduce_it() {
        let log = vec![
            ev(5, "b", EventType::TokenUsage),
            ev(1, "a", EventType::WorkerSpawned),
            ev(9, "c", EventType::VerifyResult),
        ];
        let out = compact_for_snapshot(log);
        let seqs: Vec<u64> = out.iter().map(|e| e.seq).collect();
        let mut sorted = seqs.clone();
        sorted.sort_unstable();
        assert_eq!(seqs, sorted);
    }

    #[test]
    fn the_served_floor_matches_the_registry() {
        let floor = studio_agents::layout::studio_floor();
        assert_eq!(floor.desks.len(), studio_agents::REGISTRY.len());
    }
}
