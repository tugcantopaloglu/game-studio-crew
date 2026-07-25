use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct BrowseQuery {
    #[serde(default)]
    pub path: String,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/fs/browse", get(browse))
}

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn roots() -> Vec<String> {
    #[cfg(windows)]
    {
        let mut out = Vec::new();
        for letter in b'A'..=b'Z' {
            let drive = format!("{}:\\", letter as char);
            if Path::new(&drive).is_dir() {
                out.push(drive);
            }
        }
        out
    }
    #[cfg(not(windows))]
    {
        vec!["/".to_string()]
    }
}

pub fn children(dir: &Path) -> std::io::Result<Vec<String>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name.starts_with('$') {
            continue;
        }
        out.push(name);
    }
    out.sort_by_key(|n| n.to_lowercase());
    Ok(out)
}

async fn browse(Query(q): Query<BrowseQuery>) -> Response {
    let requested = q.path.trim();
    let dir = if requested.is_empty() {
        home()
    } else {
        PathBuf::from(requested)
    };

    if !dir.is_dir() {
        return (
            StatusCode::NOT_FOUND,
            format!("{} is not a directory I can open", dir.display()),
        )
            .into_response();
    }

    let entries = match children(&dir) {
        Ok(e) => e,
        Err(e) => {
            return (
                StatusCode::FORBIDDEN,
                format!("could not read {}: {e}", dir.display()),
            )
                .into_response()
        }
    };

    axum::Json(serde_json::json!({
        "path": dir.to_string_lossy(),
        "parent": dir.parent().map(|p| p.to_string_lossy().into_owned()),
        "dirs": entries,
        "roots": roots(),
        "home": home().to_string_lossy(),
        "separator": std::path::MAIN_SEPARATOR.to_string(),
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_machine_reports_at_least_one_root_to_start_browsing_from() {
        assert!(!roots().is_empty(), "a picker with no root has nowhere to begin");
    }

    #[test]
    fn listing_a_directory_returns_only_its_subdirectories() {
        let dir = std::env::temp_dir().join("studio-fs-browse");
        let _ = std::fs::create_dir_all(dir.join("levels"));
        let _ = std::fs::write(dir.join("notes.txt"), "not a directory");

        let names = children(&dir).unwrap();
        assert!(names.iter().any(|n| n == "levels"));
        assert!(!names.iter().any(|n| n == "notes.txt"));
    }

    #[test]
    fn dotted_and_system_directories_stay_out_of_the_picker() {
        let dir = std::env::temp_dir().join("studio-fs-hidden");
        let _ = std::fs::create_dir_all(dir.join(".git"));
        let _ = std::fs::create_dir_all(dir.join("art"));

        let names = children(&dir).unwrap();
        assert_eq!(names, vec!["art".to_string()]);
    }
}
