use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use studio_events::{plan_resume, Coalescer, Envelope, ResumePlan};
use studio_store::Store;
use tokio::sync::broadcast;

pub mod assets;
pub mod fsapi;
pub mod games;
pub mod gitapi;
pub mod health;
pub mod resume;
pub mod runplan;
pub mod settings;
pub mod web;

pub const FLOOR_HTML: &str = include_str!("../web/floor.html");
pub const VOXEL_JS: &str = include_str!("../web/voxel.js");
pub const SCENE_JS: &str = include_str!("../web/scene.js");
pub const THREE_JS: &str = include_str!("../web/vendor/three.module.js");

#[derive(Debug, Clone, Deserialize)]
pub struct TaskRequest {
    pub role: String,
    pub brief: String,
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MeetingRequest {
    pub kind: String,
    pub participants: Vec<String>,
    pub topic: String,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub ask_above: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowRequest {
    pub workflow: String,
    pub brief: String,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub ask_above: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BuildRequest {
    pub prompt: String,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub ask_above: Option<u64>,
    #[serde(default)]
    pub guided: bool,
    #[serde(default)]
    pub step_confirm: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlayRequest {
    pub project: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResumeRequest {
    pub project: String,
    #[serde(default)]
    pub ask_above: Option<u64>,
    #[serde(default)]
    pub step_confirm: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RevertRequest {
    pub project: String,
    pub sha: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SummarizeRequest {
    pub project: String,
}

#[derive(Debug, Clone)]
pub enum StudioCommand {
    Task(TaskRequest),
    Meeting(MeetingRequest),
    Workflow(WorkflowRequest),
    Build(BuildRequest),
    Summarize(SummarizeRequest),
    Resume(ResumeRequest),
}

pub type Approvals = Arc<std::sync::Mutex<HashMap<String, std::sync::mpsc::Sender<bool>>>>;

#[derive(Debug, Clone, PartialEq)]
pub enum PlanVerdict {
    Start { steps: Vec<studio_workflow::StepEdit> },
    Cancel,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StepVerdict {
    pub approve: bool,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Interrupt {
    pub stop: bool,
    pub note: Option<String>,
}

pub type PlanGates = Arc<std::sync::Mutex<HashMap<String, std::sync::mpsc::Sender<PlanVerdict>>>>;
pub type StepGates = Arc<std::sync::Mutex<HashMap<String, std::sync::mpsc::Sender<StepVerdict>>>>;
pub type Interrupts = Arc<std::sync::Mutex<Vec<Interrupt>>>;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>,
    pub live: broadcast::Sender<Envelope>,
    pub commands: Option<std::sync::mpsc::Sender<StudioCommand>>,
    pub approvals: Approvals,
    pub plans: PlanGates,
    pub steps: StepGates,
    pub interrupts: Interrupts,
    pub stopping: Arc<std::sync::atomic::AtomicBool>,
    pub studio_dir: Arc<std::path::PathBuf>,
}

pub fn default_studio_dir() -> std::path::PathBuf {
    match std::env::current_dir() {
        Ok(cwd) => cwd.join(".studio"),
        Err(_) => std::path::PathBuf::from(".studio"),
    }
}

impl AppState {
    pub fn new(store: Arc<Store>) -> Self {
        let (live, _) = broadcast::channel(1024);
        Self {
            store,
            live,
            commands: None,
            approvals: Arc::new(std::sync::Mutex::new(HashMap::new())),
            plans: Arc::new(std::sync::Mutex::new(HashMap::new())),
            steps: Arc::new(std::sync::Mutex::new(HashMap::new())),
            interrupts: Arc::new(std::sync::Mutex::new(Vec::new())),
            stopping: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            studio_dir: Arc::new(default_studio_dir()),
        }
    }

    pub fn with_studio_dir(mut self, dir: std::path::PathBuf) -> Self {
        self.studio_dir = Arc::new(dir);
        self
    }

    pub fn await_approval(&self, id: &str) -> std::sync::mpsc::Receiver<bool> {
        let (tx, rx) = std::sync::mpsc::channel();
        if let Ok(mut pending) = self.approvals.lock() {
            pending.insert(id.to_string(), tx);
        }
        rx
    }

    pub fn resolve_approval(&self, id: &str, approve: bool) -> bool {
        let sender = self.approvals.lock().ok().and_then(|mut p| p.remove(id));
        match sender {
            Some(tx) => tx.send(approve).is_ok(),
            None => false,
        }
    }

    pub fn await_plan(&self, plan_id: &str) -> std::sync::mpsc::Receiver<PlanVerdict> {
        let (tx, rx) = std::sync::mpsc::channel();
        if let Ok(mut waiting) = self.plans.lock() {
            waiting.insert(plan_id.to_string(), tx);
        }
        rx
    }

    pub fn resolve_plan(&self, plan_id: &str, verdict: PlanVerdict) -> bool {
        let sender = self.plans.lock().ok().and_then(|mut p| p.remove(plan_id));
        match sender {
            Some(tx) => tx.send(verdict).is_ok(),
            None => false,
        }
    }

    pub fn await_step(&self, approval_id: &str) -> std::sync::mpsc::Receiver<StepVerdict> {
        let (tx, rx) = std::sync::mpsc::channel();
        if let Ok(mut waiting) = self.steps.lock() {
            waiting.insert(approval_id.to_string(), tx);
        }
        rx
    }

    pub fn resolve_step(&self, approval_id: &str, verdict: StepVerdict) -> bool {
        let sender = self.steps.lock().ok().and_then(|mut p| p.remove(approval_id));
        match sender {
            Some(tx) => tx.send(verdict).is_ok(),
            None => false,
        }
    }

    pub fn interrupt(&self, interrupt: Interrupt) {
        if interrupt.stop {
            self.stopping.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        if let Ok(mut queued) = self.interrupts.lock() {
            queued.push(interrupt);
        }
    }

    pub fn stop_asked(&self) -> Arc<std::sync::atomic::AtomicBool> {
        self.stopping.clone()
    }

    pub fn nothing_is_being_stopped(&self) {
        self.stopping.store(false, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn take_interrupts(&self) -> Vec<Interrupt> {
        match self.interrupts.lock() {
            Ok(mut queued) => std::mem::take(&mut *queued),
            Err(_) => Vec::new(),
        }
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

fn origin_is_local(origin: &str) -> bool {
    let rest = match origin.split_once("://") {
        Some((scheme, rest)) if scheme == "http" || scheme == "https" => rest,
        _ => return false,
    };
    let host = rest.split('/').next().unwrap_or("");
    let host = match host.rsplit_once(':') {
        Some((h, port)) if port.chars().all(|c| c.is_ascii_digit()) => h,
        _ => host,
    };
    matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1")
}

async fn guard_origin(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<Response, StatusCode> {
    if let Some(origin) = req.headers().get(header::ORIGIN) {
        let ok = origin.to_str().map(origin_is_local).unwrap_or(false);
        if !ok {
            return Err(StatusCode::FORBIDDEN);
        }
    }
    Ok(next.run(req).await)
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
        .route("/projects", get(projects).post(create_project))
        .route("/approve", post(approve))
        .route("/workflows", get(workflows))
        .route("/workflow", post(start_workflow))
        .route("/build", post(start_build))
        .route("/resume", post(start_resume))
        .route("/resumable", get(resumable))
        .route("/play", post(play))
        .route("/qa", get(qa_report))
        .route("/shot", get(latest_shot))
        .route("/revert", post(revert))
        .merge(web::routes())
        .merge(assets::routes())
        .merge(fsapi::routes())
        .merge(settings::routes())
        .merge(games::routes())
        .merge(gitapi::routes())
        .merge(runplan::routes())
        .merge(health::routes())
        .layer(axum::middleware::from_fn(guard_origin))
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
    if req.topic.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "a meeting needs a topic; it becomes the title of the decision".to_string(),
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

async fn start_build(
    State(state): State<AppState>,
    axum::Json(req): axum::Json<BuildRequest>,
) -> Response {
    if req.prompt.trim().len() < 8 {
        return (
            StatusCode::BAD_REQUEST,
            "say a bit more about what you want built".to_string(),
        )
            .into_response();
    }
    match state.dispatch(StudioCommand::Build(req)) {
        Ok(()) => (StatusCode::ACCEPTED, "planning".to_string()).into_response(),
        Err(e) => (StatusCode::SERVICE_UNAVAILABLE, e).into_response(),
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct WhichProject {
    pub project: String,
}

async fn resumable(
    State(state): State<AppState>,
    Query(q): Query<WhichProject>,
) -> Response {
    let held = resume::read(&state.studio_dir, &q.project);
    axum::Json(match held {
        None => serde_json::json!({"resumable": false}),
        Some(held) => serde_json::json!({
            "resumable": true,
            "title": held.title,
            "done": held.done.len(),
            "left": held.left(),
            "steps": held.plan.tasks.len(),
            "left_at": held.left_at,
            "why": held.why,
            "say": held
                .left()
                .iter()
                .filter_map(|id| held.plan.tasks.iter().find(|t| &t.id == id))
                .map(|t| serde_json::json!({"id": t.id, "role": t.role, "say": t.say()}))
                .collect::<Vec<_>>(),
        }),
    })
    .into_response()
}

async fn start_resume(
    State(state): State<AppState>,
    axum::Json(req): axum::Json<ResumeRequest>,
) -> Response {
    if resume::read(&state.studio_dir, &req.project).is_none() {
        return (
            StatusCode::CONFLICT,
            "there is no stopped run to pick up for this project".to_string(),
        )
            .into_response();
    }
    match state.dispatch(StudioCommand::Resume(req)) {
        Ok(()) => (StatusCode::ACCEPTED, "resuming".to_string()).into_response(),
        Err(e) => (StatusCode::SERVICE_UNAVAILABLE, e).into_response(),
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewProject {
    pub name: String,
    pub root: String,
    #[serde(default)]
    pub engine: Option<String>,
    #[serde(default = "yes")]
    pub git: bool,
}

fn yes() -> bool {
    true
}

#[cfg(test)]
mod project_path_tests {
    use super::strip_verbatim;
    use std::path::PathBuf;

    #[test]
    fn a_verbatim_drive_path_loses_the_prefix() {
        assert_eq!(
            strip_verbatim(PathBuf::from(r"\\?\C:\games\neon")),
            PathBuf::from(r"C:\games\neon")
        );
    }

    #[test]
    fn a_verbatim_unc_share_is_left_alone() {
        let unc = PathBuf::from(r"\\?\UNC\server\share");
        assert_eq!(strip_verbatim(unc.clone()), unc);
    }

    #[test]
    fn a_plain_path_is_untouched() {
        let plain = PathBuf::from("/home/topal/games");
        assert_eq!(strip_verbatim(plain.clone()), plain);
    }
}

fn strip_verbatim(p: std::path::PathBuf) -> std::path::PathBuf {
    let text = p.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.len() > 2 && rest.as_bytes()[1] == b':' => {
            std::path::PathBuf::from(rest)
        }
        _ => p,
    }
}

async fn projects(State(state): State<AppState>) -> Result<Response, StatusCode> {
    let rows = state.store.projects().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let json: Vec<_> = rows
        .into_iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id, "name": p.name, "root": p.root,
                "engine": p.engine, "git": p.git,
            })
        })
        .collect();
    Ok(axum::Json(json).into_response())
}

pub(crate) fn project_root(state: &AppState, id: &str) -> Option<std::path::PathBuf> {
    state
        .store
        .projects()
        .ok()?
        .into_iter()
        .find(|p| p.id == id)
        .map(|p| std::path::PathBuf::from(p.root))
}

async fn qa_report(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let Some(root) = q.get("project").and_then(|id| project_root(&state, id)) else {
        return (StatusCode::NOT_FOUND, "no such project".to_string()).into_response();
    };
    let path = root.join("qa").join("report.md");
    match std::fs::read_to_string(&path) {
        Ok(body) => {
            let head: String = body.lines().take(30).collect::<Vec<_>>().join("\n");
            let defects = body.matches("QA-").count();
            axum::Json(serde_json::json!({
                "exists": true,
                "defects": defects,
                "head": head.chars().take(1200).collect::<String>(),
            }))
            .into_response()
        }
        Err(_) => axum::Json(serde_json::json!({"exists": false, "defects": 0, "head": ""}))
            .into_response(),
    }
}

async fn latest_shot(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let Some(root) = q.get("project").and_then(|id| project_root(&state, id)) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let shot = root.join(".studio-out").join("shots").join("latest.png");
    match std::fs::read(&shot) {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, "image/png"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn revert(
    State(state): State<AppState>,
    axum::Json(req): axum::Json<RevertRequest>,
) -> Response {
    let Some(root) = project_root(&state, &req.project) else {
        return (StatusCode::NOT_FOUND, "no such project".to_string()).into_response();
    };
    if !studio_core::git::is_repo(&root) {
        return (
            StatusCode::CONFLICT,
            "this project is not a git repository; nothing to revert to".to_string(),
        )
            .into_response();
    }
    match studio_core::git::reset_hard(&root, &req.sha) {
        Ok(()) => (StatusCode::OK, format!("reverted to {}", req.sha)).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

fn windowed_binary(bin: std::path::PathBuf) -> std::path::PathBuf {
    let name = bin.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let stripped = name.replace("_console", "");
    if stripped != name {
        let sibling = bin.with_file_name(stripped);
        if sibling.exists() {
            return sibling;
        }
    }
    bin
}

pub const WEB_PLAY_URL: &str = "http://127.0.0.1:8765/";

struct Playing {
    group: Option<studio_core::ProcessGroup>,
    child: std::process::Child,
}

static PLAYING: std::sync::Mutex<std::collections::BTreeMap<String, Playing>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

pub fn stop_playing(project: &str) -> bool {
    let taken = PLAYING.lock().ok().and_then(|mut held| held.remove(project));
    let Some(mut was) = taken else {
        return false;
    };
    match was.group.as_mut() {
        Some(group) => {
            let _ = group.kill_tree();
        }
        None => {
            let _ = was.child.kill();
        }
    }
    let _ = was.child.wait();
    true
}

pub fn stop_playing_everything() {
    let ids: Vec<String> = PLAYING
        .lock()
        .map(|held| held.keys().cloned().collect())
        .unwrap_or_default();
    for id in ids {
        stop_playing(&id);
    }
}

fn hold(project: &str, group: Option<studio_core::ProcessGroup>, child: std::process::Child) {
    if let Ok(mut held) = PLAYING.lock() {
        held.insert(project.to_string(), Playing { group, child });
    }
}

fn start_supervised(
    mut cmd: std::process::Command,
) -> std::io::Result<(studio_core::ProcessGroup, std::process::Child)> {
    let mut group = studio_core::ProcessGroup::new()?;
    group.prepare(&mut cmd);
    let child = cmd.spawn()?;
    group.adopt(&child)?;
    Ok((group, child))
}

fn open_in_browser(url: &str, cwd: &str) -> std::io::Result<()> {
    let (program, args): (&str, Vec<&str>) = if cfg!(windows) {
        ("cmd", vec!["/C", "start", ""])
    } else if cfg!(target_os = "macos") {
        ("open", Vec::new())
    } else {
        ("xdg-open", Vec::new())
    };
    studio_core::command(program)
        .args(args)
        .arg(url)
        .current_dir(cwd)
        .spawn()
        .map(|_| ())
}

fn start_playing(p: &studio_store::ProjectRow, bin: &std::path::Path) -> std::io::Result<()> {
    stop_playing(&p.id);

    match p.engine.as_str() {
        "web" => {
            let mut cmd = std::process::Command::new(bin);
            cmd.arg("tools/serve.mjs").current_dir(&p.root);
            let (group, child) = start_supervised(cmd)?;
            hold(&p.id, Some(group), child);
            std::thread::sleep(std::time::Duration::from_millis(400));
            open_in_browser(WEB_PLAY_URL, &p.root)
        }
        "python" => {
            let mut cmd = studio_core::command("cmd");
            cmd.args(["/C", "start", ""])
                .arg(bin)
                .arg("main.py")
                .current_dir(&p.root);
            let child = cmd.spawn()?;
            hold(&p.id, None, child);
            Ok(())
        }
        _ => {
            let mut cmd = studio_core::command(bin);
            cmd.arg("--path").arg(&p.root).current_dir(&p.root);
            let child = cmd.spawn()?;
            hold(&p.id, None, child);
            Ok(())
        }
    }
}

async fn play(
    State(state): State<AppState>,
    axum::Json(req): axum::Json<PlayRequest>,
) -> Response {
    let rows = match state.store.projects() {
        Ok(rows) => rows,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let Some(p) = rows.into_iter().find(|p| p.id == req.project) else {
        return (StatusCode::NOT_FOUND, "no such project".to_string()).into_response();
    };
    let profiles = studio_engine::EngineProfile::builtin();
    let Some(profile) = profiles.iter().find(|e| e.id == p.engine) else {
        return (
            StatusCode::BAD_REQUEST,
            format!("no engine profile for {}", p.engine),
        )
            .into_response();
    };
    let bin = match studio_engine::resolve_binary(profile) {
        Ok(b) => windowed_binary(b),
        Err(e) => return (StatusCode::CONFLICT, e.to_string()).into_response(),
    };

    let name = p.name.clone();
    let started = tokio::task::spawn_blocking(move || start_playing(&p, &bin).map_err(|e| (e, bin)))
        .await;

    match started {
        Ok(Ok(())) => (StatusCode::OK, format!("{name} is starting")).into_response(),
        Ok(Err((e, bin))) => (
            StatusCode::BAD_GATEWAY,
            format!("could not start {}: {e}", bin.display()),
        )
            .into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "the launch thread died".to_string(),
        )
            .into_response(),
    }
}

async fn create_project(
    State(state): State<AppState>,
    axum::Json(req): axum::Json<NewProject>,
) -> Response {
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return (StatusCode::BAD_REQUEST, "a project needs a name".to_string()).into_response();
    }

    let raw = req.root.trim();
    if raw.is_empty() {
        return (StatusCode::BAD_REQUEST, "a project needs a path".to_string()).into_response();
    }
    let root = std::path::PathBuf::from(raw);
    if !root.is_absolute() {
        return (
            StatusCode::BAD_REQUEST,
            format!("give an absolute path; {raw} is relative"),
        )
            .into_response();
    }

    if let Err(e) = std::fs::create_dir_all(&root) {
        return (
            StatusCode::BAD_REQUEST,
            format!("could not create {}: {e}", root.display()),
        )
            .into_response();
    }

    let canonical = root.canonicalize().map(strip_verbatim).unwrap_or(root);
    let mut engine = req.engine.unwrap_or_else(|| "godot".into());

    if engine == "auto" {
        let profiles = studio_engine::EngineProfile::builtin();
        match studio_engine::detect(&canonical, &profiles).first() {
            Some(d) => engine = d.id.clone(),
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    format!(
                        "no engine detected at {}; pick one explicitly to scaffold a fresh project",
                        canonical.display()
                    ),
                )
                    .into_response()
            }
        }
    }

    if let Err(e) = studio_engine::scaffold(&engine, &canonical, &name) {
        return (
            StatusCode::BAD_REQUEST,
            format!("could not scaffold the {engine} project: {e}"),
        )
            .into_response();
    }

    let git_ready = if req.git {
        if !studio_core::git::available() {
            return (
                StatusCode::BAD_REQUEST,
                "git is not on PATH; install it or create the project without git".to_string(),
            )
                .into_response();
        }
        if let Err(e) = studio_core::git::init(&canonical) {
            return (StatusCode::BAD_REQUEST, format!("git init failed: {e}")).into_response();
        }
        true
    } else {
        false
    };

    let row = studio_store::ProjectRow {
        id: format!("proj_{}", name.to_lowercase().replace(|c: char| !c.is_alphanumeric(), "-")),
        name,
        root: canonical.to_string_lossy().into_owned(),
        engine,
        git: git_ready,
    };

    let ts = now_rfc3339();
    match state.store.insert_project(row.clone(), ts) {
        Ok(()) => (
            StatusCode::CREATED,
            axum::Json(serde_json::json!({
                "id": row.id, "name": row.name, "root": row.root,
                "engine": row.engine, "git": row.git,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::CONFLICT,
            format!("could not record the project (is the name or path already used?): {e}"),
        )
            .into_response(),
    }
}

pub(crate) fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApprovalReply {
    pub approval_id: String,
    pub approve: bool,
}

async fn approve(
    State(state): State<AppState>,
    axum::Json(req): axum::Json<ApprovalReply>,
) -> Response {
    if state.resolve_approval(&req.approval_id, req.approve) {
        (StatusCode::ACCEPTED, "recorded".to_string()).into_response()
    } else {
        (
            StatusCode::CONFLICT,
            "nothing is waiting on that approval; it may have already been answered".to_string(),
        )
            .into_response()
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
    let head = state
        .store
        .head_seq(&run)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let all = state
        .store
        .events_since(&run, 0)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
    let head = state
        .store
        .head_seq(&run)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let body = match plan_resume(q.since_seq, head) {
        ResumePlan::UpToDate => serde_json::json!({
            "run": run, "head": head, "mode": "up_to_date", "events": Vec::<Envelope>::new()
        }),
        ResumePlan::Snapshot { head } => {
            let all = state
                .store
                .events_since(&run, 0)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            serde_json::json!({
                "run": run, "head": head, "mode": "snapshot",
                "events": compact_for_snapshot(all)
            })
        }
        ResumePlan::Replay { from_seq, .. } => {
            let tail = state
                .store
                .events_since(&run, from_seq.saturating_sub(1))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
        if let Ok(head) = state.store.head_seq(run) {
            let backlog = match plan_resume(q.since_seq, head) {
                ResumePlan::UpToDate => Ok(Vec::new()),
                ResumePlan::Snapshot { .. } => {
                    state.store.events_since(run, 0).map(compact_for_snapshot)
                }
                ResumePlan::Replay { from_seq, .. } => {
                    state.store.events_since(run, from_seq.saturating_sub(1))
                }
            };
            for e in backlog.unwrap_or_default() {
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
                Err(broadcast::error::RecvError::Lagged(_)) => return,
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

    #[test]
    fn nothing_on_the_play_path_spawns_a_bare_command_and_flashes_a_console() {
        let source = include_str!("lib.rs");
        let play = source
            .split("pub const WEB_PLAY_URL")
            .nth(1)
            .and_then(|rest| rest.split("#[cfg(test)]").next())
            .expect("the play path is between the url constant and the tests");
        assert!(
            !play.contains("std::process::Command::new(\"cmd\")"),
            "a bare Command::new on Windows gives the child its own console; pressing play \
             would flash a black window every time. Use studio_core::command."
        );
        assert!(
            play.contains("studio_core::command"),
            "the play path must go through the launcher that sets CREATE_NO_WINDOW"
        );
    }

    #[test]
    fn a_server_the_studio_starts_for_a_game_is_supervised_so_it_cannot_outlive_the_daemon() {
        let mut cmd = std::process::Command::new(if cfg!(windows) { "cmd" } else { "sh" });
        cmd.args(if cfg!(windows) {
            ["/C", "ping -n 30 127.0.0.1 >NUL"]
        } else {
            ["-c", "sleep 30"]
        });
        let (mut group, mut child) = start_supervised(cmd).expect("a shell always spawns");
        assert!(
            child.try_wait().unwrap().is_none(),
            "the child must still be running for this to prove anything"
        );

        group.kill_tree().unwrap();
        let died = std::time::Instant::now();
        while child.try_wait().unwrap().is_none() {
            assert!(
                died.elapsed() < std::time::Duration::from_secs(5),
                "killing the group left the serve.mjs the studio started running forever"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[test]
    fn playing_a_project_stops_whatever_the_last_play_left_running() {
        let mut cmd = std::process::Command::new(if cfg!(windows) { "cmd" } else { "sh" });
        cmd.args(if cfg!(windows) {
            ["/C", "ping -n 30 127.0.0.1 >NUL"]
        } else {
            ["-c", "sleep 30"]
        });
        let (group, child) = start_supervised(cmd).unwrap();

        let project = "proj_test_replacing";
        hold(project, Some(group), child);
        assert!(
            stop_playing(project),
            "a second play must reclaim the port the first one is holding"
        );
        assert!(!stop_playing(project), "there is nothing left to stop twice");
        assert!(PLAYING.lock().unwrap().get(project).is_none());
    }

    #[test]
    fn the_launch_path_reclaims_the_port_before_it_binds_it_again() {
        let source = include_str!("lib.rs");
        let launch = source
            .split("fn start_playing(")
            .nth(1)
            .and_then(|rest| rest.split("\nasync fn play(").next())
            .expect("start_playing sits just above the route");
        assert!(
            launch.trim_start().starts_with("p: &studio_store::ProjectRow")
                && launch.contains("stop_playing(&p.id)"),
            "serve.mjs binds 8765; starting a second one without stopping the first leaves a \
             server nobody can reach and nobody kills"
        );
    }

    fn a_run_that_stopped(dir: &std::path::Path) -> resume::Unfinished {
        use studio_workflow::{Plan, PlanTask};
        let task = |id: &str, say: &str| PlanTask {
            id: id.into(),
            role: "artist".into(),
            brief: format!("brief for {id}"),
            depends_on: Vec::new(),
            say: say.into(),
        };
        let held = resume::Unfinished {
            project: "proj_flappy".into(),
            title: "3D Flappy Bird".into(),
            brief: "build a 3d flappy bird".into(),
            plan: Plan {
                title: "3D Flappy Bird".into(),
                tasks: vec![
                    task("t1", "Pin down how the bird flies"),
                    task("t2", "Draw the bird and the pipes"),
                    task("t3", "Make the flap thump"),
                ],
            },
            done: vec!["t1".into()],
            left_at: "2026-07-26T19:40:00Z".into(),
            why: "the account is out of allowance until 7:40pm".into(),
        };
        resume::write(dir, &held).unwrap();
        held
    }

    async fn get(state: AppState, uri: &str) -> (StatusCode, serde_json::Value) {
        use tower::ServiceExt;
        let req = axum::http::Request::builder()
            .uri(uri)
            .body(axum::body::Body::empty())
            .unwrap();
        let res = router(state).oneshot(req).await.unwrap();
        let status = res.status();
        let raw = axum::body::to_bytes(res.into_body(), 1_000_000).await.unwrap();
        (status, serde_json::from_slice(&raw).unwrap_or(serde_json::Value::Null))
    }

    fn state_for_resume(slug: &str) -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("studio-resume-route-{slug}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = Arc::new(Store::open(dir.join("s.db")).unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        std::mem::forget(rx);
        (
            AppState::new(store).with_commands(tx).with_studio_dir(dir.clone()),
            dir,
        )
    }

    #[tokio::test]
    async fn a_project_with_nothing_stopped_offers_no_resume() {
        let (state, _dir) = state_for_resume("nothing");
        let (status, body) = get(state, "/resumable?project=proj_flappy").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["resumable"], false);
    }

    #[tokio::test]
    async fn a_stopped_run_offers_exactly_the_steps_that_never_ran() {
        let (state, dir) = state_for_resume("offers");
        a_run_that_stopped(&dir);

        let (status, body) = get(state, "/resumable?project=proj_flappy").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["resumable"], true);
        assert_eq!(body["done"], 1);
        assert_eq!(body["steps"], 3);
        assert_eq!(body["left"], serde_json::json!(["t2", "t3"]));
        assert!(
            body["why"].as_str().unwrap().contains("7:40pm"),
            "the floor has to say why it stopped, or picking it up is a guess"
        );
        assert_eq!(
            body["say"][0]["say"], "Draw the bird and the pipes",
            "the button names the work in the player's terms, not t2"
        );
    }

    #[tokio::test]
    async fn resuming_a_project_that_finished_is_refused_rather_than_queued() {
        use tower::ServiceExt;
        let (state, _dir) = state_for_resume("refused");
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/resume")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(r#"{"project":"proj_flappy"}"#))
            .unwrap();
        let res = router(state).oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn a_stopped_run_can_be_picked_up() {
        use tower::ServiceExt;
        let (state, dir) = state_for_resume("accepted");
        a_run_that_stopped(&dir);

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/resume")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(r#"{"project":"proj_flappy"}"#))
            .unwrap();
        let res = router(state).oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::ACCEPTED);
    }

    fn ev(seq: u64, actor: &str, ty: EventType) -> Envelope {
        Envelope::new(seq, "t", "run_1", actor, Scene::daemon(), ty, serde_json::json!({}))
    }

    async fn post_with_origin(origin: Option<&str>) -> StatusCode {
        use tower::ServiceExt;

        let slug: String = origin
            .unwrap_or("none")
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let dir = std::env::temp_dir().join(format!("studio-origin-{slug}"));
        let _ = std::fs::create_dir_all(&dir);
        let store = Arc::new(Store::open(dir.join("s.db")).unwrap());
        let (tx, _rx) = std::sync::mpsc::channel();
        let app = router(AppState::new(store).with_commands(tx));

        let mut req = axum::http::Request::builder()
            .method("POST")
            .uri("/task")
            .header("content-type", "application/json");
        if let Some(o) = origin {
            req = req.header("origin", o);
        }
        let req = req
            .body(axum::body::Body::from(
                r#"{"role":"gameplay_engineer","brief":"a brief long enough"}"#,
            ))
            .unwrap();

        app.oneshot(req).await.unwrap().status()
    }

    #[tokio::test]
    async fn a_cross_origin_post_cannot_spawn_a_worker() {
        assert_eq!(post_with_origin(Some("http://evil.test")).await, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn a_same_origin_post_is_accepted() {
        assert_ne!(
            post_with_origin(Some("http://127.0.0.1:7878")).await,
            StatusCode::FORBIDDEN
        );
    }

    #[test]
    fn only_local_origins_are_accepted() {
        assert!(origin_is_local("http://127.0.0.1:7878"));
        assert!(origin_is_local("http://localhost:7878"));
        assert!(origin_is_local("http://localhost"));
        assert!(origin_is_local("http://[::1]:7878"));

        assert!(!origin_is_local("http://evil.test"));
        assert!(!origin_is_local("https://evil.test:7878"));
        assert!(!origin_is_local("http://127.0.0.1.evil.test"));
        assert!(!origin_is_local("http://notlocalhost"));
        assert!(!origin_is_local("null"));
        assert!(!origin_is_local("file://"));
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

#[cfg(test)]
mod approval_tests {
    use super::*;
    use std::sync::Arc;

    static NEXT_APPROVAL_DIR: std::sync::atomic::AtomicUsize =
        std::sync::atomic::AtomicUsize::new(0);

    fn state() -> AppState {
        let nth = NEXT_APPROVAL_DIR.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir()
            .join(format!("studio-approve-{}-{nth}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        AppState::new(Arc::new(Store::open(dir.join("s.db")).unwrap()))
    }

    #[test]
    fn a_waiting_run_receives_the_answer_the_floor_sends() {
        let s = state();
        let rx = s.await_approval("ask_1");
        assert!(s.resolve_approval("ask_1", true));
        assert_eq!(rx.recv().unwrap(), true);
    }

    #[test]
    fn a_refusal_reaches_the_waiting_run_too() {
        let s = state();
        let rx = s.await_approval("ask_2");
        assert!(s.resolve_approval("ask_2", false));
        assert_eq!(rx.recv().unwrap(), false);
    }

    #[test]
    fn answering_an_unknown_or_repeated_approval_is_reported_not_silently_dropped() {
        let s = state();
        assert!(!s.resolve_approval("never_asked", true));

        let _rx = s.await_approval("ask_3");
        assert!(s.resolve_approval("ask_3", true));
        assert!(
            !s.resolve_approval("ask_3", true),
            "the second answer must not claim success"
        );
    }

    #[test]
    fn a_run_whose_floor_closed_sees_the_channel_break_rather_than_hanging() {
        let s = state();
        let rx = s.await_approval("ask_4");
        s.approvals.lock().unwrap().clear();
        assert!(rx.recv().is_err());
    }
}

#[cfg(test)]
mod resume_boundary_tests {
    use super::*;
    use std::sync::Arc;
    use studio_events::{EventType, Scene};
    use tower::ServiceExt;

    fn state_with(run: &str, events: u64) -> AppState {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir()
            .join(format!("studio-resume-{run}-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Arc::new(Store::open(dir.join("s.db")).unwrap());
        for i in 0..events {
            store
                .append_event(
                    run,
                    "2026-07-25T09:12:44.118Z",
                    "gameplay_engineer#1",
                    EventType::ToolCall,
                    Scene::daemon(),
                    serde_json::json!({ "i": i }),
                )
                .unwrap();
        }
        AppState::new(store)
    }

    async fn resume(state: AppState, uri: &str) -> serde_json::Value {
        let res = router(state)
            .oneshot(
                axum::http::Request::builder()
                    .uri(uri)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn seqs(body: &serde_json::Value) -> Vec<u64> {
        body["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["seq"].as_u64().unwrap())
            .collect()
    }

    #[tokio::test]
    async fn a_replay_contains_the_event_whose_seq_is_from_seq_because_the_store_read_is_exclusive() {
        let body = resume(state_with("edge", 40), "/runs/edge/events?since_seq=9").await;
        assert_eq!(body["mode"], "replay");
        assert_eq!(
            seqs(&body).first().copied(),
            Some(10),
            "plan_resume returns from_seq = since_seq + 1 and the store reads seq > ?2, \
             so the read must be handed from_seq - 1; handing it from_seq drops event 10 \
             from every replay and every websocket reconnect"
        );
        assert_eq!(seqs(&body).last().copied(), Some(40));
        assert_eq!(seqs(&body).len(), 31);
    }

    #[tokio::test]
    async fn two_resumes_either_side_of_a_cut_reproduce_the_log_with_no_gap_and_no_repeat() {
        let state = state_with("stitch", 60);
        let first = resume(state.clone(), "/runs/stitch/events?since_seq=0").await;
        let second = resume(state, "/runs/stitch/events?since_seq=37").await;

        let mut stitched = seqs(&first);
        let tail = seqs(&second);
        let cut = stitched.iter().position(|s| *s > 37).unwrap();
        stitched.truncate(cut);
        stitched.extend(tail);

        assert_eq!(stitched, (1..=60).collect::<Vec<u64>>());
    }

    #[tokio::test]
    async fn a_resume_from_zero_replays_the_whole_log() {
        let body = resume(state_with("whole", 12), "/runs/whole/events?since_seq=0").await;
        assert_eq!(seqs(&body), (1..=12).collect::<Vec<u64>>());
    }

    #[tokio::test]
    async fn a_client_that_is_already_current_is_sent_nothing() {
        let body = resume(state_with("current", 25), "/runs/current/events?since_seq=25").await;
        assert_eq!(body["mode"], "up_to_date");
        assert_eq!(body["head"], 25);
        assert!(seqs(&body).is_empty());
    }

    #[tokio::test]
    async fn the_head_of_a_run_is_reported_without_replaying_it() {
        let body = resume(state_with("headline", 31), "/runs/headline/snapshot").await;
        assert_eq!(body["head"], 31, "the snapshot head must survive being read separately from the log");
        assert!(!seqs(&body).is_empty());
    }

    #[tokio::test]
    async fn a_run_nobody_has_written_resumes_as_empty_rather_than_failing() {
        let body = resume(state_with("blank", 0), "/runs/blank/events?since_seq=0").await;
        assert_eq!(body["head"], 0);
        assert!(seqs(&body).is_empty());
    }
}
