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
                let at = row
                    .get("factory")
                    .and_then(|v| v.as_str())
                    .or_else(|| row.get("image").and_then(|v| v.as_str()))?;
                Some(format!("{name} at {at}"))
            })
            .collect();
        format!(
            " The crew has already generated: {}. Load those rather than building them again.",
            listed.join("; ")
        )
    };

    let can = match (cap.draws(), cap.models()) {
        (true, true) => {
            "draw sprites and textures with the codex CLI's built-in image tool and turn them \
             into procedural three.js models the engine can load"
        }
        (true, false) => {
            "draw sprites and textures with the codex CLI's built-in image tool, background \
             already removed"
        }
        _ => "generate character and prop models by asking the codex CLI for procedural three.js \
              source",
    };

    format!(
        "\n\nThis studio can {can}; the {} skill in this project says how. Use it for new assets \
         instead of hand-placing primitives or shipping placeholder art, and if it refuses, build \
         the asset by hand and say so.{}",
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

    fn switch_off(studio: &Path) {
        let mut stored = Settings::new();
        stored.set(studio_server::assets::SETTING_ENABLED, false.into());
        stored.save(&Settings::path_in(studio)).unwrap();
    }

    #[test]
    fn a_studio_that_switched_this_off_hands_the_crew_no_extra_words_at_all() {
        let (studio, project) = scratch("off");
        switch_off(&studio);
        assert_eq!(crew_hint(&studio, &project), "");
    }

    #[test]
    fn the_crew_is_told_which_skill_to_reach_for_without_anyone_switching_it_on() {
        let (studio, project) = scratch("on");

        let hint = crew_hint(&studio, &project);
        if !readiness(&studio, &project).ready() {
            assert_eq!(hint, "", "an unavailable codex must add nothing to a brief");
            return;
        }
        assert!(hint.contains(studio_server::assets::SKILL_NAME));
        assert!(
            !hint.contains("not pictures"),
            "the studio spent two versions telling its crew a false thing: {hint}"
        );
    }

    #[test]
    fn an_asset_the_crew_already_generated_is_named_in_the_hint_so_it_is_not_built_twice() {
        let (studio, project) = scratch("listed");
        if !readiness(&studio, &project).ready() {
            return;
        }

        let manifest = studio_server::assets::manifest_path(&project);
        std::fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        std::fs::write(
            &manifest,
            r#"[{"name":"Scrapyard Scout","slug":"scrapyard_scout","factory":"src/models/scrapyard_scout.js"},
                {"name":"Health Potion","slug":"health_potion","image":"assets/sprites/health_potion.png"}]"#,
        )
        .unwrap();

        let hint = crew_hint(&studio, &project);
        assert!(
            hint.contains("assets/sprites/health_potion.png"),
            "a drawn sprite has no factory, and listing only factories would hide it: {hint}"
        );
        assert!(hint.contains("Scrapyard Scout"));
        assert!(hint.contains("src/models/scrapyard_scout.js"));
        assert!(hint.contains("rather than building them again"));
    }

    #[test]
    fn announcing_a_switched_off_generator_never_panics_and_installs_nothing() {
        let (studio, project) = scratch("announce");
        switch_off(&studio);
        announce(&studio, &project);
        assert!(
            !project.join(".claude").exists(),
            "a switched-off feature installs no skill"
        );
    }
}
