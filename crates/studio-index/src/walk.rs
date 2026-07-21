use crate::{Index, Refresh, Result};
use std::path::Path;
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanReport {
    pub seen: usize,
    pub indexed: usize,
    pub unchanged: usize,
    pub removed: usize,
}

const IGNORED_DIRS: &[&str] = &[
    ".git",
    ".studio",
    ".godot",
    ".import",
    ".vs",
    ".vscode",
    ".idea",
    "node_modules",
    "target",
    "Library",
    "Temp",
    "Obj",
    "Logs",
    "UserSettings",
    "Binaries",
    "Intermediate",
    "DerivedDataCache",
    "Saved",
    "build",
    "builds",
];

pub fn is_ignored_dir(name: &str) -> bool {
    IGNORED_DIRS
        .iter()
        .any(|d| d.eq_ignore_ascii_case(name))
}

impl Index {
    pub fn scan(&mut self, root: &Path) -> Result<ScanReport> {
        let mut found = Vec::new();
        collect(root, root, &mut found)?;

        let mut report = ScanReport::default();
        for relative in &found {
            let absolute = root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
            let Ok(bytes) = std::fs::read(&absolute) else {
                continue;
            };
            let mtime = modified_seconds(&absolute);

            report.seen += 1;
            match self.index_file(relative, &bytes, &mtime)? {
                Refresh::Indexed => report.indexed += 1,
                Refresh::Unchanged => report.unchanged += 1,
            }
        }

        for stale in self.paths_outside(&found)? {
            self.forget(&stale)?;
            report.removed += 1;
        }

        Ok(report)
    }

    fn paths_outside(&self, present: &[String]) -> Result<Vec<String>> {
        let known = self.query_names("SELECT path FROM files", [])?;
        Ok(known
            .into_iter()
            .filter(|p| !present.contains(p))
            .collect())
    }
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return Ok(()),
        Err(e) => return Err(e.into()),
    };

    for entry in entries {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();

        if file_type.is_dir() {
            if !is_ignored_dir(&name) {
                collect(root, &entry.path(), out)?;
            }
            continue;
        }

        if let Some(relative) = relative_slash_path(root, &entry.path()) {
            out.push(relative);
        }
    }

    Ok(())
}

fn relative_slash_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    Some(relative.to_string_lossy().replace('\\', "/"))
}

fn modified_seconds(path: &Path) -> String {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(root: &Path, relative: &str, body: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    #[test]
    fn ignored_directories_are_matched_case_insensitively() {
        assert!(is_ignored_dir(".git"));
        assert!(is_ignored_dir("Library"));
        assert!(is_ignored_dir("library"));
        assert!(!is_ignored_dir("scripts"));
        assert!(!is_ignored_dir("addons"));
    }

    #[test]
    fn a_scan_indexes_sources_and_skips_engine_caches() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "scripts/player.gd", "class_name Player\n\nfunc go():\n\tpass\n");
        write(root, "project.godot", "[application]\n");
        write(root, ".godot/cache.gd", "func junk():\n\tpass\n");

        let mut index = Index::open_in_memory().unwrap();
        let report = index.scan(root).unwrap();

        assert_eq!(report.seen, 2);
        assert_eq!(report.indexed, 2);
        assert!(index.lookup("Player.go", 5).unwrap().len() == 1);
        assert!(index.lookup("junk", 5).unwrap().is_empty());
    }

    #[test]
    fn the_index_does_not_index_its_own_database() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "scripts/player.gd", "class_name Player\n\nfunc go():\n\tpass\n");
        write(root, ".studio/studio-index.db", "sqlite bytes");
        write(root, ".studio/studio-index.db-wal", "wal bytes");

        let mut index = Index::open_in_memory().unwrap();
        let report = index.scan(root).unwrap();

        assert_eq!(report.seen, 1);
        assert!(index.file(".studio/studio-index.db").unwrap().is_none());
    }

    #[test]
    fn paths_are_stored_relative_with_forward_slashes_on_every_platform() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "scripts/deep/player.gd", "func go():\n\tpass\n");

        let mut index = Index::open_in_memory().unwrap();
        index.scan(dir.path()).unwrap();

        assert!(index.file("scripts/deep/player.gd").unwrap().is_some());
    }

    #[test]
    fn a_second_scan_with_no_edits_reparses_nothing() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "scripts/player.gd", "class_name Player\n\nfunc go():\n\tpass\n");

        let mut index = Index::open_in_memory().unwrap();
        index.scan(dir.path()).unwrap();
        let second = index.scan(dir.path()).unwrap();

        assert_eq!(second.indexed, 0);
        assert_eq!(second.unchanged, 1);
    }

    #[test]
    fn a_deleted_file_is_dropped_from_the_index_on_the_next_scan() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "scripts/player.gd", "class_name Player\n\nfunc go():\n\tpass\n");
        write(root, "scripts/enemy.gd", "class_name Enemy\n\nfunc hit():\n\tpass\n");

        let mut index = Index::open_in_memory().unwrap();
        index.scan(root).unwrap();
        fs::remove_file(root.join("scripts/enemy.gd")).unwrap();

        let report = index.scan(root).unwrap();
        assert_eq!(report.removed, 1);
        assert!(index.lookup("Enemy.hit", 5).unwrap().is_empty());
        assert_eq!(index.lookup("Player.go", 5).unwrap().len(), 1);
    }
}
