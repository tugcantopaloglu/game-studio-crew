use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub const IMG2THREEJS_REPO: &str = "https://github.com/hoainho/img2threejs";
pub const IMG2THREEJS_TAG: &str = "v1.3";

pub fn ensure_img2threejs(project: &Path) -> Result<bool> {
    let dest = project.join(".claude").join("skills").join("img2threejs");
    if dest.join("SKILL.md").exists() {
        return Ok(false);
    }

    let cache = cache_dir()?.join(format!("img2threejs-{IMG2THREEJS_TAG}"));
    if !cache.join("SKILL.md").exists() {
        let _ = std::fs::remove_dir_all(&cache);
        std::fs::create_dir_all(cache.parent().unwrap())?;
        let status = std::process::Command::new("git")
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

fn cache_dir() -> Result<PathBuf> {
    let base = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    Ok(base.join("studiod").join("skills"))
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
