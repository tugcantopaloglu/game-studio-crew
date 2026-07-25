use crate::{CoreError, Result};
use serde::Serialize;
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

pub fn head_sha(root: &Path) -> Option<String> {
    if !is_repo(root) {
        return None;
    }
    git(root, &["rev-parse", "--short", "HEAD"]).ok()
}

fn looks_like_a_sha(sha: &str) -> bool {
    sha.chars().all(|c| c.is_ascii_hexdigit()) && sha.len() >= 6 && sha.len() <= 40
}

pub fn reset_hard(root: &Path, sha: &str) -> Result<()> {
    if !looks_like_a_sha(sha) {
        return Err(CoreError::Git(format!("refusing to reset to '{sha}'; not a commit sha")));
    }
    git(root, &["reset", "--hard", sha])?;
    git(root, &["clean", "-fd", "--exclude=.claude", "--exclude=.studio-out"])?;
    Ok(())
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

const FIELD: char = '\u{1f}';
const LOG_FORMAT: &str = "--format=%H\u{1f}%h\u{1f}%P\u{1f}%an\u{1f}%at\u{1f}%D\u{1f}%s";
const PAGE_CEILING: usize = 500;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Commit {
    pub sha: String,
    pub short: String,
    pub parents: Vec<String>,
    pub author: String,
    pub at: i64,
    pub refs: Vec<String>,
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Row {
    pub lane: usize,
    pub links: Vec<(usize, usize)>,
    #[serde(flatten)]
    pub commit: Commit,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct History {
    pub rows: Vec<Row>,
    pub lanes: usize,
    pub more: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Change {
    pub code: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Remote {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Rollback {
    pub sha: String,
    pub subject: String,
    pub discards: Vec<Commit>,
    pub dirty: Vec<Change>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Host {
    pub gh: bool,
    pub signed_in: bool,
    pub login: Option<String>,
}

fn read_log(root: &Path, args: &[&str]) -> Result<Vec<Commit>> {
    let mut all = vec!["log", LOG_FORMAT];
    all.extend_from_slice(args);
    Ok(git(root, &all)?.lines().filter_map(parse_commit).collect())
}

fn parse_commit(line: &str) -> Option<Commit> {
    let mut field = line.split(FIELD);
    let sha = field.next()?.trim().to_string();
    if sha.is_empty() {
        return None;
    }
    Some(Commit {
        sha,
        short: field.next()?.to_string(),
        parents: field.next()?.split_whitespace().map(str::to_string).collect(),
        author: field.next()?.to_string(),
        at: field.next()?.trim().parse().unwrap_or(0),
        refs: field
            .next()?
            .split(',')
            .map(str::trim)
            .filter(|r| !r.is_empty())
            .map(str::to_string)
            .collect(),
        subject: field.next().unwrap_or("").to_string(),
    })
}

pub fn history(root: &Path, skip: usize, limit: usize) -> Result<History> {
    if !is_repo(root) || head_sha(root).is_none() {
        return Ok(History { rows: Vec::new(), lanes: 0, more: false });
    }
    let limit = limit.clamp(1, PAGE_CEILING);
    let skip = format!("--skip={skip}");
    let count = format!("--max-count={}", limit + 1);
    let mut commits = read_log(root, &["--all", "--topo-order", &skip, &count])?;

    let more = commits.len() > limit;
    commits.truncate(limit);
    Ok(lay_out(commits, more))
}

fn free_lane(lanes: &mut Vec<Option<String>>) -> usize {
    match lanes.iter().position(Option::is_none) {
        Some(i) => i,
        None => {
            lanes.push(None);
            lanes.len() - 1
        }
    }
}

fn lane_of(lanes: &[Option<String>], sha: &str) -> Option<usize> {
    lanes.iter().position(|l| l.as_deref() == Some(sha))
}

pub fn lay_out(commits: Vec<Commit>, more: bool) -> History {
    let mut lanes: Vec<Option<String>> = Vec::new();
    let mut rows = Vec::new();
    let mut widest = 0;

    for commit in commits {
        let above = lanes.clone();
        let lane = match lane_of(&lanes, &commit.sha) {
            Some(i) => i,
            None => free_lane(&mut lanes),
        };
        lanes[lane] = None;

        for (n, parent) in commit.parents.iter().enumerate() {
            let target = match lane_of(&lanes, parent) {
                Some(held) if n == 0 && held > lane && lanes[lane].is_none() => {
                    lanes[held] = None;
                    lane
                }
                Some(_) => continue,
                None if n == 0 && lanes[lane].is_none() => lane,
                None => free_lane(&mut lanes),
            };
            lanes[target] = Some(parent.clone());
        }

        let mut links = Vec::new();
        for (i, held) in above.iter().enumerate() {
            if i == lane {
                continue;
            }
            if let Some(sha) = held {
                if let Some(to) = lane_of(&lanes, sha) {
                    links.push((i, to));
                }
            }
        }
        for parent in &commit.parents {
            if let Some(to) = lane_of(&lanes, parent) {
                links.push((lane, to));
            }
        }

        widest = widest.max(lanes.iter().rposition(Option::is_some).map(|i| i + 1).unwrap_or(0));
        widest = widest.max(lane + 1);
        rows.push(Row { lane, links, commit });
    }

    History { rows, lanes: widest, more }
}

pub fn branch(root: &Path) -> Option<String> {
    let name = git(root, &["rev-parse", "--abbrev-ref", "HEAD"]).ok()?;
    if name.is_empty() || name == "HEAD" {
        return None;
    }
    Some(name)
}

pub fn changes(root: &Path) -> Result<Vec<Change>> {
    let text = git(root, &["status", "--porcelain"])?;
    Ok(text
        .lines()
        .filter(|l| l.len() > 3)
        .map(|l| {
            let (code, path) = l.split_at(2);
            Change { code: code.trim().to_string(), path: path.trim().to_string() }
        })
        .collect())
}

pub fn remotes(root: &Path) -> Result<Vec<Remote>> {
    let text = git(root, &["remote", "-v"])?;
    let mut out: Vec<Remote> = Vec::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(name), Some(url)) = (parts.next(), parts.next()) else {
            continue;
        };
        if out.iter().any(|r| r.name == name) {
            continue;
        }
        out.push(Remote { name: name.to_string(), url: url.to_string() });
    }
    Ok(out)
}

fn carries_a_credential(url: &str) -> bool {
    let Some((_, rest)) = url.split_once("://") else {
        return false;
    };
    let host = rest.split('/').next().unwrap_or("");
    match host.split_once('@') {
        Some((userinfo, _)) => {
            userinfo.contains(':')
                || userinfo.starts_with("ghp_")
                || userinfo.starts_with("github_pat_")
        }
        None => false,
    }
}

pub fn set_remote(root: &Path, url: &str) -> Result<()> {
    let url = url.trim();
    if url.is_empty() {
        return Err(CoreError::Git(
            "give a remote URL, like https://github.com/you/your-game.git".into(),
        ));
    }
    if carries_a_credential(url) {
        return Err(CoreError::Git(
            "that URL carries a credential; the studio never stores one. Give the plain URL and \
             sign in with the gh CLI, or use an SSH remote."
                .into(),
        ));
    }
    let named = remotes(root)?.iter().any(|r| r.name == "origin");
    if named {
        git(root, &["remote", "set-url", "origin", url])?;
    } else {
        git(root, &["remote", "add", "origin", url])?;
    }
    Ok(())
}

fn advice(said: &str) -> Option<&'static str> {
    let lower = said.to_lowercase();
    if lower.contains("non-fast-forward") || lower.contains("fetch first") {
        return Some("the remote has commits this branch does not; fetch and merge them, then push again");
    }
    if lower.contains("authentication failed")
        || lower.contains("could not read username")
        || lower.contains("terminal prompts disabled")
        || lower.contains("permission denied")
    {
        return Some("git could not authenticate to the remote; sign in with `gh auth login`, or point the remote at SSH");
    }
    if lower.contains("could not resolve host") || lower.contains("unable to access") {
        return Some("git could not reach the remote; check the network and the remote URL");
    }
    None
}

pub fn push(root: &Path) -> Result<String> {
    let remotes = remotes(root)?;
    let Some(remote) = remotes.iter().find(|r| r.name == "origin").or_else(|| remotes.first())
    else {
        return Err(CoreError::Git(
            "this project has no remote yet. Create one with the gh CLI, or paste a remote URL \
             and set it, then push again."
                .into(),
        ));
    };
    let Some(branch) = branch(root) else {
        return Err(CoreError::Git(
            "this repository is not on a branch; check one out before pushing".into(),
        ));
    };

    let out = Command::new("git")
        .args(["push", "--set-upstream", &remote.name, &branch])
        .env("GIT_TERMINAL_PROMPT", "0")
        .current_dir(root)
        .output()?;

    let said = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout).trim(),
        String::from_utf8_lossy(&out.stderr).trim()
    );
    let said = said.trim().to_string();

    if !out.status.success() {
        return Err(CoreError::Git(match advice(&said) {
            Some(next) => format!("push to {} was rejected: {said}\n{next}", remote.name),
            None => format!("push to {} was rejected: {said}", remote.name),
        }));
    }
    Ok(if said.is_empty() {
        format!("{branch} on {} is already up to date", remote.name)
    } else {
        said
    })
}

fn login_in(text: &str) -> Option<String> {
    text.split_whitespace()
        .skip_while(|w| *w != "account")
        .nth(1)
        .map(str::to_string)
}

pub fn host() -> Host {
    let present = Command::new("gh")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !present {
        return Host { gh: false, signed_in: false, login: None };
    }
    let Ok(out) = Command::new("gh").args(["auth", "status"]).output() else {
        return Host { gh: true, signed_in: false, login: None };
    };
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Host { gh: true, signed_in: out.status.success(), login: login_in(&said) }
}

fn is_a_repo_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

pub fn create_remote(root: &Path, name: &str, private: bool) -> Result<String> {
    let host = host();
    if !host.gh {
        return Err(CoreError::Git(
            "the gh CLI is not on PATH, so I cannot create the repository for you; \
             paste a remote URL instead"
                .into(),
        ));
    }
    if !host.signed_in {
        return Err(CoreError::Git(
            "the gh CLI is installed but not signed in; run `gh auth login` in a terminal, \
             or paste a remote URL instead"
                .into(),
        ));
    }
    let name = name.trim();
    if !is_a_repo_name(name) {
        return Err(CoreError::Git(format!(
            "'{name}' is not a repository name; use letters, digits, dashes, dots and underscores"
        )));
    }
    if !is_repo(root) {
        return Err(CoreError::Git(
            "this project is not a git repository yet; open it with git before creating a remote"
                .into(),
        ));
    }

    let visibility = if private { "--private" } else { "--public" };
    let out = Command::new("gh")
        .args(["repo", "create", name, visibility, "--source", ".", "--remote", "origin"])
        .current_dir(root)
        .output()?;
    if !out.status.success() {
        return Err(CoreError::Git(format!(
            "gh could not create {name}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }

    remotes(root)?
        .into_iter()
        .find(|r| r.name == "origin")
        .map(|r| r.url)
        .ok_or_else(|| CoreError::Git(format!("gh reported creating {name} but set no remote")))
}

pub fn is_commit(root: &Path, sha: &str) -> bool {
    git(root, &["cat-file", "-t", sha]).map(|kind| kind == "commit").unwrap_or(false)
}

pub fn rollback_plan(root: &Path, sha: &str) -> Result<Rollback> {
    if !looks_like_a_sha(sha) {
        return Err(CoreError::Git(format!(
            "refusing to roll back to '{sha}'; that is not a commit sha"
        )));
    }
    if !is_commit(root, sha) {
        return Err(CoreError::Git(format!(
            "refusing to roll back to '{sha}'; this repository holds no commit by that name"
        )));
    }
    let range = format!("{sha}..HEAD");
    Ok(Rollback {
        sha: sha.to_string(),
        subject: read_log(root, &["--max-count=1", sha])?
            .first()
            .map(|c| c.subject.clone())
            .unwrap_or_default(),
        discards: read_log(root, &[&range])?,
        dirty: changes(root)?,
    })
}

pub fn rollback(root: &Path, sha: &str) -> Result<Rollback> {
    let plan = rollback_plan(root, sha)?;
    reset_hard(root, sha)?;
    Ok(plan)
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

    fn run(dir: &std::path::Path, args: &[&str]) {
        let out = Command::new("git")
            .args(["-c", "user.name=Studio Test", "-c", "user.email=test@localhost"])
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn write_and_commit(dir: &std::path::Path, file: &str, subject: &str) -> String {
        std::fs::write(dir.join(file), format!("{subject}\n")).unwrap();
        commit(dir, subject).unwrap().expect("a written file should produce a commit")
    }

    #[test]
    fn the_lane_layout_rejoins_a_merged_branch_to_the_commit_it_forked_from() {
        if !available() {
            return;
        }
        let dir = scratch("merge");
        init(&dir).unwrap();

        run(&dir, &["checkout", "-b", "feature"]);
        write_and_commit(&dir, "dash.gd", "gameplay_engineer: the feature-side commit");
        run(&dir, &["checkout", "main"]);
        write_and_commit(&dir, "hud.gd", "ui_engineer: the main-side commit");
        run(&dir, &["merge", "--no-ff", "feature", "-m", "studio: merge the feature branch"]);

        let page = history(&dir, 0, 50).unwrap();
        assert_eq!(page.rows.len(), 4, "four commits, four rows");
        assert_eq!(page.lanes, 2, "one fork needs exactly two lanes");

        let row = |needle: &str| {
            page.rows
                .iter()
                .find(|r| r.commit.subject.contains(needle))
                .unwrap_or_else(|| panic!("no row for {needle}"))
        };
        let merge = row("merge the feature");
        let mainside = row("main-side");
        let feature = row("feature-side");
        let fork = row("open the project");

        assert_eq!(merge.commit.parents.len(), 2);
        assert_eq!(merge.links.len(), 2, "a merge sends one edge to each parent");
        assert_ne!(merge.links[0].1, merge.links[1].1, "its parents must land in two lanes");
        assert_eq!(merge.lane, mainside.lane, "a merge stays in its first parent's lane");
        assert_ne!(mainside.lane, feature.lane, "the two sides must not share a lane");

        assert_eq!(fork.lane, 0, "the trunk stays in the leftmost lane");
        assert!(
            page.rows
                .iter()
                .flat_map(|r| r.links.iter())
                .any(|(from, to)| from != to && *to == fork.lane),
            "the branch must bend back into the lane the fork point sits in"
        );
        assert!(fork.links.is_empty(), "the root commit has nothing below it");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_tree_carries_the_refs_that_point_at_each_commit() {
        if !available() {
            return;
        }
        let dir = scratch("refs");
        init(&dir).unwrap();
        run(&dir, &["tag", "v0.1"]);

        let page = history(&dir, 0, 10).unwrap();
        let head = &page.rows[0].commit;
        assert!(head.refs.iter().any(|r| r.contains("main")), "{:?}", head.refs);
        assert!(head.refs.iter().any(|r| r.contains("v0.1")), "{:?}", head.refs);
        assert!(!head.author.is_empty());
        assert!(head.at > 0, "a commit without a timestamp cannot be shown as an age");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_tree_pages_so_a_long_history_never_arrives_at_once() {
        if !available() {
            return;
        }
        let dir = scratch("paged");
        init(&dir).unwrap();
        for n in 0..12 {
            write_and_commit(&dir, &format!("level{n}.tscn"), &format!("level_designer: level {n}"));
        }

        let first = history(&dir, 0, 5).unwrap();
        assert_eq!(first.rows.len(), 5);
        assert!(first.more, "thirteen commits do not fit in a page of five");

        let second = history(&dir, 5, 5).unwrap();
        assert_eq!(second.rows.len(), 5);
        assert_ne!(first.rows[0].commit.sha, second.rows[0].commit.sha);

        let last = history(&dir, 10, 5).unwrap();
        assert_eq!(last.rows.len(), 3);
        assert!(!last.more, "the final page must say it is the final page");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_tree_of_a_repository_with_no_commits_is_empty_rather_than_an_error() {
        if !available() {
            return;
        }
        let dir = scratch("nocommits");
        run(&dir, &["init"]);

        let page = history(&dir, 0, 20).unwrap();
        assert!(page.rows.is_empty());
        assert_eq!(page.lanes, 0);
        assert!(!page.more);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_push_with_no_remote_says_what_to_do_instead_of_repeating_git() {
        if !available() {
            return;
        }
        let dir = scratch("noremote");
        init(&dir).unwrap();

        let complaint = push(&dir).unwrap_err().to_string();
        assert!(complaint.contains("no remote"), "{complaint}");
        assert!(complaint.contains("gh"), "the message must name a way forward: {complaint}");
        assert!(complaint.contains("remote URL"), "{complaint}");
        assert!(!complaint.contains("fatal:"), "raw git output is not an instruction: {complaint}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_push_reports_exactly_what_the_remote_did() {
        if !available() {
            return;
        }
        let dir = scratch("push");
        let bare = scratch("bare");
        run(&bare, &["init", "--bare"]);
        init(&dir).unwrap();
        set_remote(&dir, &bare.to_string_lossy()).unwrap();

        let said = push(&dir).unwrap();
        assert!(said.contains("main"), "the report must name the branch: {said}");

        let landed = Command::new("git")
            .args(["rev-parse", "--short", "main"])
            .current_dir(&bare)
            .output()
            .unwrap();
        assert!(landed.status.success(), "the remote did not receive the branch");
        assert_eq!(
            String::from_utf8_lossy(&landed.stdout).trim(),
            head_sha(&dir).unwrap(),
            "a push that reports success must have moved the remote"
        );

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&bare);
    }

    #[test]
    fn a_rejected_push_is_reported_as_a_rejection_with_the_reason() {
        if !available() {
            return;
        }
        let dir = scratch("rejected");
        let other = scratch("rejected-peer");
        let bare = scratch("rejected-bare");
        run(&bare, &["init", "--bare"]);
        run(&bare, &["symbolic-ref", "HEAD", "refs/heads/main"]);

        init(&dir).unwrap();
        set_remote(&dir, &bare.to_string_lossy()).unwrap();
        push(&dir).unwrap();

        run(&other, &["clone", &bare.to_string_lossy(), "."]);
        std::fs::write(other.join("peer.gd"), "peer\n").unwrap();
        run(&other, &["add", "-A"]);
        run(&other, &["commit", "-m", "producer: a commit from elsewhere"]);
        run(&other, &["push", "origin", "main"]);

        write_and_commit(&dir, "mine.gd", "gameplay_engineer: a commit of my own");
        let complaint = push(&dir).unwrap_err().to_string();
        assert!(complaint.contains("rejected"), "{complaint}");
        assert!(complaint.contains("fetch"), "the reason must be actionable: {complaint}");

        for dir in [dir, other, bare] {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn a_remote_url_carrying_a_credential_is_refused_and_never_stored() {
        if !available() {
            return;
        }
        let dir = scratch("token");
        init(&dir).unwrap();

        for leaky in [
            "https://ghp_0123456789abcdef@github.com/you/game.git",
            "https://you:hunter2@github.com/you/game.git",
        ] {
            let complaint = set_remote(&dir, leaky).unwrap_err().to_string();
            assert!(complaint.contains("never stores"), "{complaint}");
            assert!(!complaint.contains("hunter2"), "the message repeated the secret");
            assert!(remotes(&dir).unwrap().is_empty(), "a refused URL must not be written");
        }

        set_remote(&dir, "https://github.com/you/game.git").unwrap();
        set_remote(&dir, "git@github.com:you/game.git").unwrap();
        let stored = remotes(&dir).unwrap();
        assert_eq!(stored.len(), 1, "setting a remote twice replaces it");
        assert_eq!(stored[0].url, "git@github.com:you/game.git");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rollback_refuses_a_sha_that_is_not_a_commit_in_this_repository() {
        if !available() {
            return;
        }
        let dir = scratch("rollback-refuse");
        init(&dir).unwrap();
        let head = head_sha(&dir).unwrap();

        let complaint = rollback(&dir, "main").unwrap_err().to_string();
        assert!(complaint.contains("not a commit sha"), "{complaint}");

        let complaint = rollback(&dir, "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef")
            .unwrap_err()
            .to_string();
        assert!(complaint.contains("holds no commit"), "{complaint}");

        let tree = git(&dir, &["rev-parse", "HEAD^{tree}"]).unwrap();
        let complaint = rollback(&dir, &tree).unwrap_err().to_string();
        assert!(complaint.contains("holds no commit"), "a tree is not a commit: {complaint}");

        assert_eq!(head_sha(&dir).unwrap(), head, "a refused rollback must move nothing");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rollback_names_every_commit_and_dirty_file_it_is_about_to_destroy() {
        if !available() {
            return;
        }
        let dir = scratch("rollback-plan");
        init(&dir).unwrap();
        let fork = head_sha(&dir).unwrap();

        write_and_commit(&dir, "dash.gd", "gameplay_engineer: add a dash");
        write_and_commit(&dir, "hud.gd", "ui_engineer: add a hud");
        std::fs::write(dir.join("dash.gd"), "edited by hand\n").unwrap();
        std::fs::write(dir.join("notes.txt"), "never committed\n").unwrap();

        let plan = rollback_plan(&dir, &fork).unwrap();
        assert_eq!(plan.discards.len(), 2, "two commits are about to be thrown away");
        assert!(plan.discards.iter().any(|c| c.subject.contains("add a dash")));
        assert!(plan.dirty.iter().any(|c| c.path.contains("dash.gd")), "{:?}", plan.dirty);
        assert!(
            plan.dirty.iter().any(|c| c.path.contains("notes.txt")),
            "an untracked file dies in the clean and must be listed: {:?}",
            plan.dirty
        );

        let done = rollback(&dir, &fork).unwrap();
        assert_eq!(done.discards.len(), 2);
        assert_eq!(head_sha(&dir).unwrap(), fork);
        assert!(!dir.join("dash.gd").exists());
        assert!(!dir.join("notes.txt").exists());
        assert!(!has_changes(&dir).unwrap());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_url_without_a_credential_is_left_alone() {
        assert!(!carries_a_credential("https://github.com/you/game.git"));
        assert!(!carries_a_credential("git@github.com:you/game.git"));
        assert!(!carries_a_credential("C:\\games\\bare.git"));
        assert!(carries_a_credential("https://ghp_abc@github.com/you/game.git"));
        assert!(carries_a_credential("https://you:token@github.com/you/game.git"));
    }

    #[test]
    fn the_signed_in_account_is_read_out_of_what_gh_prints() {
        let said = "github.com\n  x Logged in to github.com account octocat (keyring)\n";
        assert_eq!(login_in(said).as_deref(), Some("octocat"));
        assert_eq!(login_in("not logged in"), None);
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
