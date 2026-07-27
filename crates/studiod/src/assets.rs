use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use studio_server::assets::{capability, recorded, AssetKind, Capability};
use studio_settings::Settings;

pub fn launcher() -> String {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "studiod".to_string())
}

fn flags(args: &[String]) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let mut at = 0;
    while at < args.len() {
        let Some(key) = args[at].strip_prefix("--") else {
            at += 1;
            continue;
        };
        let value = match args.get(at + 1) {
            Some(next) if !next.starts_with("--") => {
                at += 1;
                next.clone()
            }
            _ => "1".to_string(),
        };
        out.insert(key.to_string(), value);
        at += 1;
    }
    out
}

fn asked_project(given: Option<&String>) -> Result<PathBuf> {
    let root = match given {
        Some(at) => PathBuf::from(at),
        None => std::env::current_dir()?,
    };
    let settled = root
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("there is no project at {}: {e}", root.display()))?;
    Ok(settled)
}

fn told(record: &studio_server::assets::Generated) {
    if let Some(image) = record.image.as_deref() {
        println!("image    {image} ({}x{})", record.width, record.height);
    }
    if let Some(factory) = record.factory.as_deref() {
        println!("factory  {factory}");
    }
    if let Some(export) = record.export.as_deref() {
        println!("export   {export}");
    }
    if record.meshes > 0 {
        println!("meshes   {}", record.meshes);
    }
    if !record.clips.is_empty() {
        println!(
            "clips    {} ({})",
            record.clips.join(", "),
            if record.rigged { "rigged" } else { "incomplete rig" }
        );
    }
    println!("notes    {}", record.notes);
}

pub fn cli(args: &[String]) -> Result<()> {
    let Some(verb) = args.first().map(String::as_str) else {
        usage();
        return Ok(());
    };
    let rest = &args[1..];
    let flags = flags(rest);
    let studio = match flags.get("studio") {
        Some(at) => PathBuf::from(at),
        None => crate::studio_dir(),
    };
    let project = asked_project(flags.get("project"))?;
    let stored = Settings::load(&Settings::path_in(&studio)).unwrap_or_default();
    let cap = capability(&studio, Some(&project));

    match verb {
        "list" => {
            let made = recorded(&project);
            if made.is_empty() {
                println!("no assets generated in this project yet");
            }
            for row in made {
                println!(
                    "{:10} {:24} {}",
                    row.get("kind").and_then(|v| v.as_str()).unwrap_or("?"),
                    row.get("slug").and_then(|v| v.as_str()).unwrap_or("?"),
                    row.get("factory")
                        .and_then(|v| v.as_str())
                        .or_else(|| row.get("image").and_then(|v| v.as_str()))
                        .unwrap_or("")
                );
            }
            Ok(())
        }
        "rig" => {
            let Some(slug) = flags.get("slug") else {
                bail!("name the model to rig with --slug");
            };
            let asked = studio_server::assets::RigRequest {
                slug: slug.clone(),
                model: studio_server::assets::model_in(&stored),
            };
            match studio_server::assets::rig(&project, &cap, &asked) {
                Ok(record) => {
                    told(&record);
                    Ok(())
                }
                Err(why) => bail!("{why}"),
            }
        }
        "animate" => {
            let (Some(slug), Some(fbx)) = (flags.get("slug"), flags.get("fbx")) else {
                bail!("name the model with --slug and the downloaded clip with --fbx");
            };
            let asked = studio_server::assets::AnimateRequest {
                slug: slug.clone(),
                animation: fbx.clone(),
                name: flags.get("name").cloned(),
            };
            match studio_server::assets::animate(&project, &cap, &asked) {
                Ok(record) => {
                    told(&record);
                    Ok(())
                }
                Err(why) => bail!("{why}"),
            }
        }
        kind => {
            let Some(kind) = AssetKind::from_key(kind) else {
                bail!(
                    "{kind} is not an asset this studio makes; ask for one of {}, or rig, animate \
                     or list",
                    AssetKind::ALL.map(|k| k.key()).join(", ")
                );
            };
            let Some(name) = flags.get("name") else {
                bail!("give the asset a name with --name");
            };
            let Some(description) = flags.get("describe") else {
                bail!("say what it looks like with --describe; the words are all codex gets");
            };
            let reference = flags.get("reference").map(|r| {
                studio_server::assets::reference_in(&project, r)
                    .map_err(|e| anyhow::anyhow!("{e}"))
            });
            let reference = match reference {
                Some(Ok(at)) => Some(at),
                Some(Err(e)) => return Err(e),
                None => None,
            };

            let asked = studio_server::assets::Request {
                kind,
                name: name.clone(),
                description: description.clone(),
                reference,
                model: studio_server::assets::model_in(&stored),
                overwrite: flags.contains_key("replace"),
                concept: !flags.contains_key("no-concept")
                    && studio_server::assets::concept_in(&stored),
                rig: !flags.contains_key("no-rig") && studio_server::assets::rig_in(&stored),
            };
            match studio_server::assets::generate(&project, &cap, &asked) {
                Ok(record) => {
                    told(&record);
                    Ok(())
                }
                Err(why) => bail!("{why}"),
            }
        }
    }
}

fn usage() {
    println!("usage: studiod asset <character|prop|sprite|texture|rig|animate|list> [options]");
    println!();
    println!("  --project <path>   the project to work in; defaults to the working directory");
    println!("  --name <name>      what the asset is called");
    println!("  --describe <text>  what it looks like; codex gets nothing else");
    println!("  --reference <path> an image inside the project to match instead of drawing one");
    println!("  --replace          overwrite a file that is already there");
    println!("  --no-concept       build a model from the words alone, without drawing it first");
    println!("  --no-rig           leave a character static");
    println!("  --slug <slug>      which existing asset to rig or animate");
    println!("  --fbx <path>       a mixamo clip inside the project to retarget");
}

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

pub fn rig_failures(studio_dir: &Path, project: &Path) -> Vec<studio_verify::Failure> {
    let stored = Settings::load(&Settings::path_in(studio_dir)).unwrap_or_default();
    if !studio_server::assets::enabled_in(&stored) || !studio_server::assets::rig_in(&stored) {
        return Vec::new();
    }
    let Some(engine) = studio_server::assets::engine_of(project) else {
        return Vec::new();
    };
    if studio_server::assets::plan_for(&engine, "probe").is_none() {
        return Vec::new();
    }

    let changed = studio_core::git::is_repo(project)
        .then(|| studio_core::git::changes(project).ok())
        .flatten()
        .map(|rows| rows.into_iter().map(|c| c.path).collect::<Vec<_>>());

    let run = launcher();
    studio_server::assets::audit_rigs(project, &engine, changed.as_deref())
        .into_iter()
        .map(|found| studio_verify::Failure {
            id: format!("unrigged:{}", found.slug),
            kind: studio_verify::FailureKind::Export,
            symbol: Some(found.slug.clone()),
            file: Some(found.factory.clone()),
            line: None,
            message: found.why(),
            detail: Some(format!(
                "a character in a 3d game that cannot be posed is set dressing. Run `{run} asset \
                 rig --project {} --slug {}` to have the studio rig it, or give it the joint \
                 hierarchy and clips by hand as the {} skill describes. If this model is not a \
                 character, say so in your report and name it as a prop.",
                project.display(),
                found.slug,
                studio_server::assets::SKILL_NAME
            )),
        })
        .collect()
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
             into rigged, animated procedural three.js models the engine can load"
        }
        (true, false) => {
            "draw sprites and textures with the codex CLI's built-in image tool, background \
             already removed"
        }
        _ => "generate character and prop models by asking the codex CLI for procedural three.js \
              source",
    };

    let run = launcher();
    format!(
        "\n\nThis studio can {can}. When a task needs art, run \
         `{run} asset <character|prop|sprite|texture> --project {} --name \"...\" --describe \
         \"...\"` rather than hand-placing primitives or leaving placeholder art; it does the \
         whole pipeline and prints where every file landed. `{run} asset list --project {}` says \
         what is already there, and `rig` and `animate` work on what already exists. **Reach for \
         this before the img2threejs skill**: that skill is how a model gets sculpted by hand, \
         and this command is the studio doing it for you. The {} skill in this project explains \
         what it does and how to drive a step by hand. If it refuses, build the asset yourself \
         and say in your report that you did and why.{}",
        project.display(),
        project.display(),
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
    fn the_crew_is_handed_the_command_rather_than_told_the_feature_exists() {
        let (studio, project) = scratch("command");
        if !readiness(&studio, &project).ready() {
            return;
        }
        let hint = crew_hint(&studio, &project);
        assert!(
            hint.contains("asset sprite") || hint.contains("asset <character"),
            "a worker that has to invent the invocation will not use it: {hint}"
        );
        assert!(
            hint.contains(&project.display().to_string()),
            "the command has to name the project, because a worker's cwd is not a promise: {hint}"
        );
        assert!(hint.contains("--describe"));
        assert!(
            hint.contains("say in your report that you did and why"),
            "a silent fallback to placeholder art is the failure this whole feature exists to stop"
        );
    }

    #[test]
    fn the_asset_cli_refuses_what_it_cannot_do_instead_of_guessing() {
        let (_, project) = scratch("cli");
        let at = project.display().to_string();

        let unknown = cli(&["tileset".into(), "--project".into(), at.clone()]).unwrap_err();
        assert!(unknown.to_string().contains("not an asset this studio makes"));
        assert!(unknown.to_string().contains("sprite"));

        let unnamed = cli(&["sprite".into(), "--project".into(), at.clone()]).unwrap_err();
        assert!(unnamed.to_string().contains("--name"));

        let mute = cli(&[
            "sprite".into(),
            "--project".into(),
            at.clone(),
            "--name".into(),
            "Potion".into(),
        ])
        .unwrap_err();
        assert!(mute.to_string().contains("--describe"));

        let nowhere = cli(&["rig".into(), "--project".into(), at.clone()]).unwrap_err();
        assert!(nowhere.to_string().contains("--slug"));

        let missing = cli(&["sprite".into(), "--project".into(), "no/such/place".into()])
            .unwrap_err();
        assert!(missing.to_string().contains("there is no project at"));

        assert!(cli(&["list".into(), "--project".into(), at]).is_ok());
    }

    #[test]
    fn a_static_character_fails_the_gate_with_the_command_that_fixes_it() {
        let (studio, project) = scratch("gate");
        let models = project.join("src").join("models");
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(
            models.join("hero.js"),
            "export default function hero(T){const g=new T.Group();g.name='hero';\
             const h=new T.Mesh();h.name='head';const t=new T.Mesh();t.name='torso';\
             const a=new T.Mesh();a.name='upper_arm_l';const l=new T.Mesh();l.name='thigh_r';\
             return g;}",
        )
        .unwrap();

        let said = rig_failures(&studio, &project);
        assert_eq!(said.len(), 1, "one static character, one failure: {said:?}");
        assert_eq!(said[0].kind, studio_verify::FailureKind::Export);
        assert_eq!(said[0].file.as_deref(), Some("src/models/hero.js"));
        assert!(said[0].message.contains("reads as a character"));

        let detail = said[0].detail.clone().unwrap_or_default();
        assert!(detail.contains("asset rig"), "the fix has to be one command: {detail}");
        assert!(detail.contains("--slug hero"));
        assert!(
            detail.contains("not a character"),
            "a mislabelled prop needs a way out that is not rigging it: {detail}"
        );
    }

    #[test]
    fn the_gate_is_silent_when_the_studio_was_told_not_to_rig() {
        let (studio, project) = scratch("gate-off");
        let models = project.join("src").join("models");
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(
            models.join("hero.js"),
            "export default function hero(T){const h=new T.Mesh();h.name='head';\
             const t=new T.Mesh();t.name='torso';const a=new T.Mesh();a.name='arm_l';\
             const l=new T.Mesh();l.name='leg_r';}",
        )
        .unwrap();
        assert_eq!(rig_failures(&studio, &project).len(), 1);

        let mut stored = Settings::new();
        stored.set(studio_server::assets::SETTING_RIG, false.into());
        stored.save(&Settings::path_in(&studio)).unwrap();
        assert!(
            rig_failures(&studio, &project).is_empty(),
            "assets.rig governs the gate as well as the generator"
        );

        let mut off = Settings::new();
        off.set(studio_server::assets::SETTING_ENABLED, false.into());
        off.save(&Settings::path_in(&studio)).unwrap();
        assert!(rig_failures(&studio, &project).is_empty());
    }

    #[test]
    fn a_project_with_no_three_js_model_path_is_never_gated() {
        let base = std::env::temp_dir().join("studiod-assets-gate-2d");
        let _ = std::fs::remove_dir_all(&base);
        let studio = base.join(".studio");
        let project = base.join("game");
        std::fs::create_dir_all(&studio).unwrap();
        std::fs::create_dir_all(project.join("src").join("models")).unwrap();
        std::fs::write(project.join("main.py"), "print('hi')\n").unwrap();
        std::fs::write(
            project.join("src").join("models").join("hero.js"),
            "const h={};h.name='head';const t={};t.name='torso';\
             const a={};a.name='arm_l';const l={};l.name='leg_r';",
        )
        .unwrap();

        assert!(
            rig_failures(&studio, &project).is_empty(),
            "a 2d or python game has nowhere to put a rig, so it must never be failed for one"
        );
    }

    #[test]
    #[ignore]
    fn the_gate_agrees_with_models_this_studio_really_generated() {
        let Ok(list) = std::env::var("STUDIO_REAL_FACTORIES") else {
            println!(
                "set STUDIO_REAL_FACTORIES to <path>=<expect> pairs, comma separated, where \
                 expect is rigged or static"
            );
            return;
        };

        let base = std::env::temp_dir().join("studiod-gate-real");
        let _ = std::fs::remove_dir_all(&base);
        let studio = base.join(".studio");
        std::fs::create_dir_all(&studio).unwrap();

        for (at, expect) in list.split(',').filter_map(|pair| pair.split_once('=')) {
            let project = base.join(format!("game-{}", expect));
            let models = project.join("src").join("models");
            std::fs::create_dir_all(&models).unwrap();
            std::fs::write(project.join("index.html"), "<html></html>").unwrap();

            let profiles = studio_engine::EngineProfile::builtin();
            let web = profiles.iter().find(|p| p.id == "web").unwrap();
            studio_engine::install_helpers(web, &project).unwrap();

            let name = Path::new(at).file_name().unwrap();
            std::fs::copy(at, models.join(name)).unwrap();

            let said = rig_failures(&studio, &project);
            println!(
                "{:8} {:24} {}",
                expect,
                name.to_string_lossy(),
                if said.is_empty() {
                    "passes the gate".to_string()
                } else {
                    said[0].message.clone()
                }
            );
            match expect {
                "rigged" => assert!(
                    said.is_empty(),
                    "a model this studio rigged must not be failed by its own gate: {said:?}"
                ),
                "static" => assert_eq!(said.len(), 1, "a static character has to be caught"),
                "prop" => assert!(
                    said.is_empty(),
                    "a prop is not a character and must never be asked to walk: {said:?}"
                ),
                other => panic!("unknown expectation {other}"),
            }
            let _ = std::fs::remove_dir_all(&project);
        }
    }

    #[test]
    fn the_launcher_the_crew_is_told_to_run_is_this_binary() {
        let run = launcher();
        assert!(!run.is_empty());
        assert!(
            run.contains("studiod") || std::env::current_exe().is_err(),
            "the crew has to be given a path that exists, not a name it has to find: {run}"
        );
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
