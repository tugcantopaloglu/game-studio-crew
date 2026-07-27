use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub const IMG2THREEJS_REPO: &str = "https://github.com/hoainho/img2threejs";
pub const IMG2THREEJS_TAG: &str = "v1.3";

pub fn ensure_img2threejs(project: &Path) -> Result<bool> {
    let dest = skills_dir(project).join("img2threejs");
    if dest.join("SKILL.md").exists() {
        return Ok(false);
    }

    let cache = cache_dir()?.join(format!("img2threejs-{IMG2THREEJS_TAG}"));
    if !cache.join("SKILL.md").exists() {
        let _ = std::fs::remove_dir_all(&cache);
        std::fs::create_dir_all(cache.parent().unwrap())?;
        let status = studio_core::command("git")
            .args([
                "clone",
                "--depth",
                "1",
                "--branch",
                IMG2THREEJS_TAG,
                IMG2THREEJS_REPO,
            ])
            .arg(&cache)
            .status()
            .context("git is not available to fetch the img2threejs skill")?;
        if !status.success() {
            anyhow::bail!("git clone of {IMG2THREEJS_REPO} {IMG2THREEJS_TAG} failed");
        }
    }

    copy_dir(&cache, &dest)?;
    Ok(true)
}

pub fn ensure_codex_assets(project: &Path, engine: &str) -> Result<bool> {
    let dir = skills_dir(project).join(studio_server::assets::SKILL_NAME);
    let doc = dir.join("SKILL.md");
    let wanted = studio_server::assets::skill_body(engine);
    if std::fs::read_to_string(&doc).ok().as_deref() == Some(wanted.as_str()) {
        return Ok(false);
    }
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("could not create {}", dir.display()))?;
    std::fs::write(&doc, wanted).with_context(|| format!("could not write {}", doc.display()))?;
    Ok(true)
}

fn skills_dir(project: &Path) -> PathBuf {
    project.join(".claude").join("skills")
}

fn cache_dir() -> Result<PathBuf> {
    let base = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    Ok(base.join("studiod").join("skills"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(slug: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("studiod-skill-{slug}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_asset_skill_lands_beside_the_other_project_skills() {
        let project = scratch("place");
        assert!(ensure_codex_assets(&project, "web").unwrap());

        let doc = project
            .join(".claude")
            .join("skills")
            .join(studio_server::assets::SKILL_NAME)
            .join("SKILL.md");
        assert!(doc.is_file(), "a skill claude can find has to be a SKILL.md");
        let body = std::fs::read_to_string(&doc).unwrap();
        assert!(body.starts_with("---\nname: codex-assets"));
        assert!(body.contains("It cannot draw a picture"));
        assert!(body.contains("src/models/<slug>.js"));
    }

    #[test]
    fn installing_the_asset_skill_twice_writes_nothing_the_second_time() {
        let project = scratch("idempotent");
        assert!(ensure_codex_assets(&project, "web").unwrap());
        assert!(!ensure_codex_assets(&project, "web").unwrap());
    }

    #[test]
    fn a_glb_engine_gets_a_skill_that_names_the_export_step_instead_of_the_browser() {
        let project = scratch("godot");
        ensure_codex_assets(&project, "godot").unwrap();
        let body = std::fs::read_to_string(
            project
                .join(".claude")
                .join("skills")
                .join(studio_server::assets::SKILL_NAME)
                .join("SKILL.md"),
        )
        .unwrap();
        assert!(body.contains("assets/models/<slug>.glb"));
        assert!(body.contains("imports the .glb"));
    }

    #[test]
    fn a_changed_skill_replaces_the_one_a_previous_version_installed() {
        let project = scratch("rewrite");
        ensure_codex_assets(&project, "web").unwrap();
        let doc = project
            .join(".claude")
            .join("skills")
            .join(studio_server::assets::SKILL_NAME)
            .join("SKILL.md");
        std::fs::write(&doc, "an older studio wrote this").unwrap();

        assert!(ensure_codex_assets(&project, "web").unwrap());
        assert!(std::fs::read_to_string(&doc).unwrap().contains("codex-assets"));
    }
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let src = entry.path();
        let dst = to.join(&name);
        if entry.file_type()?.is_dir() {
            copy_dir(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}
