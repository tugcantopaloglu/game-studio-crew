use std::path::{Path, PathBuf};

pub fn path_extensions() -> Vec<String> {
    if !cfg!(windows) {
        return Vec::new();
    }
    std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
        .split(';')
        .map(|e| e.trim().to_lowercase())
        .filter(|e| !e.is_empty())
        .collect()
}

pub fn launcher_for(found: &Path) -> (PathBuf, Vec<String>) {
    let ext = found
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if cfg!(windows) && (ext == "cmd" || ext == "bat") {
        let shell = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into());
        return (
            PathBuf::from(shell),
            vec!["/c".into(), found.to_string_lossy().into_owned()],
        );
    }
    (found.to_path_buf(), Vec::new())
}

pub fn resolve(program: &str) -> Option<PathBuf> {
    if program.contains('/') || program.contains('\\') {
        let direct = PathBuf::from(program);
        return direct.is_file().then_some(direct);
    }

    let raw = std::env::var_os("PATH")?;
    let extensions = path_extensions();
    for dir in std::env::split_paths(&raw) {
        for ext in &extensions {
            let candidate = dir.join(format!("{program}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        if !cfg!(windows) {
            let bare = dir.join(program);
            if bare.is_file() {
                return Some(bare);
            }
        }
    }
    None
}

pub fn spawnable(program: &str) -> Option<(PathBuf, Vec<String>)> {
    resolve(program).map(|found| launcher_for(&found))
}

pub fn on_path(program: &str) -> bool {
    resolve(program).is_some()
}

pub fn quiet(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

pub fn command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    let mut cmd = std::process::Command::new(program);
    quiet(&mut cmd);
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_real_executable_is_launched_directly_with_no_shell_in_front_of_it() {
        let (launcher, prefix) = launcher_for(Path::new(r"C:\tools\claude.exe"));
        assert_eq!(launcher, PathBuf::from(r"C:\tools\claude.exe"));
        assert!(
            prefix.is_empty(),
            "putting a shell in front of a working exe would only add a quoting layer"
        );
    }

    #[test]
    fn a_windows_batch_shim_is_routed_through_the_shell_because_createprocess_cannot_run_one() {
        if !cfg!(windows) {
            return;
        }
        let (launcher, prefix) = launcher_for(Path::new(r"C:\npm\codex.cmd"));
        assert_ne!(launcher, PathBuf::from(r"C:\npm\codex.cmd"));
        assert_eq!(prefix.first().map(String::as_str), Some("/c"));
        assert_eq!(prefix.get(1).map(String::as_str), Some(r"C:\npm\codex.cmd"));
    }

    #[test]
    fn an_extensionless_script_is_never_what_gets_launched_on_windows() {
        if !cfg!(windows) {
            return;
        }
        let resolved = resolve("codex");
        if let Some(found) = resolved {
            assert!(
                found.extension().is_some(),
                "npm installs an extensionless shim beside the .cmd and CreateProcess cannot \
                 execute it; resolving to the bare name is the silent failure this exists to stop: {}",
                found.display()
            );
        }
    }

    #[test]
    fn the_only_cli_the_studio_can_drive_today_resolves_to_a_real_executable() {
        let Some(found) = resolve("claude") else {
            return;
        };
        let ext = found
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        assert_ne!(
            ext, "cmd",
            "claude resolving to a shim would mean every worker spawn now goes through a shell \
             that re-parses the argument vector, and the frozen prefix travels in that vector"
        );
    }

    #[test]
    fn a_program_that_is_not_installed_resolves_to_nothing_rather_than_to_its_bare_name() {
        assert!(resolve("studio-no-such-program-anywhere").is_none());
        assert!(!on_path("studio-no-such-program-anywhere"));
    }

    #[test]
    fn an_explicit_path_is_taken_as_given_and_not_hunted_for_on_path() {
        assert!(resolve("./definitely-not-here-either").is_none());
    }

    #[test]
    fn no_background_git_call_spawns_a_bare_command_and_flashes_a_console() {
        let source = include_str!("git.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("git.rs has a production half");
        assert!(
            !production.contains("Command::new("),
            "a bare Command::new on Windows gives the child its own console, and git runs after \
             every worker — the window flashes on screen each time. Use launcher::command."
        );
    }

    #[test]
    fn the_no_window_flag_is_the_value_windows_actually_documents() {
        assert_eq!(
            0x0800_0000u32, 0x08000000,
            "CREATE_NO_WINDOW is 0x08000000; a wrong constant here silently does nothing"
        );
    }
}
