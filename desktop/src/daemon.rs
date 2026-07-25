use std::fs::File;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use studio_core::ProcessGroup;

pub const PORT: u16 = 7878;
const CONNECT_TIMEOUT: Duration = Duration::from_millis(250);
const FLOOR_TIMEOUT: Duration = Duration::from_secs(45);
const POLL: Duration = Duration::from_millis(100);
const NOTHING_TO_CODE_WITH: i32 = 2;
const LOG_TAIL: usize = 24;

#[derive(Debug)]
pub struct Failure {
    pub headline: String,
    pub detail: String,
    pub what_to_do: String,
}

impl Failure {
    fn new(headline: &str, detail: String, what_to_do: &str) -> Self {
        Self {
            headline: headline.into(),
            detail,
            what_to_do: what_to_do.into(),
        }
    }
}

pub struct Daemon {
    child: Option<Child>,
    group: ProcessGroup,
    log: PathBuf,
}

impl Daemon {
    pub fn shutdown(&mut self) {
        if self.child.is_none() {
            return;
        }
        let _ = self.group.kill_tree();
        if let Some(mut child) = self.child.take() {
            let _ = child.wait();
        }
    }

    pub fn stopped(&mut self) -> bool {
        match self.child.as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(Some(_))),
            None => !floor_answers(),
        }
    }

    pub fn death_notice(&self) -> Failure {
        Failure::new(
            "the studio daemon stopped",
            tail_of(&self.log),
            "Close this window and start it again. The last lines of daemon output are above; \
             the full log is in daemon.log inside your studio directory.",
        )
    }
}

pub fn floor_url() -> String {
    format!("http://127.0.0.1:{PORT}/")
}

pub fn bring_up(slot: &Mutex<Option<Daemon>>) -> Result<(), Failure> {
    if floor_answers() {
        park(slot, attached()?);
        return Ok(());
    }

    let exe = locate_daemon()?;
    let home = studio_home()?;
    check_requirements(&exe, &home)?;

    let daemon = spawn(&exe, &home)?;
    let log = daemon.log.clone();
    park(slot, daemon);
    wait_for_the_floor(slot, &log)
}

fn park(slot: &Mutex<Option<Daemon>>, daemon: Daemon) {
    if let Ok(mut held) = slot.lock() {
        *held = Some(daemon);
    }
}

fn attached() -> Result<Daemon, Failure> {
    Ok(Daemon {
        child: None,
        group: ProcessGroup::new()
            .map_err(|e| Failure::new("could not set up process supervision", e.to_string(), ""))?,
        log: PathBuf::new(),
    })
}

fn wait_for_the_floor(slot: &Mutex<Option<Daemon>>, log: &Path) -> Result<(), Failure> {
    let deadline = Instant::now() + FLOOR_TIMEOUT;
    loop {
        if floor_answers() {
            return Ok(());
        }
        let stopped = slot
            .lock()
            .ok()
            .and_then(|mut held| held.as_mut().map(|d| d.stopped()))
            .unwrap_or(true);
        if stopped {
            return Err(Failure::new(
                "the studio daemon exited before it served the floor",
                tail_of(log),
                "Run studiod doctor in a terminal to see what it is missing.",
            ));
        }
        if Instant::now() >= deadline {
            return Err(Failure::new(
                "the studio daemon never answered on its port",
                tail_of(log),
                &format!(
                    "Nothing was listening on 127.0.0.1:{PORT} after {} seconds. \
                     Check whether another program holds that port.",
                    FLOOR_TIMEOUT.as_secs()
                ),
            ));
        }
        std::thread::sleep(POLL);
    }
}

pub fn floor_answers() -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], PORT));
    TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).is_ok()
}

fn locate_daemon() -> Result<PathBuf, Failure> {
    let name = if cfg!(windows) { "studiod.exe" } else { "studiod" };

    if let Ok(shell) = std::env::current_exe() {
        if let Some(beside) = shell.parent().map(|dir| dir.join(name)) {
            if beside.is_file() {
                return Ok(beside);
            }
        }
    }

    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    Err(Failure::new(
        "the studio daemon is missing",
        format!("{name} was not next to this app and not on PATH."),
        "Reinstall Game Studio Crew, or build the daemon with cargo build --release -p studiod.",
    ))
}

fn studio_home() -> Result<PathBuf, Failure> {
    let base = std::env::var_os("STUDIO_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("GameStudioCrew"))
        })
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".game-studio-crew")))
        .ok_or_else(|| {
            Failure::new(
                "there is nowhere to keep the studio",
                "Neither STUDIO_HOME nor LOCALAPPDATA nor HOME is set.".into(),
                "Set STUDIO_HOME to a directory the studio may write to.",
            )
        })?;

    std::fs::create_dir_all(base.join(".studio")).map_err(|e| {
        Failure::new(
            "the studio directory could not be created",
            format!("{}: {e}", base.display()),
            "Set STUDIO_HOME to a directory you can write to.",
        )
    })?;
    Ok(base)
}

fn check_requirements(exe: &Path, home: &Path) -> Result<(), Failure> {
    let checked = Command::new(exe)
        .arg("doctor")
        .current_dir(home)
        .stdin(Stdio::null())
        .output();

    let checked = match checked {
        Ok(out) => out,
        Err(_) => return Ok(()),
    };

    if checked.status.code() != Some(NOTHING_TO_CODE_WITH) {
        return Ok(());
    }

    Err(Failure::new(
        "there is nothing to code with",
        String::from_utf8_lossy(&checked.stdout).into_owned(),
        "Install one coding CLI, put it on PATH, and start the app again. \
         Everything else the doctor lists is optional.",
    ))
}

fn spawn(exe: &Path, home: &Path) -> Result<Daemon, Failure> {
    let log = home.join(".studio").join("daemon.log");
    let out = File::create(&log).map_err(|e| {
        Failure::new(
            "the daemon log could not be opened",
            format!("{}: {e}", log.display()),
            "Set STUDIO_HOME to a directory you can write to.",
        )
    })?;
    let errors = out.try_clone().map_err(|e| {
        Failure::new("the daemon log could not be opened", e.to_string(), "")
    })?;

    let mut group = ProcessGroup::new().map_err(|e| {
        Failure::new(
            "could not set up process supervision",
            e.to_string(),
            "Without it, closing the window would leave workers running.",
        )
    })?;

    let mut cmd = Command::new(exe);
    cmd.arg("studio")
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(errors));
    group.prepare(&mut cmd);

    let child = cmd.spawn().map_err(|e| {
        Failure::new(
            "the studio daemon would not start",
            format!("{}: {e}", exe.display()),
            "Reinstall Game Studio Crew.",
        )
    })?;

    group.adopt(&child).map_err(|e| {
        Failure::new(
            "the daemon could not be supervised",
            e.to_string(),
            "Closing the window would leave workers running, so the app stops here.",
        )
    })?;

    Ok(Daemon {
        child: Some(child),
        group,
        log,
    })
}

fn tail_of(log: &Path) -> String {
    let text = match std::fs::read_to_string(log) {
        Ok(text) => text,
        Err(_) => return "the daemon wrote nothing before it stopped".into(),
    };
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return "the daemon wrote nothing before it stopped".into();
    }
    lines[lines.len().saturating_sub(LOG_TAIL)..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_floor_url_is_always_loopback() {
        assert_eq!(floor_url(), "http://127.0.0.1:7878/");
    }

    #[test]
    fn a_log_that_was_never_written_still_produces_a_readable_notice() {
        assert!(tail_of(Path::new("no-such-daemon.log")).contains("wrote nothing"));
    }

    #[test]
    fn only_the_last_lines_of_a_long_log_reach_the_window() {
        let dir = std::env::temp_dir().join("game-studio-shell-tail-test");
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("daemon.log");
        let body: String = (0..200).map(|i| format!("line {i}\n")).collect();
        std::fs::write(&log, body).unwrap();

        let tail = tail_of(&log);
        assert_eq!(tail.lines().count(), LOG_TAIL);
        assert!(tail.ends_with("line 199"));
        assert!(!tail.contains("line 0\n"));
    }

    #[test]
    fn a_daemon_we_only_attached_to_is_never_killed_on_close() {
        let mut attached = attached().unwrap();
        attached.shutdown();
        assert!(attached.child.is_none());
    }
}
