use std::path::Path;

const SKIP_DIRS: [&str; 10] = [
    ".git", ".claude", ".godot", ".studio-out", "node_modules", "vendor",
    "target", "__pycache__", ".import", "tools",
];
const MAX_FILES: usize = 90;
const MAX_DOC_CHARS: usize = 900;
const MAX_SURVEY_CHARS: usize = 5000;

pub fn survey(root: &Path) -> Option<String> {
    let mut files = Vec::new();
    walk(root, root, &mut files, 0);
    if files.is_empty() {
        return None;
    }
    files.sort();

    let mut out = String::new();

    let profiles = studio_engine::EngineProfile::builtin();
    if let Some(d) = studio_engine::detect(root, &profiles).first() {
        out.push_str(&format!("Engine: {}\n", d.id));
    }

    let shown = files.len().min(MAX_FILES);
    out.push_str(&format!("Files ({} total):\n", files.len()));
    for f in files.iter().take(MAX_FILES) {
        out.push_str("  ");
        out.push_str(f);
        out.push('\n');
    }
    if files.len() > shown {
        out.push_str(&format!("  ... and {} more\n", files.len() - shown));
    }

    for doc in ["README.md", "design/spec.md", "qa/report.md"] {
        if let Ok(body) = std::fs::read_to_string(root.join(doc)) {
            let head: String = body.chars().take(MAX_DOC_CHARS).collect();
            out.push_str(&format!("\n--- {doc} (head) ---\n{}\n", head.trim_end()));
        }
    }

    if let Some(log) = recent_commits(root, 8) {
        if !log.trim().is_empty() {
            out.push_str(&format!("\nRecent commits:\n{log}"));
        }
    }

    if out.len() > MAX_SURVEY_CHARS {
        out.truncate(MAX_SURVEY_CHARS);
        out.push_str("\n... (survey truncated)");
    }
    Some(out)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>, depth: usize) {
    if depth > 6 || out.len() > 400 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) && !name.starts_with('.') {
                walk(root, &path, out, depth + 1);
            }
            continue;
        }
        if let Ok(rel) = path.strip_prefix(root) {
            let rel = rel.to_string_lossy().replace('\\', "/");
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            out.push(format!("{rel} ({size}b)"));
        }
    }
}

fn recent_commits(root: &Path, n: usize) -> Option<String> {
    if !studio_core::git::is_repo(root) {
        return None;
    }
    let out = std::process::Command::new("git")
        .args(["log", &format!("-{n}"), "--format=  %h %s"])
        .current_dir(root)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}
