use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;

use crate::AppState;

pub const MODULES: [(&str, &str); 9] = [
    ("bus.js", include_str!("../web/bus.js")),
    ("panels.js", include_str!("../web/panels.js")),
    ("settings.js", include_str!("../web/settings.js")),
    ("games.js", include_str!("../web/games.js")),
    ("gitpanel.js", include_str!("../web/gitpanel.js")),
    ("runpanel.js", include_str!("../web/runpanel.js")),
    ("chatter.js", include_str!("../web/chatter.js")),
    ("avatar.js", include_str!("../web/avatar.js")),
    ("perf.js", include_str!("../web/perf.js")),
];

pub fn lookup(name: &str) -> Option<&'static str> {
    MODULES.iter().find(|(n, _)| *n == name).map(|(_, body)| *body)
}

fn js(body: &'static str) -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        body,
    )
}

pub fn routes() -> Router<AppState> {
    let mut router = Router::new();
    for (name, body) in MODULES {
        router = router.route(&format!("/{name}"), get(move || async move { js(body) }));
    }
    router
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_module_the_floor_serves_is_reachable_by_name() {
        for (name, _) in MODULES {
            assert!(lookup(name).is_some(), "{name} is listed but not resolvable");
        }
    }

    #[test]
    fn the_floor_loads_the_shared_bus_and_the_panel_host() {
        let floor = include_str!("../web/floor.html");
        assert!(floor.contains("/bus.js"), "panels read state through the bus");
        assert!(floor.contains("/panels.js"), "nothing mounts the side panels otherwise");
    }

    #[test]
    fn every_panel_the_host_mounts_is_served() {
        let panels = include_str!("../web/panels.js");
        for name in ["settings.js", "games.js", "gitpanel.js", "runpanel.js"] {
            assert!(panels.contains(name), "the host does not mount {name}");
            assert!(lookup(name).is_some(), "{name} is mounted but never served");
        }
    }

    #[test]
    fn no_served_module_is_empty() {
        for (name, body) in MODULES {
            assert!(!body.trim().is_empty(), "{name} is served as an empty file");
        }
    }
}
