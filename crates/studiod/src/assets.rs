use std::path::Path;

use studio_server::assets::{capability, recorded, Capability};

pub fn readiness(studio_dir: &Path, project: &Path) -> Capability {
    capability(studio_dir, Some(project))
}

pub fn announce(studio_dir: &Path, project: &Path) {
    let cap = readiness(studio_dir, project);
    if !cap.enabled {
        return;
    }
    if !cap.ready() {
        println!("  asset generation is on but idle: {}", cap.blockers.join(" "));
        return;
    }
    match crate::skills::ensure_codex_assets(project, cap.engine.as_deref().unwrap_or("web")) {
        Ok(true) => println!("  codex asset generation is available to the art crew"),
        Ok(false) => {}
        Err(e) => println!("  codex asset skill install failed: {e}"),
    }
}

pub fn crew_hint(studio_dir: &Path, project: &Path) -> String {
    let cap = capability(studio_dir, Some(project));
    if !cap.ready() {
        return String::new();
    }

    let made = recorded(project);
    let already = if made.is_empty() {
        String::new()
    } else {
        let listed: Vec<String> = made
            .iter()
            .filter_map(|row| {
                let name = row.get("name").and_then(|v| v.as_str())?;
                let factory = row.get("factory").and_then(|v| v.as_str())?;
                Some(format!("{name} at {factory}"))
            })
            .collect();
        format!(
            " The crew has already generated: {}. Load those rather than sculpting them again.",
            listed.join("; ")
        )
    };

    format!(
        "\n\nThis studio can generate character and prop models by asking the codex CLI for \
         procedural three.js source; the {} skill in this project says how. codex writes code, \
         not pictures, so a reference image is an input to it. Use it for new models instead of \
         hand-placing primitives, and if it refuses, build the factory by hand and say so.{}",
        studio_server::assets::SKILL_NAME,
        already
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_settings::Settings;

    fn scratch(slug: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let base = std::env::temp_dir().join(format!("studiod-assets-{slug}"));
        let _ = std::fs::remove_dir_all(&base);
        let studio = base.join(".studio");
        let project = base.join("game");
        std::fs::create_dir_all(&studio).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("index.html"), "<html></html>").unwrap();
        (studio, project)
    }

    fn switch_on(studio: &Path) {
        let mut stored = Settings::new();
        stored.set(studio_server::assets::SETTING_ENABLED, true.into());
        stored.save(&Settings::path_in(studio)).unwrap();
    }

    #[test]
    fn a_studio_that_never_turned_this_on_hands_the_crew_no_extra_words_at_all() {
        let (studio, project) = scratch("off");
        assert_eq!(crew_hint(&studio, &project), "");
    }

    #[test]
    fn switching_it_on_tells_the_crew_which_skill_to_reach_for() {
        let (studio, project) = scratch("on");
        switch_on(&studio);

        let hint = crew_hint(&studio, &project);
        if !readiness(&studio, &project).ready() {
            assert_eq!(hint, "", "an unavailable codex must add nothing to a brief");
            return;
        }
        assert!(hint.contains(studio_server::assets::SKILL_NAME));
        assert!(hint.contains("writes code, not pictures"));
    }

    #[test]
    fn an_asset_the_crew_already_generated_is_named_in_the_hint_so_it_is_not_built_twice() {
        let (studio, project) = scratch("listed");
        switch_on(&studio);
        if !readiness(&studio, &project).ready() {
            return;
        }

        let manifest = studio_server::assets::manifest_path(&project);
        std::fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        std::fs::write(
            &manifest,
            r#"[{"name":"Scrapyard Scout","slug":"scrapyard_scout","factory":"src/models/scrapyard_scout.js"}]"#,
        )
        .unwrap();

        let hint = crew_hint(&studio, &project);
        assert!(hint.contains("Scrapyard Scout"));
        assert!(hint.contains("src/models/scrapyard_scout.js"));
        assert!(hint.contains("rather than sculpting them again"));
    }

    #[test]
    fn announcing_an_unavailable_generator_never_panics_and_installs_nothing() {
        let (studio, project) = scratch("announce");
        announce(&studio, &project);
        assert!(
            !project.join(".claude").exists(),
            "a switched-off feature installs no skill"
        );
    }
}
