use crate::parsers::{parse_report, scan_log_expecting};
use crate::{FailureKind, VerifyResult, Verdict};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use studio_engine::{render_command, resolve_binary, EngineProfile, Substitutions, VerifyScope};

pub const DEFAULT_VERIFY_TIMEOUT: Duration = Duration::from_secs(900);

struct RunOutput {
    log: String,
    exit_code: Option<i32>,
    timed_out: bool,
}

fn is_crash_code(code: i32) -> bool {
    code as u32 >= 0xC000_0000 || code == 139 || code == 134 || code == 137
}

const READER_GRACE: Duration = Duration::from_secs(2);

type Sink = std::sync::Arc<std::sync::Mutex<Vec<u8>>>;

fn drain(
    mut source: impl Read + Send + 'static,
    sink: Sink,
    done: std::sync::mpsc::Sender<()>,
) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match source.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => sink
                    .lock()
                    .unwrap_or_else(|held| held.into_inner())
                    .extend_from_slice(&buf[..n]),
            }
        }
        let _ = done.send(());
    });
}

fn collected(sink: &Sink) -> String {
    let bytes = sink.lock().unwrap_or_else(|held| held.into_inner());
    String::from_utf8_lossy(&bytes).into_owned()
}

fn run_command(args: &[String], cwd: &Path, timeout: Duration) -> std::io::Result<RunOutput> {
    let mut group = studio_core::ProcessGroup::new()?;
    let mut cmd = studio_core::command(&args[0]);
    cmd.args(&args[1..])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    group.prepare(&mut cmd);

    let mut child = cmd.spawn()?;
    group.adopt(&child)?;

    let out_sink: Sink = Default::default();
    let err_sink: Sink = Default::default();
    let (done, readers_done) = std::sync::mpsc::channel();

    if let Some(r) = child.stdout.take() {
        drain(r, out_sink.clone(), done.clone());
    }
    if let Some(r) = child.stderr.take() {
        drain(r, err_sink.clone(), done.clone());
    }
    drop(done);

    let started = Instant::now();
    let mut timed_out = false;
    let mut status = None;
    loop {
        match child.try_wait()? {
            Some(s) => {
                status = Some(s);
                break;
            }
            None => {
                if started.elapsed() >= timeout {
                    let _ = group.kill_tree();
                    let _ = child.kill();
                    let _ = child.wait();
                    timed_out = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }

    let grace = Instant::now();
    while readers_done.recv_timeout(READER_GRACE.saturating_sub(grace.elapsed())).is_ok() {}

    let mut log = collected(&out_sink);
    log.push_str(&collected(&err_sink));

    Ok(RunOutput { log, exit_code: status.and_then(|s| s.code()), timed_out })
}

pub fn invokes_studio_helper(template: &str) -> bool {
    template.contains("studio_ci")
}

const SCRIPT_SUFFIXES: [&str; 4] = [".gd", ".mjs", ".py", ".js"];

fn missing_script(args: &[String], project: &Path) -> Option<String> {
    args.iter().skip(1).find_map(|arg| {
        let looks_like_a_script = SCRIPT_SUFFIXES
            .iter()
            .any(|suffix| arg.to_lowercase().ends_with(suffix));
        if !looks_like_a_script {
            return None;
        }
        let at = Path::new(arg);
        let resolved = if at.is_absolute() { at.to_path_buf() } else { project.join(at) };
        (!resolved.exists()).then(|| arg.clone())
    })
}

fn first_export_preset(project: &Path) -> Option<String> {
    let body = std::fs::read_to_string(project.join("export_presets.cfg")).ok()?;
    body.lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix("name="))
        .map(|name| name.trim().trim_matches('"').to_string())
        .find(|name| !name.is_empty())
}

fn single_file_with(dir: &Path, extension: &str) -> Option<PathBuf> {
    let mut found = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case(extension)));
    let first = found.next()?;
    match found.next() {
        Some(_) => None,
        None => Some(first),
    }
}

fn ue_root(engine_binary: &Path) -> Option<String> {
    let mut at = engine_binary.parent()?;
    loop {
        if at.file_name().is_some_and(|n| n == "Engine") {
            return at.parent().map(absolute);
        }
        at = at.parent()?;
    }
}

fn unity_build_target() -> &'static str {
    if cfg!(target_os = "windows") {
        "Win64"
    } else if cfg!(target_os = "macos") {
        "OSXUniversal"
    } else {
        "Linux64"
    }
}

fn ue_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "Win64"
    } else if cfg!(target_os = "macos") {
        "Mac"
    } else {
        "Linux"
    }
}

#[derive(Debug, Clone)]
pub struct ProjectPaths {
    pub project: PathBuf,
    pub out: PathBuf,
}

impl ProjectPaths {
    pub fn new(project: impl Into<PathBuf>, out: impl Into<PathBuf>) -> Self {
        Self { project: project.into(), out: out.into() }
    }
}

pub trait EngineDriver {
    fn verify(&self, scope: VerifyScope, paths: &ProjectPaths) -> VerifyResult;
}

pub struct ProfileDriver {
    pub profile: EngineProfile,
    pub engine_binary: PathBuf,
    pub extra: Vec<(String, String)>,
    pub timeout: Duration,
}

impl ProfileDriver {
    pub fn resolve(profile: EngineProfile) -> Result<Self, studio_engine::EngineError> {
        let engine_binary = resolve_binary(&profile)?;
        Ok(Self {
            profile,
            engine_binary,
            extra: Vec::new(),
            timeout: DEFAULT_VERIFY_TIMEOUT,
        })
    }

    fn substitutions(&self, paths: &ProjectPaths) -> Substitutions {
        let mut s = Substitutions::new()
            .set("engine", self.engine_binary.to_string_lossy())
            .set("project", absolute(&paths.project))
            .set("out", absolute(&paths.out));
        for (k, v) in self.derived(paths) {
            s = s.set(k, v);
        }
        for (k, v) in &self.extra {
            s = s.set(k, v.clone());
        }
        s
    }

    fn derived(&self, paths: &ProjectPaths) -> Vec<(&'static str, String)> {
        let mut bound: Vec<(&'static str, String)> = Vec::new();

        match self.profile.id.as_str() {
            "godot" => {
                if let Some(preset) = first_export_preset(&paths.project) {
                    bound.push(("preset", preset));
                }
            }
            "unity" => {
                bound.push(("platform", unity_build_target().into()));
            }
            "ue5" => {
                bound.push(("platform", ue_platform().into()));
                if let Some(root) = ue_root(&self.engine_binary) {
                    bound.push(("ue_root", root));
                }
                if let Some(uproject) = single_file_with(&paths.project, "uproject") {
                    if let Some(stem) = uproject.file_stem().map(|s| s.to_string_lossy().into_owned())
                    {
                        bound.push(("target", stem.clone()));
                        bound.push(("suite", stem));
                    }
                    bound.push(("uproject", absolute(&uproject)));
                }
            }
            _ => {}
        }

        bound
    }

    fn failure_kind(scope: VerifyScope) -> FailureKind {
        match scope {
            VerifyScope::Compile => FailureKind::Compile,
            VerifyScope::TestFast | VerifyScope::TestFull => FailureKind::Test,
            VerifyScope::Import => FailureKind::Import,
            VerifyScope::Export => FailureKind::Export,
            VerifyScope::Runtime => FailureKind::Crash,
        }
    }

    fn inconclusive(&self, scope: VerifyScope, reason: String, started: Instant) -> VerifyResult {
        VerifyResult {
            verdict: Verdict::Inconclusive,
            failures: Vec::new(),
            scope,
            engine: self.profile.id.clone(),
            duration_ms: started.elapsed().as_millis() as u64,
            raw_report_path: None,
            inconclusive_reason: Some(reason),
        }
    }
}

impl EngineDriver for ProfileDriver {
    fn verify(&self, scope: VerifyScope, paths: &ProjectPaths) -> VerifyResult {
        let started = Instant::now();

        if let Err(e) = std::fs::create_dir_all(&paths.out) {
            return self.inconclusive(scope, format!("could not create the out directory: {e}"), started);
        }

        let template = match self.profile.command(scope) {
            Ok(t) => t,
            Err(e) => return self.inconclusive(scope, e.to_string(), started),
        };

        let args = match render_command(template, &self.substitutions(paths)) {
            Ok(a) => a,
            Err(e) => return self.inconclusive(scope, e.to_string(), started),
        };
        if args.is_empty() {
            return self.inconclusive(scope, "empty command line".into(), started);
        }
        if let Some(missing) = missing_script(&args, &paths.project) {
            return self.inconclusive(
                scope,
                format!(
                    "{} runs {missing}, which is not in the project; install it or drop the \
                     {} scope from the {} profile rather than reporting a scope that cannot run",
                    self.profile.id,
                    scope.key(),
                    self.profile.id
                ),
                started,
            );
        }

        let report_spec = self.profile.report(scope);
        let report_path = match report_spec {
            Some(spec) => match self.substitutions(paths).apply(&spec.path) {
                Ok(p) => Some(PathBuf::from(p)),
                Err(e) => return self.inconclusive(scope, e.to_string(), started),
            },
            None => None,
        };

        if let Some(path) = report_path.as_ref() {
            if path.exists() {
                if let Err(e) = std::fs::remove_file(path) {
                    return self.inconclusive(
                        scope,
                        format!(
                            "could not clear the previous report at {}: {e}",
                            path.display()
                        ),
                        started,
                    );
                }
            }
        }

        let run = match run_command(&args, &paths.project, self.timeout) {
            Ok(r) => r,
            Err(e) => {
                return self.inconclusive(scope, format!("could not run the engine: {e}"), started)
            }
        };

        if run.timed_out {
            return self.inconclusive(
                scope,
                format!(
                    "the engine did not exit within {}s and was killed",
                    self.timeout.as_secs()
                ),
                started,
            );
        }

        let log = run.log;
        let exit_code = run.exit_code;

        if let Some(code) = exit_code {
            if is_crash_code(code) {
                return self.inconclusive(
                    scope,
                    format!("the engine crashed with exit code {code}"),
                    started,
                );
            }
        }

        let parsed = match report_spec {
            Some(spec) => {
                let path = match report_path.as_ref() {
                    Some(p) => p,
                    None => return self.inconclusive(scope, "no report path".into(), started),
                };

                if !path.exists() {
                    if let Some(reason) = crate::looks_like_infrastructure(&log) {
                        return self.inconclusive(scope, reason, started);
                    }
                    return self.inconclusive(
                        scope,
                        format!(
                            "the command produced no report at {}; exit code was {:?}",
                            path.display(),
                            exit_code
                        ),
                        started,
                    );
                }

                match std::fs::read_to_string(path) {
                    Ok(body) => parse_report(&spec.format, &body),
                    Err(e) => {
                        return self.inconclusive(scope, format!("report unreadable: {e}"), started)
                    }
                }
            }
            None => scan_log_expecting(
                exit_code,
                &log,
                Self::failure_kind(scope),
                invokes_studio_helper(template),
            ),
        };

        let raw_report_path = report_path.map(|p| p.to_string_lossy().into_owned());

        VerifyResult {
            verdict: parsed.verdict,
            failures: parsed.failures,
            scope,
            engine: self.profile.id.clone(),
            duration_ms: started.elapsed().as_millis() as u64,
            raw_report_path,
            inconclusive_reason: parsed.inconclusive_reason,
        }
    }
}

pub fn write_report_for_test(out: &Path, name: &str, body: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(out)?;
    std::fs::write(out.join(name), body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_profile(command: &str, report: Option<(&str, &str)>) -> EngineProfile {
        let reports = match report {
            Some((format, path)) => format!(
                "[reports]\ncompile = {{ format = \"{format}\", path = \"{path}\" }}\n"
            ),
            None => String::new(),
        };
        let src = format!(
            r#"
schema_version = 1
id = "fake"
display_name = "Fake"

[detect]
markers = []
precedence = 1

[tooling]
resolver = "path"
binary_env = "FAKE_ENGINE"
binary_names = ["node"]

[commands]
compile   = "{command}"
test_fast = "{command}"
test_full = "{command}"
import    = "{command}"
export    = "{command}"

{reports}

[prose]
profile = "fake engine prose"
"#
        );
        EngineProfile::parse(&src).unwrap()
    }

    fn driver(profile: EngineProfile) -> ProfileDriver {
        ProfileDriver {
            profile,
            engine_binary: PathBuf::from("node"),
            extra: Vec::new(),
            timeout: DEFAULT_VERIFY_TIMEOUT,
        }
    }

    #[test]
    fn a_clean_command_with_no_report_passes_on_exit_zero() {
        let dir = tempfile::tempdir().unwrap();
        let d = driver(fake_profile("{engine} -e console.log('ok')", None));
        let paths = ProjectPaths::new(dir.path(), dir.path().join("out"));
        let r = d.verify(VerifyScope::Compile, &paths);
        assert_eq!(r.verdict, Verdict::Pass, "{:?}", r.inconclusive_reason);
    }

    #[test]
    fn a_nonzero_exit_with_no_report_fails() {
        let dir = tempfile::tempdir().unwrap();
        let d = driver(fake_profile("{engine} -e process.exit(3)", None));
        let paths = ProjectPaths::new(dir.path(), dir.path().join("out"));
        let r = d.verify(VerifyScope::Compile, &paths);
        assert_eq!(r.verdict, Verdict::Fail);
    }

    #[test]
    fn a_missing_report_is_inconclusive_not_a_pass() {
        let dir = tempfile::tempdir().unwrap();
        let d = driver(fake_profile(
            "{engine} -e console.log('done')",
            Some(("junit", "{out}/never-written.xml")),
        ));
        let paths = ProjectPaths::new(dir.path(), dir.path().join("out"));
        let r = d.verify(VerifyScope::Compile, &paths);
        assert_eq!(
            r.verdict,
            Verdict::Inconclusive,
            "a command that claims success but writes no report has not verified anything"
        );
        assert!(r.inconclusive_reason.unwrap().contains("no report"));
    }

    const WRITES_ONE_PASSING_CASE: &str = "{engine} -e require('fs').writeFileSync('{out}/r.xml','<testsuites><testsuite><testcase/></testsuite></testsuites>')";

    #[test]
    fn a_report_that_exists_is_parsed() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        let d = driver(fake_profile(WRITES_ONE_PASSING_CASE, Some(("junit", "{out}/r.xml"))));
        let paths = ProjectPaths::new(dir.path(), &out);
        let r = d.verify(VerifyScope::Compile, &paths);
        assert_eq!(r.verdict, Verdict::Pass, "{:?}", r.inconclusive_reason);
        assert!(r.raw_report_path.is_some());
    }

    #[test]
    fn a_stale_report_from_an_earlier_round_is_not_a_pass() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(
            out.join("r.xml"),
            "<testsuites><testsuite><testcase/></testsuite></testsuites>",
        )
        .unwrap();

        let d = driver(fake_profile(
            "{engine} -e process.exit(1)",
            Some(("junit", "{out}/r.xml")),
        ));
        let paths = ProjectPaths::new(dir.path(), &out);
        let r = d.verify(VerifyScope::Compile, &paths);
        assert_eq!(
            r.verdict,
            Verdict::Inconclusive,
            "a crashed run must not inherit the previous round's green report"
        );
    }

    #[test]
    fn a_report_with_no_test_cases_is_not_a_pass() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        let d = driver(fake_profile(
            "{engine} -e require('fs').writeFileSync('{out}/r.xml','<testsuites><testsuite/></testsuites>')",
            Some(("junit", "{out}/r.xml")),
        ));
        let paths = ProjectPaths::new(dir.path(), &out);
        let r = d.verify(VerifyScope::Compile, &paths);
        assert_eq!(
            r.verdict,
            Verdict::Inconclusive,
            "an empty suite means the tests never ran, not that they passed"
        );
    }

    #[test]
    fn an_engine_that_never_exits_is_killed_and_inconclusive() {
        let dir = tempfile::tempdir().unwrap();
        let mut d = driver(fake_profile("{engine} -e setInterval(Boolean,1000)", None));
        d.timeout = Duration::from_millis(400);
        let paths = ProjectPaths::new(dir.path(), dir.path().join("out"));
        let r = d.verify(VerifyScope::Compile, &paths);
        assert_eq!(r.verdict, Verdict::Inconclusive);
        assert!(r.inconclusive_reason.unwrap().contains("did not exit"));
    }

    #[test]
    fn a_timeout_returns_even_when_a_grandchild_still_holds_the_log_pipe() {
        let dir = tempfile::tempdir().unwrap();
        let spawner = "{engine} -e require('child_process').spawn(process.execPath,['-e','setInterval(Boolean,1000)'],{stdio:'inherit'});setInterval(Boolean,1000)";
        let mut d = driver(fake_profile(spawner, None));
        d.timeout = Duration::from_millis(400);
        let paths = ProjectPaths::new(dir.path(), dir.path().join("out"));

        let started = Instant::now();
        let r = d.verify(VerifyScope::Compile, &paths);

        assert_eq!(r.verdict, Verdict::Inconclusive);
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the timeout governs the wait loop, not the function: a grandchild inheriting the \
             stdout handle must not hold verify() open past its own deadline"
        );
    }

    #[test]
    fn godot_binds_its_export_preset_from_the_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("export_presets.cfg"),
            "[preset.0]\n\nname=\"Windows Desktop\"\nplatform=\"Windows Desktop\"\n",
        )
        .unwrap();

        let mut profile = EngineProfile::parse(studio_engine::GODOT_PROFILE).unwrap();
        profile.tooling.binary_names = vec!["node".into()];
        let d = ProfileDriver {
            profile,
            engine_binary: PathBuf::from("node"),
            extra: Vec::new(),
            timeout: Duration::from_secs(1),
        };

        let paths = ProjectPaths::new(dir.path(), dir.path().join("out"));
        let subs = d.substitutions(&paths);
        assert_eq!(
            subs.apply("{preset}").unwrap(),
            "Windows Desktop",
            "an unbound preset placeholder makes the export scope inconclusive on every run, forever"
        );
    }

    #[test]
    fn a_scope_whose_script_is_not_installed_says_so_instead_of_failing_vaguely() {
        let dir = tempfile::tempdir().unwrap();
        let d = driver(fake_profile("{engine} -s addons/gut/gut_cmdln.gd", None));
        let paths = ProjectPaths::new(dir.path(), dir.path().join("out"));

        let r = d.verify(VerifyScope::Compile, &paths);

        assert_eq!(r.verdict, Verdict::Inconclusive);
        let reason = r.inconclusive_reason.unwrap();
        assert!(
            reason.contains("gut_cmdln.gd"),
            "a scope that can never run should name what is missing, got: {reason}"
        );
    }

    #[test]
    fn a_path_with_spaces_stays_one_argument() {
        let subs = Substitutions::new().set("project", "C:/Program Files/My Game");
        let args = render_command("{engine} --path {project}", &subs.clone().set("engine", "godot"))
            .unwrap();
        assert_eq!(args, vec!["godot", "--path", "C:/Program Files/My Game"]);
    }

    #[test]
    fn an_engine_that_cannot_be_run_is_inconclusive() {
        let dir = tempfile::tempdir().unwrap();
        let mut d = driver(fake_profile("{engine} -e ''", None));
        d.engine_binary = PathBuf::from("studio-no-such-binary-anywhere");
        let paths = ProjectPaths::new(dir.path(), dir.path().join("out"));
        let r = d.verify(VerifyScope::Compile, &paths);
        assert_eq!(r.verdict, Verdict::Inconclusive);
    }

    #[test]
    fn an_unbound_placeholder_is_inconclusive_rather_than_a_bad_command() {
        let dir = tempfile::tempdir().unwrap();
        let d = driver(fake_profile("{engine} --target {unbound_thing}", None));
        let paths = ProjectPaths::new(dir.path(), dir.path().join("out"));
        let r = d.verify(VerifyScope::Compile, &paths);
        assert_eq!(r.verdict, Verdict::Inconclusive);
        assert!(r.inconclusive_reason.unwrap().contains("unbound_thing"));
    }

    #[test]
    fn the_out_directory_is_created_before_the_command_runs() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("nested/out/dir");
        let d = driver(fake_profile("{engine} -e console.log('ok')", None));
        let paths = ProjectPaths::new(dir.path(), &out);
        d.verify(VerifyScope::Compile, &paths);
        assert!(out.exists());
    }
}

fn absolute(p: &Path) -> String {
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    };
    let cleaned = std::fs::canonicalize(&joined).unwrap_or(joined);
    let s = cleaned.to_string_lossy().replace('\\', "/");
    s.strip_prefix("//?/").unwrap_or(&s).to_string()
}

#[cfg(test)]
mod path_tests {
    use super::*;

    #[test]
    fn a_relative_project_path_becomes_absolute() {
        let out = absolute(Path::new("some/relative/dir"));
        assert!(
            Path::new(&out).is_absolute(),
            "a relative --path would resolve against the command's own cwd and miss the project"
        );
    }

    #[test]
    fn an_absolute_path_survives_and_loses_the_windows_verbatim_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let out = absolute(dir.path());
        assert!(!out.starts_with("//?/"), "engines choke on verbatim paths");
        assert!(!out.contains('\\'), "forward slashes keep quoting simple");
    }
}
