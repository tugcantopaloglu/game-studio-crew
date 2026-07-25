use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::{now_rfc3339, project_root, AppState, StudioCommand, SummarizeRequest};

const SKIP_DIRS: [&str; 8] = [
    ".git", ".claude", ".godot", ".studio-out", "node_modules", "target", "__pycache__", ".import",
];
const HASHED_BYTES: u64 = 128 * 1024;
const MAX_FINGERPRINTED_FILES: usize = 6000;
const MAX_DEPTH: usize = 8;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mechanic {
    pub name: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub text: String,
    #[serde(default)]
    pub mechanics: Vec<Mechanic>,
    pub fingerprint: String,
    pub generated: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Card {
    #[serde(default)]
    pub origin: Option<String>,
    #[serde(default)]
    pub adopted: Option<String>,
    #[serde(default)]
    pub summary: Option<Summary>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SummaryState {
    Missing,
    Stale(Summary),
    Fresh(Summary),
}

pub fn card_path(root: &Path) -> PathBuf {
    root.join(".studio").join("game.json")
}

pub fn read_card(root: &Path) -> Card {
    std::fs::read_to_string(card_path(root))
        .ok()
        .and_then(|body| serde_json::from_str(&body).ok())
        .unwrap_or_default()
}

pub fn write_card(root: &Path, card: &Card) -> std::io::Result<()> {
    let path = card_path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_string_pretty(card)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, body)
}

fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<String>, depth: usize) {
    if depth > MAX_DEPTH || out.len() > MAX_FINGERPRINTED_FILES {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) && !name.starts_with('.') {
                collect(root, &path, out, depth + 1);
            }
            continue;
        }
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let body = if size <= HASHED_BYTES {
            std::fs::read(&path).map(|b| fnv(&b)).unwrap_or(0)
        } else {
            0
        };
        out.push(format!("{rel}|{size}|{body:016x}"));
    }
}

pub fn fingerprint(root: &Path) -> String {
    let mut lines = Vec::new();
    collect(root, root, &mut lines, 0);
    lines.sort();
    format!("fnv:{:016x}", fnv(lines.join("\n").as_bytes()))
}

pub fn summary_state(root: &Path) -> SummaryState {
    match read_card(root).summary {
        None => SummaryState::Missing,
        Some(s) if s.fingerprint == fingerprint(root) => SummaryState::Fresh(s),
        Some(s) => SummaryState::Stale(s),
    }
}

pub fn record_summary(
    root: &Path,
    text: &str,
    mechanics: Vec<Mechanic>,
) -> std::io::Result<Summary> {
    let summary = Summary {
        text: text.trim().to_string(),
        mechanics,
        fingerprint: fingerprint(root),
        generated: now_rfc3339(),
    };
    let mut card = read_card(root);
    card.summary = Some(summary.clone());
    write_card(root, &card)?;
    Ok(summary)
}

pub fn record_adoption(root: &Path) -> std::io::Result<()> {
    let mut card = read_card(root);
    card.origin = Some("adopted".to_string());
    card.adopted = Some(now_rfc3339());
    let written = write_card(root, &card);
    forget(root);
    written
}

fn git_out(root: &Path, args: &[&str]) -> Option<String> {
    let out = studio_core::command("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn commit_count(root: &Path) -> u64 {
    git_out(root, &["rev-list", "--count", "HEAD"])
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

fn last_commit_ts(root: &Path) -> Option<String> {
    git_out(root, &["log", "-1", "--format=%cI"])
}

fn folder_touched_ts(root: &Path) -> Option<String> {
    let modified = std::fs::metadata(root).ok()?.modified().ok()?;
    time::OffsetDateTime::from(modified)
        .format(&time::format_description::well_known::Rfc3339)
        .ok()
}

fn studio_authored(subject: &str) -> bool {
    match subject.split_once(": ") {
        Some((prefix, _)) => prefix == "crew" || studio_agents::role(prefix).is_some(),
        None => false,
    }
}

pub fn marked_origin(card: &Card) -> Option<&'static str> {
    card.origin
        .as_deref()
        .map(|recorded| if recorded == "adopted" { "adopted" } else { "built" })
}

pub fn origin_of(root: &Path, card: &Card) -> &'static str {
    if let Some(marked) = marked_origin(card) {
        return marked;
    }
    if !studio_core::git::is_repo(root) {
        return "unknown";
    }
    match git_out(root, &["log", "--max-parents=0", "--format=%s", "HEAD"]) {
        Some(subjects) => match subjects.lines().next() {
            Some(first) if studio_authored(first) => "built",
            Some(_) => "adopted",
            None => "unknown",
        },
        None => "unknown",
    }
}

pub fn head_pointer(root: &Path) -> Option<String> {
    let git = root.join(".git");
    let head = std::fs::read_to_string(git.join("HEAD")).ok()?;
    let head = head.trim().to_string();
    let Some(reference) = head.strip_prefix("ref: ") else {
        return Some(format!("detached {head}"));
    };
    if let Ok(sha) = std::fs::read_to_string(git.join(reference)) {
        return Some(format!("{reference} {}", sha.trim()));
    }
    let packed = std::fs::read_to_string(git.join("packed-refs")).ok()?;
    for line in packed.lines() {
        if let Some((sha, name)) = line.split_once(' ') {
            if name.trim() == reference {
                return Some(format!("{reference} {sha}"));
            }
        }
    }
    Some(format!("{reference} unborn"))
}

#[derive(Debug, Clone, PartialEq)]
pub struct History {
    pub commits: u64,
    pub last_worked: Option<String>,
    pub origin: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Detail {
    pub history: History,
    pub fresh: Option<bool>,
}

type HistoryCache = std::sync::Mutex<std::collections::HashMap<PathBuf, (String, History)>>;

fn cache() -> &'static HistoryCache {
    static CACHE: std::sync::OnceLock<HistoryCache> = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn forget(root: &Path) {
    if let Ok(mut seen) = cache().lock() {
        seen.remove(root);
    }
}

fn read_history(root: &Path, card: &Card) -> History {
    let repo = studio_core::git::is_repo(root);
    History {
        commits: if repo { commit_count(root) } else { 0 },
        last_worked: if repo {
            last_commit_ts(root).or_else(|| folder_touched_ts(root))
        } else {
            folder_touched_ts(root)
        },
        origin: origin_of(root, card),
    }
}

pub fn history_of(root: &Path, card: &Card) -> History {
    let Some(key) = head_pointer(root) else {
        return read_history(root, card);
    };

    if let Ok(seen) = cache().lock() {
        if let Some((cached, history)) = seen.get(root) {
            if *cached == key {
                return history.clone();
            }
        }
    }

    let history = read_history(root, card);
    if let Ok(mut seen) = cache().lock() {
        seen.insert(root.to_path_buf(), (key, history.clone()));
    }
    history
}

pub fn detail_of(root: &Path, card: &Card) -> Detail {
    if !root.is_dir() {
        return Detail {
            history: History { commits: 0, last_worked: None, origin: "unknown" },
            fresh: None,
        };
    }
    Detail {
        history: history_of(root, card),
        fresh: card.summary.as_ref().map(|s| s.fingerprint == fingerprint(root)),
    }
}

pub fn nothing_detected(root: &Path) -> String {
    let profiles = studio_engine::EngineProfile::builtin();
    let looked: Vec<String> = profiles
        .iter()
        .map(|p| format!("{} for {}", p.detect.markers.join(" and "), p.id))
        .collect();
    format!(
        "no engine recognised at {}; I looked for {}. Pick the engine yourself if you know which it is.",
        root.display(),
        looked.join(", ")
    )
}

fn engine_ids() -> Vec<String> {
    studio_engine::EngineProfile::builtin()
        .into_iter()
        .map(|p| p.id)
        .collect()
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/games", get(list))
        .route("/games/detail", get(detail))
        .route("/games/engines", get(engines))
        .route("/games/adopt", post(adopt))
        .route("/games/summarize", post(summarize))
}

fn game_json(p: &studio_store::ProjectRow) -> serde_json::Value {
    let root = PathBuf::from(&p.root);
    let exists = root.is_dir();
    let card = if exists { read_card(&root) } else { Card::default() };

    let summary = card.summary.as_ref().map(|s| {
        serde_json::json!({
            "text": s.text,
            "mechanics": s.mechanics,
            "generated": s.generated,
        })
    });

    serde_json::json!({
        "id": p.id,
        "name": p.name,
        "root": p.root,
        "engine": p.engine,
        "git": exists && studio_core::git::is_repo(&root),
        "exists": exists,
        "origin": if exists { marked_origin(&card).unwrap_or("unknown") } else { "unknown" },
        "adopted": card.adopted,
        "summary": summary,
    })
}

fn detail_json(p: &studio_store::ProjectRow) -> serde_json::Value {
    let root = PathBuf::from(&p.root);
    let card = read_card(&root);
    let detail = detail_of(&root, &card);
    serde_json::json!({
        "id": p.id,
        "commits": detail.history.commits,
        "last_worked": detail.history.last_worked,
        "origin": detail.history.origin,
        "fresh": detail.fresh,
    })
}

async fn list(State(state): State<AppState>) -> Response {
    let rows = match state.store.projects() {
        Ok(rows) => rows,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("could not read the project list: {e}"),
            )
                .into_response()
        }
    };
    let games: Vec<serde_json::Value> = rows.iter().map(game_json).collect();
    axum::Json(games).into_response()
}

async fn detail(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let rows = match state.store.projects() {
        Ok(rows) => rows,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("could not read the project list: {e}"),
            )
                .into_response()
        }
    };
    let wanted = q.get("project");
    let details: Vec<serde_json::Value> = rows
        .iter()
        .filter(|p| wanted.is_none_or(|id| *id == p.id))
        .map(detail_json)
        .collect();
    axum::Json(details).into_response()
}

async fn engines() -> Response {
    let rows: Vec<serde_json::Value> = studio_engine::EngineProfile::builtin()
        .into_iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id,
                "display_name": p.display_name,
                "markers": p.detect.markers,
            })
        })
        .collect();
    axum::Json(rows).into_response()
}

#[derive(Debug, Clone, Deserialize)]
pub struct AdoptRequest {
    pub name: String,
    pub root: String,
    #[serde(default)]
    pub engine: Option<String>,
    #[serde(default)]
    pub git: bool,
}

async fn adopt(State(state): State<AppState>, axum::Json(req): axum::Json<AdoptRequest>) -> Response {
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return (StatusCode::BAD_REQUEST, "a game needs a name".to_string()).into_response();
    }

    let raw = req.root.trim();
    let root = PathBuf::from(raw);
    if raw.is_empty() || !root.is_absolute() {
        return (
            StatusCode::BAD_REQUEST,
            format!("give an absolute path to the game folder; {raw} is not one"),
        )
            .into_response();
    }
    if !root.is_dir() {
        return (
            StatusCode::BAD_REQUEST,
            format!(
                "{} is not a folder that exists; adopting points at a game you already have, \
                 so use new project to start an empty one",
                root.display()
            ),
        )
            .into_response();
    }

    let canonical = root.canonicalize().map(crate::strip_verbatim).unwrap_or(root);
    let asked = req.engine.unwrap_or_else(|| "auto".to_string());
    let engine = if asked == "auto" {
        let profiles = studio_engine::EngineProfile::builtin();
        match studio_engine::detect(&canonical, &profiles).first() {
            Some(d) => d.id.clone(),
            None => return (StatusCode::BAD_REQUEST, nothing_detected(&canonical)).into_response(),
        }
    } else if engine_ids().contains(&asked) {
        asked
    } else {
        return (
            StatusCode::BAD_REQUEST,
            format!("{asked} is not an engine I know; pick one of {}", engine_ids().join(", ")),
        )
            .into_response();
    };

    let already_a_repo = studio_core::git::is_repo(&canonical);
    let git_ready = if already_a_repo {
        true
    } else if req.git {
        if !studio_core::git::available() {
            return (
                StatusCode::BAD_REQUEST,
                "git is not on PATH; install it or adopt this game without git".to_string(),
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
        id: format!(
            "proj_{}",
            name.to_lowercase().replace(|c: char| !c.is_alphanumeric(), "-")
        ),
        name,
        root: canonical.to_string_lossy().into_owned(),
        engine,
        git: git_ready,
    };

    if let Err(e) = state.store.insert_project(row.clone(), now_rfc3339()) {
        return (
            StatusCode::CONFLICT,
            format!("could not record the game (is it already adopted under this name?): {e}"),
        )
            .into_response();
    }

    let note = record_adoption(&canonical)
        .err()
        .map(|e| format!("adopted, but I could not write {}: {e}", card_path(&canonical).display()));

    (
        StatusCode::CREATED,
        axum::Json(serde_json::json!({
            "id": row.id,
            "name": row.name,
            "root": row.root,
            "engine": row.engine,
            "git": row.git,
            "git_initialised": git_ready && !already_a_repo,
            "note": note,
        })),
    )
        .into_response()
}

async fn summarize(
    State(state): State<AppState>,
    axum::Json(req): axum::Json<SummarizeRequest>,
) -> Response {
    let Some(root) = project_root(&state, &req.project) else {
        return (StatusCode::NOT_FOUND, "no such game".to_string()).into_response();
    };
    if !root.is_dir() {
        return (
            StatusCode::CONFLICT,
            format!("{} is gone; there is nothing left to read", root.display()),
        )
            .into_response();
    }

    if let SummaryState::Fresh(summary) = summary_state(&root) {
        return (
            StatusCode::OK,
            axum::Json(serde_json::json!({"cached": true, "summary": summary})),
        )
            .into_response();
    }

    match state.dispatch(StudioCommand::Summarize(req)) {
        Ok(()) => (StatusCode::ACCEPTED, "reading the game".to_string()).into_response(),
        Err(e) => (StatusCode::SERVICE_UNAVAILABLE, e).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router;
    use std::sync::Arc;
    use studio_store::Store;
    use tower::ServiceExt;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "studio-games-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn listing(root: &Path) -> Vec<String> {
        let mut out = Vec::new();
        collect(root, root, &mut out, 0);
        out.sort();
        out
    }

    struct Floor {
        app: Router,
        commands: std::sync::mpsc::Receiver<StudioCommand>,
        store: Arc<Store>,
        _dir: PathBuf,
    }

    fn floor(tag: &str) -> Floor {
        let dir = scratch(tag);
        let store = Arc::new(Store::open(dir.join("state.db")).unwrap());
        let (tx, commands) = std::sync::mpsc::channel();
        let app = router(AppState::new(store.clone()).with_commands(tx));
        Floor { app, commands, store, _dir: dir }
    }

    async fn post_json(app: &Router, path: &str, body: serde_json::Value) -> (StatusCode, String) {
        let req = axum::http::Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body.to_string()))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    #[tokio::test]
    async fn adopting_a_directory_that_already_holds_a_game_overwrites_nothing() {
        let f = floor("adopt-occupied");
        let game = scratch("occupied-game");
        std::fs::write(game.join("game.html"), "<h1>mine</h1>").unwrap();
        std::fs::write(game.join(".gitignore"), "my-own-ignores\n").unwrap();
        std::fs::create_dir_all(game.join("src")).unwrap();
        std::fs::write(game.join("src/main.js"), "console.log('mine')").unwrap();

        let before = listing(&game);
        let (status, body) = post_json(
            &f.app,
            "/games/adopt",
            serde_json::json!({
                "name": "Occupied",
                "root": game.to_string_lossy(),
                "engine": "web",
            }),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED, "{body}");
        assert_eq!(
            listing(&game),
            before,
            "adoption wrote into a folder that already had a game in it"
        );
        assert!(
            !game.join("index.html").exists(),
            "adoption scaffolded a fresh game over the real one"
        );
        assert_eq!(
            std::fs::read_to_string(game.join(".gitignore")).unwrap(),
            "my-own-ignores\n"
        );
        assert_eq!(f.store.projects().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_folder_with_no_recognisable_engine_reports_what_was_tried() {
        let f = floor("adopt-unknown");
        let unknown = scratch("unknown-game");
        std::fs::write(unknown.join("notes.txt"), "someday").unwrap();

        let (status, body) = post_json(
            &f.app,
            "/games/adopt",
            serde_json::json!({"name": "Mystery", "root": unknown.to_string_lossy()}),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        for marker in [
            "project.godot",
            "ProjectSettings/ProjectVersion.txt",
            "*.uproject",
            "index.html",
            "main.py",
        ] {
            assert!(body.contains(marker), "the refusal never mentions {marker}: {body}");
        }
        assert!(body.contains("Pick the engine yourself"), "{body}");
        assert!(f.store.projects().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_game_detection_cannot_place_is_still_adoptable_by_hand() {
        let f = floor("adopt-by-hand");
        let game = scratch("by-hand-game");
        std::fs::write(game.join("game.html"), "<h1>mine</h1>").unwrap();

        let (status, body) = post_json(
            &f.app,
            "/games/adopt",
            serde_json::json!({
                "name": "By Hand",
                "root": game.to_string_lossy(),
                "engine": "web",
            }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        assert_eq!(f.store.projects().unwrap()[0].engine, "web");
    }

    #[tokio::test]
    async fn a_cached_summary_is_served_instead_of_billing_a_worker() {
        let f = floor("summary-cache");
        let game = scratch("summary-game");
        std::fs::write(game.join("main.py"), "print('hello')").unwrap();

        let (status, body) = post_json(
            &f.app,
            "/games/adopt",
            serde_json::json!({"name": "Cached", "root": game.to_string_lossy()}),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let id = f.store.projects().unwrap()[0].id.clone();

        let (status, _) =
            post_json(&f.app, "/games/summarize", serde_json::json!({"project": id})).await;
        assert_eq!(status, StatusCode::ACCEPTED, "with no cache the daemon must be asked");
        assert!(f.commands.try_recv().is_ok(), "nothing reached the studio runner");

        record_summary(
            &game,
            "A one screen toy that prints a greeting.",
            vec![Mechanic { name: "greeting".into(), note: "prints once at start".into() }],
        )
        .unwrap();

        let (status, body) =
            post_json(&f.app, "/games/summarize", serde_json::json!({"project": id})).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(body.contains("prints a greeting"), "{body}");
        assert!(f.commands.try_recv().is_err(), "a cached summary still spent a worker");
    }

    #[test]
    fn a_summary_goes_stale_when_the_game_changes_but_not_when_it_is_only_cached() {
        let game = scratch("stale");
        std::fs::write(game.join("main.py"), "print('one')").unwrap();
        record_summary(&game, "prints one", Vec::new()).unwrap();

        assert!(
            matches!(summary_state(&game), SummaryState::Fresh(_)),
            "writing the cache into .studio must not invalidate the cache"
        );

        std::fs::write(game.join("main.py"), "print('two')").unwrap();
        assert!(
            matches!(summary_state(&game), SummaryState::Stale(_)),
            "an edited game kept claiming its old summary was current"
        );
    }

    #[test]
    fn an_edit_that_keeps_the_file_length_still_goes_stale() {
        let game = scratch("same-length");
        std::fs::write(game.join("player.gd"), "var speed = 100").unwrap();
        record_summary(&game, "a runner", Vec::new()).unwrap();
        std::fs::write(game.join("player.gd"), "var speed = 900").unwrap();
        assert!(matches!(summary_state(&game), SummaryState::Stale(_)));
    }

    #[test]
    fn a_game_with_no_summary_yet_reports_missing_rather_than_guessing() {
        let game = scratch("no-summary");
        std::fs::write(game.join("main.py"), "print('hi')").unwrap();
        assert_eq!(summary_state(&game), SummaryState::Missing);
    }

    #[test]
    fn a_marked_game_reads_as_adopted_and_unmarked_history_is_read_not_guessed() {
        let adopted = scratch("origin-adopted");
        record_adoption(&adopted).unwrap();
        assert_eq!(origin_of(&adopted, &read_card(&adopted)), "adopted");

        let plain = scratch("origin-plain");
        assert_eq!(
            origin_of(&plain, &read_card(&plain)),
            "unknown",
            "a folder with no history and no marker must not be guessed at"
        );

        assert!(studio_authored("gameplay_engineer: add a dash"));
        assert!(studio_authored("crew: artist + qa_engineer finish parallel work"));
        assert!(!studio_authored("Initial commit"));
        assert!(!studio_authored("chore: bump deps"));
    }

    async fn get(app: &Router, path: &str) -> (StatusCode, String) {
        let req = axum::http::Request::builder()
            .method("GET")
            .uri(path)
            .body(axum::body::Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    #[tokio::test]
    async fn the_list_carries_nothing_that_costs_a_git_call_and_the_detail_carries_all_of_it() {
        let f = floor("split");
        let game = scratch("split-game");
        std::fs::write(game.join("main.py"), "print('hi')").unwrap();
        post_json(
            &f.app,
            "/games/adopt",
            serde_json::json!({"name": "Split", "root": game.to_string_lossy()}),
        )
        .await;
        record_summary(&game, "a toy", Vec::new()).unwrap();

        let (status, list) = get(&f.app, "/games").await;
        assert_eq!(status, StatusCode::OK);
        for costly in ["commits", "last_worked", "fresh"] {
            assert!(
                !list.contains(costly),
                "the list opens the panel, so it must not carry {costly}: {list}"
            );
        }
        assert!(list.contains("Split") && list.contains("a toy"));

        let (status, detail) = get(&f.app, "/games/detail").await;
        assert_eq!(status, StatusCode::OK);
        for costly in ["commits", "last_worked", "fresh", "origin"] {
            assert!(detail.contains(costly), "the detail is missing {costly}: {detail}");
        }
    }

    #[tokio::test]
    async fn detail_can_be_asked_for_one_game_rather_than_the_whole_library() {
        let f = floor("detail-one");
        for name in ["One", "Two"] {
            let game = scratch(&format!("detail-{name}"));
            std::fs::write(game.join("main.py"), "print('hi')").unwrap();
            post_json(
                &f.app,
                "/games/adopt",
                serde_json::json!({"name": name, "root": game.to_string_lossy()}),
            )
            .await;
        }
        let (_, all) = get(&f.app, "/games/detail").await;
        assert_eq!(all.matches("\"id\"").count(), 2);

        let (_, one) = get(&f.app, "/games/detail?project=proj_one").await;
        assert_eq!(one.matches("\"id\"").count(), 1);
        assert!(one.contains("proj_one"));
    }

    #[test]
    fn the_head_pointer_moves_when_the_branch_moves_and_holds_still_otherwise() {
        let game = scratch("head-pointer");
        let refs = game.join(".git").join("refs").join("heads");
        std::fs::create_dir_all(&refs).unwrap();
        std::fs::write(game.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(refs.join("main"), "1111111111111111111111111111111111111111\n").unwrap();

        let first = head_pointer(&game).unwrap();
        assert_eq!(head_pointer(&game).unwrap(), first, "a repo nobody touched must key the same");

        std::fs::write(refs.join("main"), "2222222222222222222222222222222222222222\n").unwrap();
        assert_ne!(head_pointer(&game).unwrap(), first, "a new commit must invalidate the cache");
    }

    #[test]
    fn a_packed_ref_is_still_a_head_pointer_rather_than_nothing() {
        let game = scratch("packed-ref");
        std::fs::create_dir_all(game.join(".git")).unwrap();
        std::fs::write(game.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(
            game.join(".git/packed-refs"),
            "# pack-refs with: peeled fully-peeled sorted\n\
             3333333333333333333333333333333333333333 refs/heads/main\n",
        )
        .unwrap();

        let pointer = head_pointer(&game).unwrap();
        assert!(pointer.contains("3333333"), "{pointer}");
    }

    #[test]
    fn a_folder_that_is_not_a_repo_at_all_has_no_pointer_to_cache_on() {
        let game = scratch("no-repo");
        std::fs::write(game.join("main.py"), "print('hi')").unwrap();
        assert!(head_pointer(&game).is_none());
    }

    #[test]
    fn a_new_commit_is_counted_rather_than_served_from_the_cache() {
        if !studio_core::git::available() {
            return;
        }
        let game = scratch("cache-invalidation");
        std::fs::write(game.join("main.py"), "print('one')").unwrap();
        studio_core::git::init(&game).unwrap();

        let card = read_card(&game);
        let before = history_of(&game, &card).commits;
        assert!(before >= 1, "git init commits the opening state");

        std::fs::write(game.join("main.py"), "print('two')").unwrap();
        studio_core::git::commit(&game, "gameplay_engineer: change the greeting").unwrap();

        assert_eq!(
            history_of(&game, &card).commits,
            before + 1,
            "the cached history outlived the commit that changed it"
        );
    }

    #[test]
    fn adopting_a_game_clears_the_origin_the_cache_already_answered() {
        let game = scratch("origin-invalidation");
        std::fs::write(game.join("main.py"), "print('hi')").unwrap();
        assert_eq!(history_of(&game, &read_card(&game)).origin, "unknown");

        record_adoption(&game).unwrap();
        assert_eq!(history_of(&game, &read_card(&game)).origin, "adopted");
    }

    #[test]
    fn every_builtin_engine_is_offered_by_name_when_detection_fails() {
        let message = nothing_detected(Path::new("C:/games/mystery"));
        for id in engine_ids() {
            assert!(message.contains(&id), "{id} is missing from {message}");
        }
    }
}
