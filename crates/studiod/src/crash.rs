use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::Mutex;

const DEFAULT_REPO: &str = "tugcantopaloglu/game-studio-crew";
const REPO_ENV: &str = "STUDIO_CRASH_REPO";
const TAIL_LINES: usize = 20;
const MAX_LINE: usize = 200;
const MAX_URL_BODY: usize = 6000;

static RECENT: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn note(line: &str) {
    let line: String = line.trim_end().chars().take(MAX_LINE).collect();
    if let Ok(mut recent) = RECENT.lock() {
        recent.push(line);
        let overflow = recent.len().saturating_sub(TAIL_LINES);
        if overflow > 0 {
            recent.drain(..overflow);
        }
    }
}

pub fn install() {
    let subcommand = std::env::args().nth(1).unwrap_or_else(|| "help".into());
    note(&format!("studiod {subcommand} started"));

    let running = subcommand.clone();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);

        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "a panic with no message".into());
        let where_ = info
            .location()
            .map(|l| format!(" at {}:{}", l.file(), l.line()))
            .unwrap_or_default();

        let report = compose(
            &running,
            &os_build(),
            &format!("{}{where_}", first_line(&payload)),
            &std::backtrace::Backtrace::force_capture().to_string(),
            &tail(),
        );

        offer(&running, &report);
    }));
}

fn tail() -> Vec<String> {
    RECENT.lock().map(|r| r.clone()).unwrap_or_default()
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("").chars().take(MAX_LINE).collect()
}

fn compose(subcommand: &str, os: &str, panic: &str, backtrace: &str, tail: &[String]) -> String {
    let mut out = String::new();
    out.push_str(&format!("game studio crew {}\n", env!("CARGO_PKG_VERSION")));
    out.push_str(&format!("os: {os}\n"));
    out.push_str(&format!("subcommand: studiod {subcommand}\n"));
    out.push_str(&format!("when: {}\n", crate::now()));
    out.push_str(&format!("\npanic: {panic}\n"));
    out.push_str("\nbacktrace:\n");
    out.push_str(backtrace.trim_end());
    out.push_str("\n\ndaemon lifecycle (last recorded lines):\n");
    if tail.is_empty() {
        out.push_str("  nothing was recorded before the panic\n");
    } else {
        for line in tail {
            out.push_str(&format!("  {}\n", first_line(line)));
        }
    }
    redact(&out)
}

fn offer(subcommand: &str, report: &str) {
    let written = write_report(report);
    match &written {
        Some(path) => eprintln!("\ncrash report written to {}", path.display()),
        None => eprintln!("\ncould not write a crash report file"),
    }

    eprintln!("it names the panic, the build and the backtrace. It carries no file paths,");
    eprintln!("no project briefs and nothing a worker wrote.");

    if !std::io::stdin().is_terminal() {
        eprintln!("file it at https://github.com/{}/issues", repo());
        return;
    }

    eprint!("open a prefilled GitHub issue in your browser? [y/N] ");
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return;
    }
    if !answer.trim().eq_ignore_ascii_case("y") {
        eprintln!("not filed. The report stays on disk until you delete it.");
        return;
    }

    let url = issue_url(subcommand, report);
    if !open_in_browser(&url) {
        eprintln!("could not open a browser. File it here instead:");
        eprintln!("https://github.com/{}/issues/new", repo());
    }
}

fn write_report(report: &str) -> Option<PathBuf> {
    let stamp = crate::now().replace(':', "-");
    for dir in [crate::studio_dir().join("crashes"), std::env::temp_dir()] {
        if std::fs::create_dir_all(&dir).is_err() {
            continue;
        }
        let path = dir.join(format!("crash-{stamp}.txt"));
        if std::fs::write(&path, report).is_ok() {
            return Some(path);
        }
    }
    None
}

fn repo() -> String {
    std::env::var(REPO_ENV).unwrap_or_else(|_| DEFAULT_REPO.into())
}

fn issue_url(subcommand: &str, report: &str) -> String {
    let title = format!("studiod {subcommand} panicked");
    let body: String = format!("```\n{report}\n```\n")
        .chars()
        .take(MAX_URL_BODY)
        .collect();
    format!(
        "https://github.com/{}/issues/new?title={}&body={}",
        repo(),
        encode(&title),
        encode(&body)
    )
}

fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for byte in text.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn open_in_browser(url: &str) -> bool {
    let (program, args): (&str, Vec<&str>) = if cfg!(windows) {
        ("cmd", vec!["/C", "start", ""])
    } else if cfg!(target_os = "macos") {
        ("open", Vec::new())
    } else {
        ("xdg-open", Vec::new())
    };
    std::process::Command::new(program)
        .args(args)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}

fn os_build() -> String {
    let fallback = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
    let (program, args) = if cfg!(windows) {
        ("cmd", vec!["/C", "ver"])
    } else {
        ("uname", vec!["-sr"])
    };
    let probed = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()
        .and_then(|out| {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .map(|l| l.to_string())
        });
    match probed {
        Some(line) => format!("{fallback}, {line}"),
        None => fallback,
    }
}

fn redact(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        match absolute_path_at(&chars, i) {
            Some(len) => {
                let raw: String = chars[i..i + len].iter().collect();
                out.push_str(&shorten(&raw));
                i += len;
            }
            None => {
                out.push(chars[i]);
                i += 1;
            }
        }
    }
    scrub_user(&out)
}

fn absolute_path_at(chars: &[char], i: usize) -> Option<usize> {
    let starts_windows_drive = chars.get(i).is_some_and(|c| c.is_ascii_alphabetic())
        && chars.get(i + 1) == Some(&':')
        && matches!(chars.get(i + 2), Some('\\') | Some('/'))
        && (i == 0 || !chars[i - 1].is_ascii_alphanumeric());

    let rest: String = chars[i..].iter().take(16).collect();
    let starts_unix_root = ["/Users/", "/home/", "/root/", "/private/", "/tmp/", "/var/"]
        .iter()
        .any(|root| rest.starts_with(root));

    if !starts_windows_drive && !starts_unix_root {
        return None;
    }

    let mut len = 0;
    while let Some(c) = chars.get(i + len) {
        if c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '|' | '`' | '(' | ')') {
            break;
        }
        len += 1;
    }
    Some(len)
}

fn shorten(path: &str) -> String {
    let last = path.rsplit(['\\', '/']).next().unwrap_or("");
    if last.is_empty() {
        "<path>".into()
    } else {
        format!("<path>/{last}")
    }
}

fn scrub_user(text: &str) -> String {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    if home.is_empty() {
        return text.to_string();
    }
    let mut out = text.replace(&home, "<home>");
    if let Some(user) = home.rsplit(['\\', '/']).next() {
        if user.len() >= 3 {
            out = out.replace(user, "<user>");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_report_from_this_machine() -> String {
        let cwd = std::env::current_dir().unwrap().display().to_string();
        let backtrace = format!(
            "   0: studiod::studio::run_task\n             at {cwd}\\crates\\studiod\\src\\studio.rs:412\n   1: core::panicking::panic\n             at /rustc/8bab26f4f/library/core/src/panicking.rs:72\n"
        );
        compose(
            "studio",
            "windows x86_64, Microsoft Windows [Version 10.0.26200.1234]",
            "the worker outlived its watchdog at crates/studiod/src/studio.rs:412",
            &backtrace,
            &["studiod studio started".into(), "state store opened".into()],
        )
    }

    #[test]
    fn a_crash_report_carries_no_absolute_path_from_the_machine_that_produced_it() {
        let report = a_report_from_this_machine();
        let cwd = std::env::current_dir().unwrap().display().to_string();
        assert!(
            !report.contains(&cwd),
            "the working directory survived redaction:\n{report}"
        );
        let chars: Vec<char> = report.chars().collect();
        for i in 0..chars.len() {
            assert!(
                absolute_path_at(&chars, i).is_none(),
                "an absolute path survived redaction at char {i}:\n{report}"
            );
        }
    }

    #[test]
    fn redaction_keeps_the_part_of_a_backtrace_worth_reading() {
        let report = a_report_from_this_machine();
        assert!(report.contains("studiod::studio::run_task"));
        assert!(report.contains("<path>/studio.rs:412"), "{report}");
        assert!(report.contains("subcommand: studiod studio"));
    }

    #[test]
    fn the_users_name_never_reaches_a_report() {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_default();
        let user = home.rsplit(['\\', '/']).next().unwrap_or("").to_string();
        let report = compose(
            "studio",
            "windows",
            &format!("could not read {home}\\.studio\\studio-state.db"),
            "",
            &[],
        );
        assert!(!report.contains(&home));
        if user.len() >= 3 {
            assert!(!report.contains(&user), "{report}");
        }
    }

    #[test]
    fn a_recorded_line_is_capped_so_it_cannot_carry_a_wall_of_text() {
        let long = "x".repeat(4000);
        let report = compose("studio", "windows", "boom", "", &[long]);
        assert!(report.lines().all(|l| l.chars().count() <= MAX_LINE + 2));
    }

    #[test]
    fn the_issue_url_is_prefilled_and_points_at_a_configurable_repo() {
        let url = issue_url("studio", "panic: boom");
        assert!(url.starts_with(&format!("https://github.com/{DEFAULT_REPO}/issues/new?")));
        assert!(url.contains("title=studiod%20studio%20panicked"));
        assert!(url.contains("body="));
        assert!(!url.contains('\n'));
    }

    #[test]
    fn the_tail_holds_only_the_most_recent_lines() {
        for i in 0..TAIL_LINES * 2 {
            note(&format!("line {i}"));
        }
        let tail = tail();
        assert_eq!(tail.len(), TAIL_LINES);
        assert!(tail.last().unwrap().ends_with(&format!("{}", TAIL_LINES * 2 - 1)));
    }
}
