use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;

use crate::AppState;

pub const MODULES: [(&str, &str); 10] = [
    ("bus.js", include_str!("../web/bus.js")),
    ("panels.js", include_str!("../web/panels.js")),
    ("settings.js", include_str!("../web/settings.js")),
    ("games.js", include_str!("../web/games.js")),
    ("gitpanel.js", include_str!("../web/gitpanel.js")),
    ("runpanel.js", include_str!("../web/runpanel.js")),
    ("assets.js", include_str!("../web/assets.js")),
    ("chatter.js", include_str!("../web/chatter.js")),
    ("avatar.js", include_str!("../web/avatar.js")),
    ("perf.js", include_str!("../web/perf.js")),
];

pub const IMAGES: [(&str, &[u8]); 2] = [
    ("mark.png", include_bytes!("../web/mark.png")),
    ("favicon.png", include_bytes!("../web/favicon.png")),
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

fn png(body: &'static [u8]) -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        body,
    )
}

pub fn routes() -> Router<AppState> {
    let mut router = Router::new();
    for (name, body) in MODULES {
        router = router.route(&format!("/{name}"), get(move || async move { js(body) }));
    }
    for (name, body) in IMAGES {
        router = router.route(&format!("/{name}"), get(move || async move { png(body) }));
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
        for name in ["settings.js", "games.js", "gitpanel.js", "runpanel.js", "assets.js"] {
            assert!(panels.contains(name), "the host does not mount {name}");
            assert!(lookup(name).is_some(), "{name} is mounted but never served");
        }
    }

    #[test]
    fn every_panel_the_host_lists_has_somewhere_on_the_floor_to_render_into() {
        let panels = include_str!("../web/panels.js");
        let floor = include_str!("../web/floor.html");
        for id in ["run", "games", "git", "assets", "settings"] {
            assert!(
                panels.contains(&format!("id: \"{id}\"")),
                "the host does not list the {id} panel"
            );
            assert!(
                floor.contains(&format!("id=\"panel-{id}\"")),
                "the {id} panel has no host div, so the tab would open onto nothing"
            );
        }
    }

    #[test]
    fn no_served_module_is_empty() {
        for (name, body) in MODULES {
            assert!(!body.trim().is_empty(), "{name} is served as an empty file");
        }
    }

    #[test]
    fn every_image_the_floor_asks_for_is_served_as_a_png() {
        let floor = include_str!("../web/floor.html");
        for (name, body) in IMAGES {
            assert!(
                body.starts_with(b"\x89PNG\r\n\x1a\n"),
                "{name} is served as image/png but is not one"
            );
            assert!(
                floor.contains(&format!("/{name}")),
                "{name} is served but the floor never asks for it"
            );
        }
    }
}
