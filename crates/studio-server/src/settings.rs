use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::AppState;
use studio_core::{probe_answered, BriefDelivery, Provider, RoleNeeds, PROBE_QUESTION};
use studio_settings::models::{self, Candidate, ProbeLog, ProbeRecord, Verdict};
use studio_settings::Settings;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/settings", get(read_settings).post(write_settings))
        .route("/providers", get(providers))
        .route("/models", get(model_catalogue))
        .route("/models/probe", axum::routing::post(probe_models))
        .route("/limits", get(limits))
        .route("/music", get(music_list))
        .route("/music/track", get(music_track))
}

fn settings_path(state: &AppState) -> PathBuf {
    Settings::path_in(&state.studio_dir)
}

async fn read_settings(State(state): State<AppState>) -> Response {
    match Settings::load(&settings_path(&state)) {
        Ok(s) => axum::Json(s.to_value()).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(
                "could not read {}: {e}; fix or delete that file and reload",
                settings_path(&state).display()
            ),
        )
            .into_response(),
    }
}

async fn write_settings(State(state): State<AppState>, body: String) -> Response {
    let incoming: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("that is not json: {e}")).into_response()
        }
    };
    let Value::Object(incoming) = incoming else {
        return (
            StatusCode::BAD_REQUEST,
            "settings are a flat json object of key to value".to_string(),
        )
            .into_response();
    };

    let path = settings_path(&state);
    let mut stored = match Settings::load(&path) {
        Ok(s) => s,
        Err(_) => Settings::new(),
    };
    stored.merge(&incoming);

    match stored.save(&path) {
        Ok(()) => axum::Json(stored.to_value()).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("could not write {}: {e}", path.display()),
        )
            .into_response(),
    }
}

pub fn on_path(program: &str) -> Option<PathBuf> {
    studio_core::resolve(program)
}

fn provider_row(p: Provider) -> Value {
    let caps = p.capabilities();
    let installed = on_path(p.program());
    serde_json::json!({
        "id": p.id(),
        "title": p.title(),
        "program": p.program(),
        "installed": installed.is_some(),
        "path": installed.map(|b| b.to_string_lossy().into_owned()),
        "flags_verified": p.flags_were_read(),
        "capabilities": {
            "streaming_events": caps.streaming_events,
            "usage_reporting": caps.usage_reporting,
            "tool_restriction": caps.tool_restriction,
            "system_prompt_file": caps.system_prompt_file,
            "structured_output": caps.structured_output,
            "session_control": caps.session_control,
        },
        "blockers": p.blockers(RoleNeeds { structured_output: false, restricted_tools: true }),
        "plan_blockers": p.blockers(RoleNeeds { structured_output: true, restricted_tools: true }),
    })
}

async fn providers() -> Response {
    let rows: Vec<Value> = Provider::ALL.into_iter().map(provider_row).collect();
    axum::Json(rows).into_response()
}

const PROBE_TIMEOUT: Duration = Duration::from_secs(180);

fn probe_log_path(state: &AppState) -> PathBuf {
    ProbeLog::path_in(&state.studio_dir)
}

fn read_codex_config() -> Option<String> {
    std::fs::read_to_string(models::codex_config_path()?).ok()
}

fn candidate_row(provider: Provider, c: &Candidate, log: &ProbeLog) -> Value {
    let seen = log.find(provider.id(), &c.id);
    serde_json::json!({
        "id": c.id,
        "label": c.label,
        "sources": c.sources.iter().map(|s| serde_json::json!({
            "id": s.as_str(),
            "explain": s.explain(),
        })).collect::<Vec<_>>(),
        "efforts": c.efforts,
        "default_effort": c.default_effort,
        "context_window": c.context_window,
        "verdict": seen.map(|r| r.verdict).unwrap_or(Verdict::Unknown).as_str(),
        "detail": seen.and_then(|r| r.detail.clone()),
        "checked_at": seen.map(|r| r.checked_at.clone()),
        "seconds": seen.map(|r| r.seconds),
        "cost_usd": seen.and_then(|r| r.cost_usd),
        "tokens": seen.and_then(|r| r.tokens),
    })
}

const CATALOGUE_TIMEOUT: Duration = Duration::from_secs(30);
static CATALOGUES: Mutex<Vec<(String, Instant, String)>> = Mutex::new(Vec::new());
const CATALOGUE_TTL: Duration = Duration::from_secs(60);

pub fn read_catalogue(provider: Provider) -> Option<String> {
    let argv = models::catalogue_argv(provider.id())?;
    if on_path(provider.program()).is_none() {
        return None;
    }

    if let Ok(cached) = CATALOGUES.lock() {
        if let Some((_, at, body)) = cached
            .iter()
            .find(|(id, at, _)| id == provider.id() && at.elapsed() < CATALOGUE_TTL)
        {
            let _ = at;
            return Some(body.clone());
        }
    }

    let (_, stdout, _, _) =
        run_bounded(provider.program(), &argv, "", CATALOGUE_TIMEOUT).ok()?;
    if stdout.trim().is_empty() {
        return None;
    }

    if let Ok(mut cached) = CATALOGUES.lock() {
        cached.retain(|(id, _, _)| id != provider.id());
        cached.push((provider.id().to_string(), Instant::now(), stdout.clone()));
    }
    Some(stdout)
}

fn catalogue_for(state: &AppState, provider: Provider) -> Value {
    let settings = Settings::load(&settings_path(state)).unwrap_or_default();
    let log = ProbeLog::load(&probe_log_path(state));
    let codex_config = if provider == Provider::Codex {
        read_codex_config()
    } else {
        None
    };
    let catalogue = read_catalogue(provider);
    let found = models::candidates(
        provider.id(),
        &settings,
        &log,
        codex_config.as_deref(),
        catalogue.as_deref(),
    );

    serde_json::json!({
        "provider": provider.id(),
        "title": provider.title(),
        "program": provider.program(),
        "installed": on_path(provider.program()).is_some(),
        "probeable": provider.probe_args("").is_some(),
        "has_catalogue": models::catalogue_argv(provider.id()).is_some(),
        "catalogue_read": catalogue.is_some(),
        "discovery": models::discovery(provider.id()),
        "provenance": models::provenance(provider.id()),
        "candidates": found.iter().map(|c| candidate_row(provider, c, &log)).collect::<Vec<_>>(),
    })
}

async fn model_catalogue(State(state): State<AppState>) -> Response {
    let rows: Vec<Value> = Provider::ALL
        .into_iter()
        .map(|p| catalogue_for(&state, p))
        .collect();

    axum::Json(serde_json::json!({
        "providers": rows,
        "probe": {
            "question": PROBE_QUESTION,
            "automatic": false,
            "cost": "one real request per model, billed to that CLI's own subscription, and up to three minutes each while it answers",
            "route": "POST /models/probe with {\"provider\": \"codex\", \"models\": [\"gpt-5.6-luna\"]}",
            "why_still_useful": "a catalogue says a model exists; only asking it says this account may spawn it",
        },
    }))
    .into_response()
}

pub fn run_bounded(
    program: &str,
    args: &[String],
    stdin_text: &str,
    limit: Duration,
) -> std::io::Result<(Option<i32>, String, String, f64)> {
    use std::io::{Read, Write};
    use std::process::Stdio;

    let started = Instant::now();
    let mut group = studio_core::ProcessGroup::new()?;
    let found = studio_core::resolve(program).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "{program} is not on PATH as anything this OS can execute; check it is \
                 installed and that its directory is on PATH"
            ),
        )
    })?;
    let mut cmd = studio_core::command(found);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    group.prepare(&mut cmd);

    let mut child = cmd.spawn()?;
    group.adopt(&child)?;

    if let Some(mut stdin) = child.stdin.take() {
        let payload = stdin_text.as_bytes().to_vec();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&payload);
        });
    }

    let drain = |mut pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut raw = Vec::new();
            if let Some(p) = pipe.as_mut() {
                let _ = p.read_to_end(&mut raw);
            }
            String::from_utf8_lossy(&raw).into_owned()
        })
    };
    let out_pump = drain(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let err_pump = drain(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));

    let mut code = None;
    loop {
        if let Some(status) = child.try_wait()? {
            code = status.code();
            break;
        }
        if started.elapsed() >= limit {
            let _ = group.kill_tree();
            let _ = child.wait();
            break;
        }
        std::thread::sleep(Duration::from_millis(30));
    }

    let stdout = out_pump.join().unwrap_or_default();
    let stderr = err_pump.join().unwrap_or_default();
    Ok((code, stdout, stderr, started.elapsed().as_secs_f64()))
}

fn is_noise(line: &str) -> bool {
    let noisy = [
        "Cannot POST /mcp",
        "rmcp::transport",
        "<!DOCTYPE",
        "<html",
        "</html>",
        "worker quit with fatal",
        "<head>",
        "</head>",
        "<body>",
        "</body>",
        "<meta",
        "<title>",
        "<pre>",
    ];
    line.trim().is_empty() || noisy.iter().any(|n| line.contains(n))
}

pub fn explain_refusal(stdout: &str, stderr: &str) -> Option<String> {
    const STRONGEST_FIRST: [&str; 7] = [
        "not supported",
        "invalid_request_error",
        "issue with the selected model",
        "unauthorized",
        "unknown model",
        "does not exist",
        "not found",
    ];

    let lines: Vec<&str> = stdout
        .lines()
        .chain(stderr.lines())
        .filter(|l| !is_noise(l))
        .collect();

    let pick = STRONGEST_FIRST
        .iter()
        .find_map(|marker| {
            lines
                .iter()
                .rev()
                .find(|l| l.to_lowercase().contains(marker))
        })
        .or_else(|| lines.last())?;

    Some(pick.trim().chars().take(400).collect())
}

fn probe_one(provider: Provider, model: &str, now: String) -> ProbeRecord {
    let unknown = |detail: &str| ProbeRecord {
        provider: provider.id().into(),
        model: model.to_string(),
        verdict: Verdict::Unknown,
        detail: Some(detail.to_string()),
        checked_at: now.clone(),
        seconds: 0.0,
        cost_usd: None,
        tokens: None,
    };

    let Some(args) = provider.probe_args(model) else {
        return unknown(
            "the studio has never read this CLI's flags, so it will not guess a command line to probe with",
        );
    };
    if on_path(provider.program()).is_none() {
        return unknown(&format!("{} is not on PATH, so nothing was run", provider.program()));
    }

    let stdin_text = match provider.brief_delivery() {
        BriefDelivery::Stdin => PROBE_QUESTION,
        _ => "",
    };

    let run = run_bounded(provider.program(), &args, stdin_text, PROBE_TIMEOUT);
    let (_, stdout, stderr, seconds) = match run {
        Ok(v) => v,
        Err(e) => {
            let mut record = unknown(&format!("could not start {}: {e}", provider.program()));
            record.seconds = 0.0;
            return record;
        }
    };

    let terminal: Option<Value> = stdout
        .lines()
        .rev()
        .find_map(|l| serde_json::from_str::<Value>(l.trim()).ok());
    let cost_usd = terminal
        .as_ref()
        .and_then(|v| v.get("total_cost_usd"))
        .and_then(Value::as_f64);
    let tokens = tokens_used(&stdout).or_else(|| tokens_used(&stderr));

    let answered = probe_answered(&stdout);
    let spoken = terminal
        .as_ref()
        .and_then(|v| v.get("result"))
        .and_then(Value::as_str)
        .map(|s| s.trim().chars().take(400).collect::<String>());

    ProbeRecord {
        provider: provider.id().into(),
        model: model.to_string(),
        verdict: if answered { Verdict::Working } else { Verdict::Refused },
        detail: if answered {
            None
        } else {
            spoken.or_else(|| explain_refusal(&stdout, &stderr))
        },
        checked_at: now,
        seconds,
        cost_usd,
        tokens,
    }
}

pub fn tokens_used(stdout: &str) -> Option<u64> {
    let lines: Vec<&str> = stdout.lines().map(str::trim).collect();
    let at = lines.iter().rposition(|l| *l == "tokens used")?;
    let raw = lines.get(at + 1)?.replace(',', "");
    raw.parse().ok()
}

#[derive(Debug, Deserialize)]
pub struct ProbeRequest {
    pub provider: String,
    #[serde(default)]
    pub models: Vec<String>,
}

async fn probe_models(
    State(state): State<AppState>,
    axum::Json(req): axum::Json<ProbeRequest>,
) -> Response {
    let Some(provider) = Provider::from_id(&req.provider) else {
        return (
            StatusCode::BAD_REQUEST,
            format!("{} is not a provider the studio knows", req.provider),
        )
            .into_response();
    };
    let wanted: Vec<String> = req
        .models
        .iter()
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .collect();
    if wanted.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "name at least one model to check; probing every model on every panel open would spend your subscription without being asked".to_string(),
        )
            .into_response();
    }
    if wanted.len() > 12 {
        return (
            StatusCode::BAD_REQUEST,
            format!(
                "{} models at once is more than this will do in one request; each one is a real billed call",
                wanted.len()
            ),
        )
            .into_response();
    }

    let path = probe_log_path(&state);
    let done = tokio::task::spawn_blocking(move || {
        let mut log = ProbeLog::load(&path);
        let mut fresh = Vec::new();
        for model in wanted {
            let record = probe_one(provider, &model, crate::now_rfc3339());
            log.record(record.clone());
            fresh.push(record);
        }
        let saved = log.save(&path);
        (fresh, saved.err().map(|e| e.to_string()))
    })
    .await;

    match done {
        Ok((fresh, cache_error)) => axum::Json(serde_json::json!({
            "provider": provider.id(),
            "checked": fresh,
            "cache_error": cache_error,
        }))
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("the probe did not finish: {e}"),
        )
            .into_response(),
    }
}

static OBSERVED_WINDOWS: Mutex<Vec<(String, Value, String)>> = Mutex::new(Vec::new());

pub fn observe_rate_limit(raw: &Value) {
    let Some(info) = raw.get("rate_limit_info") else {
        return;
    };
    let kind = info
        .get("rateLimitType")
        .and_then(Value::as_str)
        .unwrap_or("unnamed")
        .to_string();
    let seen_at = crate::now_rfc3339();
    let Ok(mut windows) = OBSERVED_WINDOWS.lock() else {
        return;
    };
    windows.retain(|(k, _, _)| k != &kind);
    windows.push((kind, info.clone(), seen_at));
}

pub fn observed_windows() -> Vec<Value> {
    let Ok(windows) = OBSERVED_WINDOWS.lock() else {
        return Vec::new();
    };
    windows
        .iter()
        .map(|(kind, info, seen_at)| {
            serde_json::json!({
                "window": kind,
                "status": info.get("status").and_then(Value::as_str),
                "resets_at": info.get("resetsAt").and_then(Value::as_u64),
                "using_overage": info.get("isUsingOverage").and_then(Value::as_bool),
                "observed_at": seen_at,
            })
        })
        .collect()
}

static ACCOUNT: Mutex<Option<(Instant, Value)>> = Mutex::new(None);
const ACCOUNT_TTL: Duration = Duration::from_secs(60);

fn read_account() -> Value {
    if let Ok(cached) = ACCOUNT.lock() {
        if let Some((at, value)) = cached.as_ref() {
            if at.elapsed() < ACCOUNT_TTL {
                return value.clone();
            }
        }
    }

    let fresh = ask_the_cli_who_is_logged_in();
    if let Ok(mut cached) = ACCOUNT.lock() {
        *cached = Some((Instant::now(), fresh.clone()));
    }
    fresh
}

fn ask_the_cli_who_is_logged_in() -> Value {
    if on_path("claude").is_none() {
        return serde_json::json!({
            "known": false,
            "reason": "claude is not on PATH, so the studio cannot ask which plan is signed in",
        });
    }

    let out = std::process::Command::new("claude")
        .args(["auth", "status", "--json"])
        .output();

    let out = match out {
        Ok(o) => o,
        Err(e) => {
            return serde_json::json!({
                "known": false,
                "reason": format!("claude auth status did not run: {e}"),
            })
        }
    };

    let parsed: Option<Value> = serde_json::from_slice(&out.stdout).ok();
    match parsed {
        Some(v) if v.get("loggedIn").and_then(Value::as_bool) == Some(true) => serde_json::json!({
            "known": true,
            "plan": v.get("subscriptionType").and_then(Value::as_str),
            "account": v.get("email").and_then(Value::as_str),
            "method": v.get("authMethod").and_then(Value::as_str),
            "source": "claude auth status --json",
        }),
        Some(_) => serde_json::json!({
            "known": false,
            "reason": "claude reports nobody is signed in; run claude auth login",
        }),
        None => serde_json::json!({
            "known": false,
            "reason": "claude auth status returned something this build could not read",
        }),
    }
}

fn a_day_ago() -> String {
    let cutoff = time::OffsetDateTime::now_utc() - time::Duration::hours(24);
    cutoff
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

async fn limits(State(state): State<AppState>) -> Response {
    let since = a_day_ago();
    let ledger = match state.store.cache_health(&since) {
        Ok(rows) => {
            let read: u64 = rows.iter().map(|r| r.cache_read).sum();
            let written: u64 = rows.iter().map(|r| r.cache_creation).sum();
            let total = read + written;
            serde_json::json!({
                "known": total > 0,
                "since": since,
                "cache_read": read,
                "cache_creation": written,
                "hit_ratio": if total == 0 { None } else { Some(read as f64 / total as f64) },
                "prefixes": rows.len(),
            })
        }
        Err(e) => serde_json::json!({"known": false, "reason": e.to_string()}),
    };

    let windows = observed_windows();
    let note = if windows.is_empty() {
        "no worker has reported a window yet; the CLI only mentions its rate limits mid-stream, so this fills in the first time the crew runs"
    } else {
        "the CLI names the window in force and when it resets, never how much of it is left, so no percentage is shown"
    };

    axum::Json(serde_json::json!({
        "account": read_account(),
        "windows": windows,
        "note": note,
        "ledger": ledger,
    }))
    .into_response()
}

const TRACK_EXTENSIONS: [(&str, &str); 7] = [
    ("mp3", "audio/mpeg"),
    ("ogg", "audio/ogg"),
    ("oga", "audio/ogg"),
    ("wav", "audio/wav"),
    ("flac", "audio/flac"),
    ("m4a", "audio/mp4"),
    ("opus", "audio/opus"),
];

pub fn content_type_of(name: &str) -> Option<&'static str> {
    let ext = name.rsplit_once('.')?.1.to_lowercase();
    TRACK_EXTENSIONS
        .iter()
        .find(|(e, _)| *e == ext)
        .map(|(_, mime)| *mime)
}

pub fn music_folder(state: &AppState) -> PathBuf {
    let stored = Settings::load(&settings_path(state)).unwrap_or_default();
    match stored.string("music.folder").map(str::trim).filter(|s| !s.is_empty()) {
        Some(chosen) => PathBuf::from(chosen),
        None => state.studio_dir.join("music"),
    }
}

pub fn tracks_in(folder: &Path) -> Vec<(String, u64)> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut out: Vec<(String, u64)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            content_type_of(&name)?;
            let bytes = e.metadata().ok()?.len();
            Some((name, bytes))
        })
        .collect();
    out.sort_by_key(|(name, _)| name.to_lowercase());
    out
}

async fn music_list(State(state): State<AppState>) -> Response {
    let folder = music_folder(&state);
    let found = tracks_in(&folder);
    axum::Json(serde_json::json!({
        "folder": folder.to_string_lossy(),
        "exists": folder.is_dir(),
        "tracks": found
            .into_iter()
            .map(|(name, bytes)| serde_json::json!({"name": name, "bytes": bytes}))
            .collect::<Vec<_>>(),
        "playable": TRACK_EXTENSIONS.iter().map(|(e, _)| *e).collect::<Vec<_>>(),
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
pub struct TrackQuery {
    pub name: String,
}

pub fn is_a_plain_file_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains("..")
        && !name.contains(':')
}

pub fn slice_for(range: Option<&str>, len: u64) -> Option<(u64, u64)> {
    let spec = range?.trim().strip_prefix("bytes=")?;
    let (from, to) = spec.split_once('-')?;
    if len == 0 {
        return None;
    }

    let (start, end) = if from.is_empty() {
        let tail: u64 = to.parse().ok()?;
        (len.saturating_sub(tail.min(len)), len - 1)
    } else {
        let start: u64 = from.parse().ok()?;
        let end = if to.is_empty() {
            len - 1
        } else {
            to.parse::<u64>().ok()?.min(len - 1)
        };
        (start, end)
    };

    if start > end || start >= len {
        return None;
    }
    Some((start, end))
}

async fn music_track(
    State(state): State<AppState>,
    Query(q): Query<TrackQuery>,
    headers: HeaderMap,
) -> Response {
    if !is_a_plain_file_name(&q.name) {
        return (
            StatusCode::BAD_REQUEST,
            "a track is named by its file name inside the music folder, with no path".to_string(),
        )
            .into_response();
    }
    let Some(mime) = content_type_of(&q.name) else {
        return (
            StatusCode::BAD_REQUEST,
            format!("{} is not an audio file the browser will play", q.name),
        )
            .into_response();
    };

    let path = music_folder(&state).join(&q.name);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                format!("could not open {}: {e}", path.display()),
            )
                .into_response()
        }
    };

    let len = bytes.len() as u64;
    let asked = headers.get(header::RANGE).and_then(|v| v.to_str().ok());

    match slice_for(asked, len) {
        Some((start, end)) => (
            StatusCode::PARTIAL_CONTENT,
            [
                (header::CONTENT_TYPE, mime.to_string()),
                (header::ACCEPT_RANGES, "bytes".to_string()),
                (
                    header::CONTENT_RANGE,
                    format!("bytes {start}-{end}/{len}"),
                ),
                (header::CACHE_CONTROL, "no-store".to_string()),
            ],
            bytes[start as usize..=end as usize].to_vec(),
        )
            .into_response(),
        None => (
            [
                (header::CONTENT_TYPE, mime.to_string()),
                (header::ACCEPT_RANGES, "bytes".to_string()),
                (header::CACHE_CONTROL, "no-store".to_string()),
            ],
            bytes,
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use studio_store::Store;
    use tower::ServiceExt;

    fn state_in(slug: &str) -> AppState {
        let dir = std::env::temp_dir().join(format!("studio-settings-route-{slug}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = Arc::new(Store::open(dir.join("s.db")).unwrap());
        AppState::new(store).with_studio_dir(dir)
    }

    async fn body_of(state: AppState, uri: &str) -> (StatusCode, Value) {
        let req = axum::http::Request::builder()
            .uri(uri)
            .body(axum::body::Body::empty())
            .unwrap();
        let res = crate::router(state).oneshot(req).await.unwrap();
        let status = res.status();
        let raw = axum::body::to_bytes(res.into_body(), 4_000_000).await.unwrap();
        (status, serde_json::from_slice(&raw).unwrap_or(Value::Null))
    }

    async fn post_settings(state: AppState, body: &str) -> (StatusCode, Value) {
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/settings")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body.to_string()))
            .unwrap();
        let res = crate::router(state).oneshot(req).await.unwrap();
        let status = res.status();
        let raw = axum::body::to_bytes(res.into_body(), 1_000_000).await.unwrap();
        (status, serde_json::from_slice(&raw).unwrap_or(Value::Null))
    }

    #[tokio::test]
    async fn a_studio_with_no_settings_file_answers_with_an_empty_object() {
        let (status, body) = body_of(state_in("empty"), "/settings").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, serde_json::json!({}));
    }

    #[tokio::test]
    async fn a_saved_setting_is_on_disk_where_the_daemon_reads_it() {
        let state = state_in("saved");
        let (status, _) = post_settings(state.clone(), r#"{"models.tier2":"haiku"}"#).await;
        assert_eq!(status, StatusCode::OK);

        let stored = Settings::load(&settings_path(&state)).unwrap();
        assert_eq!(stored.string("models.tier2"), Some("haiku"));
    }

    #[tokio::test]
    async fn a_second_save_merges_rather_than_replacing_what_the_first_one_wrote() {
        let state = state_in("merge");
        post_settings(state.clone(), r#"{"models.tier2":"haiku"}"#).await;
        post_settings(state.clone(), r#"{"lowSpec":true}"#).await;

        let (_, body) = body_of(state, "/settings").await;
        assert_eq!(body["models.tier2"], "haiku");
        assert_eq!(body["lowSpec"], true);
    }

    #[tokio::test]
    async fn a_setting_can_be_changed_back_after_it_was_saved() {
        let state = state_in("rewrite");
        post_settings(state.clone(), r#"{"provider":"gemini"}"#).await;
        post_settings(state.clone(), r#"{"provider":"claude"}"#).await;

        let (_, body) = body_of(state, "/settings").await;
        assert_eq!(body["provider"], "claude");
    }

    #[tokio::test]
    async fn a_posted_array_is_refused_with_a_sentence_that_says_what_was_wanted() {
        let (status, _) = post_settings(state_in("array"), "[1,2]").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn every_provider_the_studio_knows_is_listed_with_what_it_cannot_do() {
        let (status, body) = body_of(state_in("providers"), "/providers").await;
        assert_eq!(status, StatusCode::OK);
        let rows = body.as_array().unwrap();
        assert_eq!(rows.len(), Provider::ALL.len());

        let claude = rows.iter().find(|r| r["id"] == "claude").unwrap();
        assert_eq!(claude["blockers"].as_array().unwrap().len(), 0);
        assert_eq!(claude["capabilities"]["system_prompt_file"], true);

        let gemini = rows.iter().find(|r| r["id"] == "gemini").unwrap();
        assert!(!gemini["blockers"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_limits_view_says_the_windows_are_unread_rather_than_guessing_at_them() {
        let (status, body) = body_of(state_in("limits"), "/limits").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["note"].as_str().unwrap().len() > 20);
        assert!(body["ledger"]["known"].is_boolean());
        assert!(body["account"]["known"].is_boolean());
    }

    #[test]
    fn a_window_the_stream_reported_is_kept_with_the_reset_the_cli_gave() {
        let raw = serde_json::json!({
            "type": "rate_limit_event",
            "rate_limit_info": {
                "status": "allowed",
                "resetsAt": 1784575200u64,
                "rateLimitType": "five_hour",
                "isUsingOverage": false,
            }
        });
        observe_rate_limit(&raw);

        let seen = observed_windows();
        let five = seen.iter().find(|w| w["window"] == "five_hour").unwrap();
        assert_eq!(five["resets_at"], 1784575200u64);
        assert_eq!(five["status"], "allowed");
        assert!(
            five.get("remaining").is_none(),
            "the CLI never says how much is left; the studio must not appear to know"
        );
    }

    #[test]
    fn a_line_carrying_no_rate_limit_block_is_ignored_rather_than_stored_empty() {
        let before = observed_windows().len();
        observe_rate_limit(&serde_json::json!({"type": "result"}));
        assert_eq!(observed_windows().len(), before);
    }

    #[tokio::test]
    async fn a_music_folder_that_does_not_exist_yet_is_reported_as_missing_not_as_empty() {
        let (status, body) = body_of(state_in("music-missing"), "/music").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["exists"], false);
        assert_eq!(body["tracks"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn only_files_the_browser_can_play_are_offered_as_tracks() {
        let state = state_in("music-list");
        let folder = state.studio_dir.join("music");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("b-theme.mp3"), b"id3").unwrap();
        std::fs::write(folder.join("a-theme.ogg"), b"ogg").unwrap();
        std::fs::write(folder.join("notes.txt"), b"not music").unwrap();

        let (_, body) = body_of(state, "/music").await;
        let names: Vec<&str> = body["tracks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["a-theme.ogg", "b-theme.mp3"]);
    }

    #[tokio::test]
    async fn a_chosen_folder_replaces_the_default_one_under_the_studio_directory() {
        let state = state_in("music-folder");
        let elsewhere = std::env::temp_dir().join("studio-music-elsewhere");
        let _ = std::fs::create_dir_all(&elsewhere);
        std::fs::write(elsewhere.join("score.mp3"), b"id3").unwrap();

        post_settings(
            state.clone(),
            &serde_json::json!({"music.folder": elsewhere.to_string_lossy()}).to_string(),
        )
        .await;

        let (_, body) = body_of(state, "/music").await;
        assert_eq!(body["exists"], true);
        assert_eq!(body["tracks"][0]["name"], "score.mp3");
    }

    #[tokio::test]
    async fn a_track_name_that_climbs_out_of_the_music_folder_is_refused() {
        let state = state_in("music-escape");
        let req = axum::http::Request::builder()
            .uri("/music/track?name=..%2F..%2Fstudio-state.db")
            .body(axum::body::Body::empty())
            .unwrap();
        let res = crate::router(state).oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn a_track_is_served_with_the_content_type_the_audio_element_needs() {
        let state = state_in("music-serve");
        let folder = state.studio_dir.join("music");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("loop.mp3"), b"0123456789").unwrap();

        let req = axum::http::Request::builder()
            .uri("/music/track?name=loop.mp3")
            .body(axum::body::Body::empty())
            .unwrap();
        let res = crate::router(state).oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()[header::CONTENT_TYPE], "audio/mpeg");
        assert_eq!(res.headers()[header::ACCEPT_RANGES], "bytes");
    }

    #[tokio::test]
    async fn seeking_a_track_returns_only_the_bytes_that_were_asked_for() {
        let state = state_in("music-range");
        let folder = state.studio_dir.join("music");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("loop.mp3"), b"0123456789").unwrap();

        let req = axum::http::Request::builder()
            .uri("/music/track?name=loop.mp3")
            .header("range", "bytes=2-5")
            .body(axum::body::Body::empty())
            .unwrap();
        let res = crate::router(state).oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(res.headers()[header::CONTENT_RANGE], "bytes 2-5/10");

        let raw = axum::body::to_bytes(res.into_body(), 100).await.unwrap();
        assert_eq!(&raw[..], b"2345");
    }

    #[test]
    fn a_range_the_file_cannot_satisfy_falls_back_to_sending_the_whole_thing() {
        assert_eq!(slice_for(Some("bytes=0-3"), 10), Some((0, 3)));
        assert_eq!(slice_for(Some("bytes=5-"), 10), Some((5, 9)));
        assert_eq!(slice_for(Some("bytes=-3"), 10), Some((7, 9)));
        assert_eq!(slice_for(Some("bytes=0-99"), 10), Some((0, 9)));
        assert_eq!(slice_for(Some("bytes=20-30"), 10), None);
        assert_eq!(slice_for(Some("pages=1-2"), 10), None);
        assert_eq!(slice_for(None, 10), None);
    }

    #[test]
    fn a_name_that_is_really_a_path_never_reaches_the_filesystem() {
        assert!(is_a_plain_file_name("theme.mp3"));
        assert!(!is_a_plain_file_name("../theme.mp3"));
        assert!(!is_a_plain_file_name("sub/theme.mp3"));
        assert!(!is_a_plain_file_name(r"sub\theme.mp3"));
        assert!(!is_a_plain_file_name("C:theme.mp3"));
        assert!(!is_a_plain_file_name(""));
    }

    #[test]
    fn the_daemon_is_on_the_path_it_claims_to_spawn_workers_from() {
        assert_eq!(on_path("a-program-nobody-installed-xyz"), None);
    }

    const CODEX_WORKED_STDOUT: &str = "42\n";

    const CODEX_WORKED_STDERR: &str = concat!(
        "OpenAI Codex v0.145.0\n",
        "--------\n",
        "model: gpt-5.6-luna\n",
        "--------\n",
        "user\n",
        "what is 17 plus 25? reply with just the number\n",
        "codex\n",
        "42\n",
        "tokens used\n",
        "1,668\n"
    );

    const CODEX_REFUSED_STDOUT: &str = "";

    const CODEX_REFUSED_STDERR: &str = concat!(
        "2026-07-25T12:06:57.721399Z ERROR rmcp::transport::worker: worker quit with fatal: ",
        "Transport channel closed, when UnexpectedServerResponse(\"HTTP 404: <!DOCTYPE html>\")\n",
        "<pre>Cannot POST /mcp</pre>\n",
        "OpenAI Codex v0.145.0\n",
        "user\n",
        "what is 17 plus 25? reply with just the number\n",
        "warning: Model metadata for `gpt-5.2-codex` not found. Defaulting to fallback metadata; this can degrade performance and cause issues.\n",
        "ERROR: {\"type\":\"error\",\"status\":400,\"error\":{\"type\":\"invalid_request_error\",\"message\":\"The 'gpt-5.2-codex' model is not supported when using Codex with a ChatGPT account.\"}}\n"
    );

    #[test]
    fn a_refusal_is_explained_in_the_cli_s_own_words_not_by_a_dead_mcp_server() {
        let said = explain_refusal(CODEX_REFUSED_STDOUT, CODEX_REFUSED_STDERR).unwrap();
        assert!(
            said.contains("not supported when using Codex with a ChatGPT account"),
            "got {said}"
        );
        assert!(
            !said.contains("Cannot POST /mcp"),
            "this machine's dead MCP server must never be reported as the model's problem: {said}"
        );
        assert!(
            !said.contains("Model metadata"),
            "a fallback-metadata warning is not the reason the model was refused: {said}"
        );
    }

    #[test]
    fn a_run_that_says_nothing_useful_still_reports_something_rather_than_nothing() {
        assert!(explain_refusal("", "<pre>Cannot POST /mcp</pre>\n").is_none());
        assert_eq!(
            explain_refusal("something odd happened", "").as_deref(),
            Some("something odd happened")
        );
    }

    #[test]
    fn a_working_codex_run_is_recognised_by_the_answer_it_puts_on_stdout() {
        assert!(probe_answered(CODEX_WORKED_STDOUT));
        assert!(
            !probe_answered(CODEX_REFUSED_STDOUT),
            "a refused model prints nothing at all on stdout"
        );
    }

    #[test]
    fn the_token_count_is_found_even_though_codex_prints_it_on_stderr() {
        assert_eq!(
            tokens_used(CODEX_WORKED_STDOUT),
            None,
            "stdout carries only the answer, so reading stdout alone loses the count"
        );
        assert_eq!(tokens_used(CODEX_WORKED_STDERR), Some(1668));
        assert_eq!(tokens_used(CODEX_REFUSED_STDERR), None);
    }

    #[test]
    fn a_cli_that_merges_its_streams_still_cannot_pass_on_the_echo_alone() {
        let merged = format!("{CODEX_WORKED_STDERR}");
        assert!(probe_answered(&merged), "the real answer is in there too");
        let echo_only = concat!("user\n", "what is 17 plus 25? reply with just the number\n", "codex\n");
        assert!(!probe_answered(echo_only));
    }

    #[tokio::test]
    async fn every_provider_reports_a_catalogue_and_says_where_the_names_came_from() {
        let (status, body) = body_of(state_in("models"), "/models").await;
        assert_eq!(status, StatusCode::OK);

        let rows = body["providers"].as_array().unwrap();
        assert_eq!(rows.len(), Provider::ALL.len());
        assert_eq!(body["probe"]["automatic"], false);

        for row in rows {
            assert!(
                row["provenance"].as_str().unwrap().len() > 20,
                "{} does not say where its list came from",
                row["provider"]
            );
        }

        let claude = rows.iter().find(|r| r["provider"] == "claude").unwrap();
        let ids: Vec<&str> = claude["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"sonnet"), "sonnet is expressible now and must be offered");
    }

    #[tokio::test]
    async fn a_cli_with_its_own_catalogue_is_read_for_free_and_the_rest_cost_a_request() {
        let (_, body) = body_of(state_in("models-discovery"), "/models").await;
        let rows = body["providers"].as_array().unwrap();

        let codex = rows.iter().find(|r| r["provider"] == "codex").unwrap();
        assert_eq!(codex["has_catalogue"], true);
        assert_eq!(codex["discovery"], "a free local catalogue call");

        for id in ["claude", "gemini", "copilot", "kimi"] {
            let row = rows.iter().find(|r| r["provider"] == id).unwrap();
            assert_eq!(row["has_catalogue"], false, "{id} has no catalogue subcommand");
            assert_eq!(row["discovery"], "one real billed request per model");
        }

        assert!(
            body["probe"]["why_still_useful"].as_str().unwrap().contains("account"),
            "a catalogue listing is not entitlement and the payload must say so"
        );
    }

    #[test]
    fn a_provider_with_no_catalogue_is_never_asked_for_one() {
        for p in [Provider::Claude, Provider::Gemini, Provider::Copilot, Provider::Kimi] {
            assert_eq!(
                read_catalogue(p),
                None,
                "{} has no catalogue call, so nothing should be spawned for it",
                p.id()
            );
        }
    }

    #[tokio::test]
    async fn a_model_nobody_has_checked_reads_as_unknown_rather_than_as_working() {
        let (_, body) = body_of(state_in("models-unknown"), "/models").await;
        for row in body["providers"].as_array().unwrap() {
            for c in row["candidates"].as_array().unwrap() {
                assert_eq!(
                    c["verdict"], "unknown",
                    "{} was never probed and must not claim to work",
                    c["id"]
                );
                assert!(c["checked_at"].is_null());
            }
        }
    }

    #[tokio::test]
    async fn a_cached_verdict_is_reported_with_the_moment_it_was_measured() {
        let state = state_in("models-cached");
        let mut log = ProbeLog::default();
        log.record(ProbeRecord {
            provider: "codex".into(),
            model: "gpt-5.6-luna".into(),
            verdict: Verdict::Working,
            detail: None,
            checked_at: "2026-07-25T12:08:00Z".into(),
            seconds: 11.4,
            cost_usd: None,
            tokens: Some(1668),
        });
        log.save(&ProbeLog::path_in(&state.studio_dir)).unwrap();

        let (_, body) = body_of(state, "/models").await;
        let codex = body["providers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["provider"] == "codex")
            .unwrap();
        let luna = codex["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == "gpt-5.6-luna")
            .unwrap();
        assert_eq!(luna["verdict"], "working");
        assert_eq!(luna["checked_at"], "2026-07-25T12:08:00Z");
        assert_eq!(luna["tokens"], 1668);
    }

    async fn post_probe(state: AppState, body: &str) -> StatusCode {
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/models/probe")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body.to_string()))
            .unwrap();
        crate::router(state).oneshot(req).await.unwrap().status()
    }

    #[tokio::test]
    async fn probing_nothing_is_refused_so_a_panel_open_can_never_spend_the_subscription() {
        assert_eq!(
            post_probe(state_in("probe-empty"), r#"{"provider":"codex","models":[]}"#).await,
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn probing_an_unknown_provider_is_refused_by_name() {
        assert_eq!(
            post_probe(state_in("probe-nobody"), r#"{"provider":"wishful","models":["x"]}"#).await,
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn a_probe_of_a_cli_the_studio_never_read_records_unknown_not_refused() {
        let record = probe_one(Provider::Kimi, "kimi-k2", "now".into());
        assert_eq!(
            record.verdict,
            Verdict::Unknown,
            "never checked and refused are different facts and must not be conflated"
        );
        assert!(record.detail.unwrap().contains("never read"));
    }

    #[tokio::test]
    async fn a_probe_of_a_cli_that_is_not_installed_says_so_instead_of_blaming_the_model() {
        let record = probe_one(Provider::Kimi, "anything", "now".into());
        assert_eq!(record.verdict, Verdict::Unknown);
    }

    #[test]
    fn a_bounded_run_kills_a_command_that_never_finishes() {
        let started = Instant::now();
        let out = run_bounded(
            "node",
            &["-e".to_string(), "setTimeout(()=>{}, 60000)".to_string()],
            "",
            Duration::from_millis(400),
        );
        assert!(out.is_ok());
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "a probe must not hold the route open forever"
        );
    }

    #[test]
    fn a_bounded_run_returns_what_the_command_printed_on_both_pipes() {
        let script = "process.stdout.write('42'); process.stderr.write('noise');";
        let (code, stdout, stderr, _) = run_bounded(
            "node",
            &["-e".to_string(), script.to_string()],
            "",
            Duration::from_secs(20),
        )
        .unwrap();
        assert_eq!(code, Some(0));
        assert_eq!(stdout, "42");
        assert_eq!(stderr, "noise");
    }

    #[test]
    fn a_cli_that_npm_installed_as_a_shim_is_still_something_the_studio_can_run() {
        if !cfg!(windows) {
            return;
        }
        let dir = std::env::temp_dir().join("studio-shim-probe");
        std::fs::create_dir_all(&dir).unwrap();
        let shim = dir.join("studio-fake-cli.cmd");
        std::fs::write(&shim, "@echo off\r\necho catalogue-ok %1\r\n").unwrap();

        let (code, stdout, _, _) = run_bounded(
            &shim.to_string_lossy(),
            &["gpt-5.6-sol&echo pwned".to_string()],
            "",
            Duration::from_secs(20),
        )
        .expect(
            "npm installs codex, gemini and copilot as a .cmd shim and a bare name never resolves \
             to one, so every check of them failed while the panel said they were installed",
        );
        assert_eq!(code, Some(0));
        assert!(stdout.contains("catalogue-ok"), "the shim's output was lost: {stdout:?}");
        assert!(
            stdout.contains("gpt-5.6-sol&echo pwned"),
            "the model name did not reach the shim whole: {stdout:?}"
        );
        assert_eq!(
            stdout.lines().count(),
            1,
            "a second command ran, so the name was parsed as shell rather than passed as one \
             argument; names come from a config file and from the CLI's own catalogue, so they \
             are not ours to trust: {stdout:?}"
        );
    }

    #[test]
    fn a_bounded_run_hands_the_question_to_a_command_that_reads_stdin() {
        let script = "let s='';process.stdin.on('data',d=>s+=d);process.stdin.on('end',()=>process.stdout.write(s));";
        let (_, stdout, _, _) = run_bounded(
            "node",
            &["-e".to_string(), script.to_string()],
            PROBE_QUESTION,
            Duration::from_secs(20),
        )
        .unwrap();
        assert_eq!(stdout, PROBE_QUESTION);
    }
}
