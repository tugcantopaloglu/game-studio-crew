use std::fs::File;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use studio_core::ProcessGroup;

pub const PORT: u16 = 7878;
const CONNECT_TIMEOUT: Duration = Duration::from_millis(250);
const FLOOR_TIMEOUT: Duration = Duration::from_secs(45);
const POLL: Duration = Duration::from_millis(100);
const NOTHING_TO_CODE_WITH: i32 = 2;
const LOG_TAIL: usize = 24;
const SILENCES_BEFORE_GIVING_UP: u32 = 6;
const NO_LOG_IN_REACH: &str = "there is no daemon log in reach; this window attached to a daemon \
                               that was already running, and its output went wherever it was started from";
const AN_EMPTY_LOG: &str = "the daemon wrote nothing before it stopped";

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
    silences: u32,
    exit: Option<ExitStatus>,
}

impl Daemon {
    fn new(child: Option<Child>, group: ProcessGroup, log: PathBuf) -> Self {
        Self {
            child,
            group,
            log,
            silences: 0,
            exit: None,
        }
    }

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
        if let Some(child) = self.child.as_mut() {
            if let Ok(Some(status)) = child.try_wait() {
                self.exit = Some(status);
                return true;
            }
            return false;
        }

        if floor_answers() {
            self.silences = 0;
            return false;
        }
        self.silences += 1;
        self.silences >= SILENCES_BEFORE_GIVING_UP
    }

    pub fn death_notice(&self) -> Failure {
        let mut detail = String::new();
        if let Some(status) = self.exit {
            detail.push_str(&format!("{}\n\n", how_it_ended(status)));
        }
        detail.push_str(&tail_of(&self.log));

        Failure::new(
            "the studio daemon stopped",
            detail,
            "Close this window and start it again. The last lines of daemon output are above; \
             the full log is in daemon.log inside your studio directory.",
        )
    }
}

fn how_it_ended(status: ExitStatus) -> String {
    match status.code() {
        Some(NOTHING_TO_CODE_WITH) => {
            "it exited because it found no coding CLI to spawn workers with".into()
        }
        Some(code) => format!("it exited with code {code}"),
        None => "it was killed before it could exit on its own".into(),
    }
}

pub fn floor_url() -> String {
    format!("http://127.0.0.1:{PORT}/")
}

pub fn bring_up(
    slot: &Mutex<Option<Daemon>>,
    complain: impl Fn(Failure) + Send + 'static,
) -> Result<(), Failure> {
    if floor_answers() {
        park(slot, attached()?);
        return Ok(());
    }

    let exe = locate_daemon()?;
    let home = studio_home()?;
    let mut group = supervision()?;

    if let Some(checking) = start_requirements_check(&exe, &home, &mut group) {
        std::thread::spawn(move || {
            if let Err(missing) = read_requirements(checking) {
                complain(missing);
            }
        });
    }

    let daemon = spawn(&exe, &home, group)?;
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
    Ok(Daemon::new(None, supervision()?, log_in_the_studio_home()))
}

fn log_in_the_studio_home() -> PathBuf {
    studio_home()
        .map(|home| home.join(".studio").join("daemon.log"))
        .unwrap_or_default()
}

fn supervision() -> Result<ProcessGroup, Failure> {
    ProcessGroup::new().map_err(|e| {
        Failure::new(
            "could not set up process supervision",
            e.to_string(),
            "Without it, closing the window would leave the daemon and its workers running.",
        )
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

fn start_requirements_check(exe: &Path, home: &Path, group: &mut ProcessGroup) -> Option<Child> {
    let mut cmd = Command::new(exe);
    cmd.arg("doctor")
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    group.prepare(&mut cmd);

    let child = cmd.spawn().ok()?;
    group.adopt(&child).ok()?;
    Some(child)
}

fn read_requirements(checking: Child) -> Result<(), Failure> {
    let checked = match checking.wait_with_output() {
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

fn spawn(exe: &Path, home: &Path, mut group: ProcessGroup) -> Result<Daemon, Failure> {
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

    Ok(Daemon::new(Some(child), group, log))
}

fn tail_of(log: &Path) -> String {
    if log.as_os_str().is_empty() {
        return NO_LOG_IN_REACH.into();
    }
    let text = match std::fs::read_to_string(log) {
        Ok(text) => text,
        Err(e) => return format!("{} could not be read: {e}", log.display()),
    };
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return AN_EMPTY_LOG.into();
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
    fn a_log_that_was_never_written_names_the_file_it_looked_for() {
        let notice = tail_of(Path::new("no-such-daemon.log"));
        assert!(notice.contains("no-such-daemon.log"), "{notice}");
        assert!(
            !notice.contains("wrote nothing"),
            "a log that is not there is a different fact from a daemon that said nothing: {notice}"
        );
    }

    #[test]
    fn a_log_that_exists_but_is_blank_is_the_one_case_that_says_nothing_was_written() {
        let dir = std::env::temp_dir().join("game-studio-shell-blank-log");
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("daemon.log");
        std::fs::write(&log, "\n  \n").unwrap();
        assert_eq!(tail_of(&log), AN_EMPTY_LOG);
    }

    #[test]
    fn a_daemon_we_attached_to_still_reads_the_log_the_running_one_writes() {
        let attached = attached().unwrap();
        let expected = log_in_the_studio_home();
        assert_eq!(attached.log, expected);
        assert!(
            !expected.as_os_str().is_empty(),
            "this machine has a studio home, so the notice must never fall back to a blank path"
        );
        assert!(
            !attached.death_notice().detail.contains(NO_LOG_IN_REACH),
            "attaching to a running daemon used to throw its log away and report nothing at all"
        );
    }

    #[test]
    fn one_missed_connection_is_not_a_dead_daemon() {
        let mut attached = attached().unwrap();
        let answers = floor_answers();
        for _ in 1..SILENCES_BEFORE_GIVING_UP {
            assert!(
                !attached.stopped(),
                "a single refused connect is a busy port, not a stopped daemon"
            );
        }
        assert_eq!(
            attached.stopped(),
            !answers,
            "after {SILENCES_BEFORE_GIVING_UP} silences in a row the daemon really is gone"
        );
    }

    #[test]
    fn a_notice_says_how_the_daemon_ended_when_the_shell_owned_it() {
        assert!(how_it_ended(exit_status(NOTHING_TO_CODE_WITH)).contains("no coding CLI"));
        assert!(how_it_ended(exit_status(1)).contains("code 1"));
    }

    #[cfg(windows)]
    fn exit_status(code: i32) -> ExitStatus {
        use std::os::windows::process::ExitStatusExt;
        ExitStatus::from_raw(code as u32)
    }

    #[cfg(not(windows))]
    fn exit_status(code: i32) -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(code << 8)
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
