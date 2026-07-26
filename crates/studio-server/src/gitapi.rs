use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use std::path::PathBuf;
use studio_core::git;
use studio_events::{EventType, Scene};

use crate::{project_root, AppState};

const GIT_RUN: &str = "git";
const PAGE: usize = 60;

#[derive(Debug, Deserialize)]
pub struct TreeQuery {
    pub project: String,
    #[serde(default)]
    pub skip: usize,
    #[serde(default = "page")]
    pub limit: usize,
}

fn page() -> usize {
    PAGE
}

#[derive(Debug, Deserialize)]
pub struct PushRequest {
    pub project: String,
}

#[derive(Debug, Deserialize)]
pub struct RemoteRequest {
    pub project: String,
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    pub project: String,
    pub name: String,
    #[serde(default)]
    pub private: bool,
}

#[derive(Debug, Deserialize)]
pub struct RollbackRequest {
    pub project: String,
    pub sha: String,
    #[serde(default)]
    pub confirm: bool,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/git/tree", get(tree))
        .route("/git/host", get(host))
        .route("/git/remote", post(remote))
        .route("/git/create", post(create))
        .route("/git/push", post(push))
        .route("/git/rollback", post(rollback))
}

fn repo(state: &AppState, project: &str) -> Result<PathBuf, Response> {
    if !git::available() {
        return Err((
            StatusCode::CONFLICT,
            "git is not on PATH; install it and restart the studio".to_string(),
        )
            .into_response());
    }
    let Some(root) = project_root(state, project) else {
        return Err((StatusCode::NOT_FOUND, "no such project".to_string()).into_response());
    };
    if !git::is_repo(&root) {
        return Err((
            StatusCode::CONFLICT,
            format!(
                "{} is not a git repository; create the project with git, or run git init there",
                root.display()
            ),
        )
            .into_response());
    }
    Ok(root)
}

fn announce(state: &AppState, project: &str, action: &str, ok: bool, detail: &str) {
    let data = serde_json::json!({
        "project": project,
        "action": action,
        "ok": ok,
        "detail": detail,
    });
    if let Ok(env) = state.store.append_event(
        GIT_RUN,
        crate::now_rfc3339(),
        "daemon",
        EventType::GitAction,
        Scene::daemon(),
        data,
    ) {
        state.publish(env);
    }
}

async fn tree(State(state): State<AppState>, Query(q): Query<TreeQuery>) -> Response {
    let root = match repo(&state, &q.project) {
        Ok(root) => root,
        Err(response) => return response,
    };

    let (skip, limit) = (q.skip, q.limit);
    let read = crate::off_the_runtime(move || {
        let history = git::history(&root, skip, limit);
        let dirty = git::changes(&root).unwrap_or_default();
        let remotes = git::remotes(&root).unwrap_or_default();
        (history, dirty, remotes, git::branch(&root), git::head_sha(&root))
    })
    .await;
    let (history, dirty, remotes, branch, head) = match read {
        Ok(got) => got,
        Err(response) => return response,
    };
    let history = match history {
        Ok(h) => h,
        Err(e) => return (StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
    };

    axum::Json(serde_json::json!({
        "project": q.project,
        "branch": branch,
        "head": head,
        "skip": q.skip,
        "lanes": history.lanes,
        "more": history.more,
        "rows": history.rows,
        "dirty": dirty,
        "remotes": remotes,
    }))
    .into_response()
}

async fn host() -> Response {
    axum::Json(git::host()).into_response()
}

async fn remote(
    State(state): State<AppState>,
    axum::Json(req): axum::Json<RemoteRequest>,
) -> Response {
    let root = match repo(&state, &req.project) {
        Ok(root) => root,
        Err(response) => return response,
    };
    let url = req.url.clone();
    let set = match crate::off_the_runtime(move || git::set_remote(&root, &url)).await {
        Ok(done) => done,
        Err(response) => return response,
    };
    match set {
        Ok(()) => {
            let detail = format!("origin is {}", req.url.trim());
            announce(&state, &req.project, "remote", true, &detail);
            (StatusCode::OK, detail).into_response()
        }
        Err(e) => {
            announce(&state, &req.project, "remote", false, &e.to_string());
            (StatusCode::BAD_REQUEST, e.to_string()).into_response()
        }
    }
}

async fn create(
    State(state): State<AppState>,
    axum::Json(req): axum::Json<CreateRequest>,
) -> Response {
    let root = match repo(&state, &req.project) {
        Ok(root) => root,
        Err(response) => return response,
    };
    let (name, private) = (req.name.clone(), req.private);
    let made = match crate::off_the_runtime(move || git::create_remote(&root, &name, private)).await
    {
        Ok(done) => done,
        Err(response) => return response,
    };
    match made {
        Ok(url) => {
            let detail = format!("created {url}");
            announce(&state, &req.project, "create", true, &detail);
            (StatusCode::CREATED, detail).into_response()
        }
        Err(e) => {
            announce(&state, &req.project, "create", false, &e.to_string());
            (StatusCode::CONFLICT, e.to_string()).into_response()
        }
    }
}

async fn push(State(state): State<AppState>, axum::Json(req): axum::Json<PushRequest>) -> Response {
    let root = match repo(&state, &req.project) {
        Ok(root) => root,
        Err(response) => return response,
    };
    let pushed = match crate::off_the_runtime(move || git::push(&root)).await {
        Ok(done) => done,
        Err(response) => return response,
    };
    match pushed {
        Ok(said) => {
            announce(&state, &req.project, "push", true, &said);
            (StatusCode::OK, said).into_response()
        }
        Err(e) => {
            announce(&state, &req.project, "push", false, &e.to_string());
            (StatusCode::CONFLICT, e.to_string()).into_response()
        }
    }
}

async fn rollback(
    State(state): State<AppState>,
    axum::Json(req): axum::Json<RollbackRequest>,
) -> Response {
    let root = match repo(&state, &req.project) {
        Ok(root) => root,
        Err(response) => return response,
    };

    let sha = req.sha.clone();
    let confirm = req.confirm;
    let did = crate::off_the_runtime(move || {
        if confirm {
            git::rollback(&root, &sha).map(Ok)
        } else {
            git::rollback_plan(&root, &sha).map(Err)
        }
    })
    .await;
    let did = match did {
        Ok(done) => done,
        Err(response) => return response,
    };

    let did = match did {
        Ok(Err(plan)) => {
            return axum::Json(serde_json::json!({"applied": false, "plan": plan}))
                .into_response()
        }
        Ok(Ok(done)) => Ok(done),
        Err(e) => Err(e),
    };

    match did {
        Ok(done) => {
            let detail = format!(
                "rolled back to {} '{}', discarding {} commit(s) and {} uncommitted change(s)",
                done.sha,
                done.subject,
                done.discards.len(),
                done.dirty.len()
            );
            announce(&state, &req.project, "rollback", true, &detail);
            axum::Json(serde_json::json!({"applied": true, "plan": done, "detail": detail}))
                .into_response()
        }
        Err(e) => {
            announce(&state, &req.project, "rollback", false, &e.to_string());
            (StatusCode::BAD_REQUEST, e.to_string()).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use std::sync::Arc;
    use studio_store::{ProjectRow, Store};
    use tower::ServiceExt;

    fn scratch(tag: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("studio-gitapi-{tag}-{stamp}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn floor(project: &std::path::Path) -> (Router, Arc<Store>) {
        let home = scratch("store");
        let store = Arc::new(Store::open(home.join("studio.db")).unwrap());
        store
            .insert_project(
                ProjectRow {
                    id: "proj_test".into(),
                    name: "test".into(),
                    root: project.to_string_lossy().into_owned(),
                    engine: "godot".into(),
                    git: true,
                },
                crate::now_rfc3339(),
            )
            .unwrap();
        let state = AppState::new(store.clone());
        (crate::router(state), store)
    }

    async fn call(app: &Router, method: &str, uri: &str, body: &str) -> (StatusCode, String) {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    #[tokio::test]
    async fn a_push_with_no_remote_tells_the_floor_what_to_do_instead() {
        if !git::available() {
            return;
        }
        let project = scratch("noremote");
        git::init(&project).unwrap();
        let (app, store) = floor(&project);

        let (status, body) = call(&app, "POST", "/git/push", r#"{"project":"proj_test"}"#).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(body.contains("no remote"), "{body}");
        assert!(body.contains("gh"), "{body}");
        assert!(!body.contains("fatal:"), "raw git output is not an instruction: {body}");

        let recorded = store.events_since(GIT_RUN, 0).unwrap();
        assert_eq!(recorded.len(), 1, "a failed push is still floor news");
        assert_eq!(recorded[0].event_type, EventType::GitAction);
        assert_eq!(recorded[0].data["ok"], serde_json::json!(false));
        assert_eq!(recorded[0].data["action"], serde_json::json!("push"));

        let _ = std::fs::remove_dir_all(&project);
    }

    #[tokio::test]
    async fn the_tree_hands_the_panel_rows_it_can_draw() {
        if !git::available() {
            return;
        }
        let project = scratch("tree");
        git::init(&project).unwrap();
        std::fs::write(project.join("dash.gd"), "extends Node\n").unwrap();
        git::commit(&project, "gameplay_engineer: add a dash").unwrap();
        let (app, _store) = floor(&project);

        let (status, body) = call(&app, "GET", "/git/tree?project=proj_test&limit=10", "").await;
        assert_eq!(status, StatusCode::OK);

        let tree: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(tree["branch"], serde_json::json!("main"));
        assert_eq!(tree["more"], serde_json::json!(false));
        assert_eq!(tree["rows"].as_array().unwrap().len(), 2);

        let head = &tree["rows"][0];
        assert_eq!(head["subject"], serde_json::json!("gameplay_engineer: add a dash"));
        assert_eq!(head["lane"], serde_json::json!(0));
        assert!(head["short"].as_str().is_some_and(|s| !s.is_empty()));
        assert!(head["at"].as_i64().unwrap() > 0);

        let _ = std::fs::remove_dir_all(&project);
    }

    #[tokio::test]
    async fn a_rollback_shows_what_it_would_destroy_before_it_is_confirmed() {
        if !git::available() {
            return;
        }
        let project = scratch("rollback");
        git::init(&project).unwrap();
        let fork = git::head_sha(&project).unwrap();
        std::fs::write(project.join("dash.gd"), "extends Node\n").unwrap();
        git::commit(&project, "gameplay_engineer: add a dash").unwrap();
        std::fs::write(project.join("loose.txt"), "never committed\n").unwrap();

        let (app, store) = floor(&project);
        let asking = format!(r#"{{"project":"proj_test","sha":"{fork}"}}"#);
        let (status, body) = call(&app, "POST", "/git/rollback", &asking).await;
        assert_eq!(status, StatusCode::OK);

        let preview: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(preview["applied"], serde_json::json!(false));
        assert_eq!(preview["plan"]["discards"].as_array().unwrap().len(), 1);
        assert!(body.contains("loose.txt"), "the dirty file must be named first: {body}");
        assert!(project.join("dash.gd").exists(), "a preview must not touch the tree");
        assert!(
            store.events_since(GIT_RUN, 0).unwrap().is_empty(),
            "a preview is not a git action"
        );

        let doing = format!(r#"{{"project":"proj_test","sha":"{fork}","confirm":true}}"#);
        let (status, body) = call(&app, "POST", "/git/rollback", &doing).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("\"applied\":true"), "{body}");
        assert!(!project.join("dash.gd").exists());
        assert!(!project.join("loose.txt").exists());
        assert_eq!(git::head_sha(&project).unwrap(), fork);

        let recorded = store.events_since(GIT_RUN, 0).unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].data["ok"], serde_json::json!(true));
        assert!(recorded[0].data["detail"].as_str().unwrap().contains("discarding 1 commit"));

        let _ = std::fs::remove_dir_all(&project);
    }

    #[tokio::test]
    async fn a_rollback_to_something_that_is_not_a_commit_is_refused() {
        if !git::available() {
            return;
        }
        let project = scratch("badsha");
        git::init(&project).unwrap();
        let head = git::head_sha(&project).unwrap();
        let (app, _store) = floor(&project);

        for sha in ["main", "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"] {
            let body = format!(r#"{{"project":"proj_test","sha":"{sha}","confirm":true}}"#);
            let (status, said) = call(&app, "POST", "/git/rollback", &body).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{said}");
            assert!(said.contains("refusing to roll back"), "{said}");
        }
        assert_eq!(git::head_sha(&project).unwrap(), head);

        let _ = std::fs::remove_dir_all(&project);
    }

    #[tokio::test]
    async fn a_project_without_a_repository_is_told_so_rather_than_shown_an_empty_tree() {
        if !git::available() {
            return;
        }
        let project = scratch("bare");
        let (app, _store) = floor(&project);

        let (status, body) = call(&app, "GET", "/git/tree?project=proj_test", "").await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(body.contains("not a git repository"), "{body}");

        let (status, _) = call(&app, "GET", "/git/tree?project=proj_missing", "").await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let _ = std::fs::remove_dir_all(&project);
    }
}
