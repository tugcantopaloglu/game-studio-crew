use crate::{CoreError, Result};
use std::path::Path;
use std::process::Command;

const GITIGNORE: &str = "\
.studio-out/
.godot/
.import/
build/
export/
*.tmp
Library/
Temp/
Logs/
Binaries/
Intermediate/
DerivedDataCache/
Saved/
";

fn git(root: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git").args(args).current_dir(root).output()?;
    if !out.status.success() {
        return Err(CoreError::Git(format!(
            "{} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn is_repo(root: &Path) -> bool {
    root.join(".git").exists()
}

pub fn init(root: &Path) -> Result<()> {
    if is_repo(root) {
        return Ok(());
    }
    git(root, &["init"])?;
    git(root, &["symbolic-ref", "HEAD", "refs/heads/main"])?;

    let ignore = root.join(".gitignore");
    if !ignore.exists() {
        std::fs::write(&ignore, GITIGNORE)?;
    }

    if has_changes(root)? {
        commit_as(root, "studio: open the project")?;
    }
    Ok(())
}

pub fn has_changes(root: &Path) -> Result<bool> {
    Ok(!git(root, &["status", "--porcelain"])?.is_empty())
}

pub fn commit(root: &Path, subject: &str) -> Result<Option<String>> {
    if !is_repo(root) || !has_changes(root)? {
        return Ok(None);
    }
    commit_as(root, subject).map(Some)
}

fn commit_as(root: &Path, subject: &str) -> Result<String> {
    git(root, &["add", "-A"])?;

    let out = Command::new("git")
        .args([
            "-c",
            "user.name=Game Studio",
            "-c",
            "user.email=studio@localhost",
            "commit",
            "-m",
            subject,
        ])
        .current_dir(root)
        .output()?;
    if !out.status.success() {
        return Err(CoreError::Git(format!(
            "commit failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    git(root, &["rev-parse", "--short", "HEAD"])
}

pub fn subject(role: &str, brief: &str) -> String {
    let line = brief
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("work");

    let mut summary: String = line.chars().take(68).collect();
    if line.chars().count() > 68 {
        while !summary.is_empty() && !summary.ends_with(' ') {
            summary.pop();
        }
        let trimmed = summary.trim_end();
        summary = format!("{trimmed}...");
    }
    format!("{role}: {}", summary.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subject_is_role_then_first_line() {
        assert_eq!(
            subject("gameplay_engineer", "Add a dash ability\nwith cooldown"),
            "gameplay_engineer: Add a dash ability"
        );
    }

    #[test]
    fn subject_skips_leading_blank_lines() {
        assert_eq!(subject("artist", "\n\n  Paint the sky  "), "artist: Paint the sky");
    }

    #[test]
    fn subject_truncates_on_a_word_boundary() {
        let brief = "a".repeat(40) + " " + &"b".repeat(40);
        let s = subject("qa_engineer", &brief);
        assert!(s.ends_with("..."), "{s}");
        assert!(s.len() < 90, "{s}");
    }

    #[test]
    fn subject_never_mentions_the_tooling() {
        let s = subject("producer", "Ship the vertical slice");
        for banned in ["claude", "Claude", "AI", "Co-Authored", "Generated with"] {
            assert!(!s.contains(banned), "{s} leaked {banned}");
        }
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("studio-git-{tag}-{stamp}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn init_then_commit_records_history_without_naming_the_tooling() {
        if !available() {
            return;
        }
        let dir = scratch("roundtrip");

        init(&dir).unwrap();
        assert!(is_repo(&dir));
        assert!(dir.join(".gitignore").is_file());

        std::fs::write(dir.join("Player.gd"), "extends CharacterBody2D\n").unwrap();
        assert!(has_changes(&dir).unwrap());

        let sha = commit(&dir, &subject("gameplay_engineer", "Add a dash ability"))
            .unwrap()
            .expect("a dirty tree should produce a commit");
        assert!(!sha.is_empty());
        assert!(!has_changes(&dir).unwrap());

        let log = Command::new("git")
            .args(["log", "--format=%s%n%b%n%an <%ae>"])
            .current_dir(&dir)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&log.stdout);
        assert!(text.contains("gameplay_engineer: Add a dash ability"), "{text}");
        for banned in ["laude", "Co-Authored", "Generated with", "anthropic"] {
            assert!(!text.contains(banned), "commit log leaked {banned}:\n{text}");
        }

        assert!(commit(&dir, "artist: nothing changed").unwrap().is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn init_leaves_a_clean_tree_so_the_first_worker_does_not_inherit_the_ignore_file() {
        if !available() {
            return;
        }
        let dir = scratch("cleaninit");
        init(&dir).unwrap();

        assert!(
            !has_changes(&dir).unwrap(),
            "an untracked .gitignore would be swept into whichever worker commits first"
        );
        assert!(commit(&dir, "game_designer: first real work").unwrap().is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verify_output_never_reaches_a_commit() {
        if !available() {
            return;
        }
        let dir = scratch("ignored");
        init(&dir).unwrap();

        std::fs::create_dir_all(dir.join(".studio-out")).unwrap();
        std::fs::write(dir.join(".studio-out/report.json"), "{}").unwrap();
        std::fs::create_dir_all(dir.join(".godot")).unwrap();
        std::fs::write(dir.join(".godot/cache"), "x").unwrap();

        assert!(
            !has_changes(&dir).unwrap(),
            "engine and verify artefacts must be ignored, not committed"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn init_is_idempotent_and_keeps_an_existing_ignore_file() {
        if !available() {
            return;
        }
        let dir = scratch("idempotent");
        std::fs::write(dir.join(".gitignore"), "mine/\n").unwrap();

        init(&dir).unwrap();
        init(&dir).unwrap();

        assert_eq!(std::fs::read_to_string(dir.join(".gitignore")).unwrap(), "mine/\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn commit_on_a_non_repo_is_a_no_op() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("studio-git-{stamp}"));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(commit(&dir, "role: nothing").unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
