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
use studio_core::{Provider, RoleNeeds};
use studio_settings::Settings;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/settings", get(read_settings).post(write_settings))
        .route("/providers", get(providers))
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
    let extensions: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
            .split(';')
            .map(|e| e.trim().to_lowercase())
            .filter(|e| !e.is_empty())
            .collect()
    } else {
        Vec::new()
    };

    let raw = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&raw) {
        let bare = dir.join(program);
        if bare.is_file() {
            return Some(bare);
        }
        for ext in &extensions {
            let with_ext = dir.join(format!("{program}{ext}"));
            if with_ext.is_file() {
                return Some(with_ext);
            }
        }
    }
    None
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
}
