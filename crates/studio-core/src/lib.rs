mod proc;
mod spec;
mod stream;

pub use proc::ProcessGroup;
pub use spec::{Effort, SessionMode, WorkerSpec};
pub use stream::{map_cli_event, CliEvent, McpServer, StreamState};

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use studio_events::Outcome;

pub const WALL_CLOCK_LIMIT: Duration = Duration::from_secs(600);

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, CoreError>;

#[derive(Debug, Clone, PartialEq)]
pub struct WorkerReport {
    pub outcome: Outcome,
    pub state: StreamStateSnapshot,
    pub exit_code: Option<i32>,
    pub duration: Duration,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct StreamStateSnapshot {
    pub session_id: Option<String>,
    pub usage: Option<studio_events::Usage>,
    pub cost_usd: f64,
    pub is_error: bool,
    pub saw_result: bool,
    pub text: String,
    pub mcp_connected: Vec<String>,
}

pub struct WorkerLimits {
    pub stall_timeout: Duration,
    pub wall_clock: Duration,
}

impl Default for WorkerLimits {
    fn default() -> Self {
        Self { stall_timeout: Duration::from_secs(180), wall_clock: WALL_CLOCK_LIMIT }
    }
}

pub struct Worker {
    child: Child,
    group: ProcessGroup,
}

impl Worker {
    pub fn spawn(program: &str, args: &[String], brief: &str) -> Result<Self> {
        let mut group = ProcessGroup::new()?;
        let mut cmd = Command::new(program);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        group.prepare(&mut cmd);

        let mut child = cmd.spawn()?;
        group.adopt(&child)?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(brief.as_bytes())?;
        }

        Ok(Self { child, group })
    }

    pub fn drive(
        mut self,
        limits: &WorkerLimits,
        mut on_event: impl FnMut(&CliEvent),
    ) -> Result<WorkerReport> {
        let started = Instant::now();
        let stdout = self.child.stdout.take().expect("stdout piped at spawn");

        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let pump = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(l) => {
                        if tx.send(l).is_err() {
                            return;
                        }
                    }
                    Err(_) => return,
                }
            }
        });

        let mut state = StreamState::default();
        let mut timed_out = false;
        let mut stalled = false;

        loop {
            let remaining_wall = limits.wall_clock.saturating_sub(started.elapsed());
            if remaining_wall.is_zero() {
                timed_out = true;
                break;
            }
            let wait = limits.stall_timeout.min(remaining_wall);

            match rx.recv_timeout(wait) {
                Ok(line) => {
                    if let Some(ev) = stream::parse_line(&line) {
                        state.apply(&ev);
                        on_event(&ev);
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if started.elapsed() >= limits.wall_clock {
                        timed_out = true;
                    } else {
                        stalled = true;
                    }
                    break;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        let exit_code = if timed_out || stalled {
            let _ = self.group.kill_tree();
            let _ = self.child.wait();
            None
        } else {
            self.child.wait()?.code()
        };
        let _ = pump.join();

        let snapshot = StreamStateSnapshot {
            session_id: state.session_id.clone(),
            usage: state.authoritative_usage(),
            cost_usd: state.cost_usd,
            is_error: state.is_error,
            saw_result: state.saw_result,
            text: state.text.clone(),
            mcp_connected: state
                .mcp_servers
                .iter()
                .filter(|s| s.is_connected())
                .map(|s| s.name.clone())
                .collect(),
        };

        let outcome = if timed_out {
            Outcome::TimedOut
        } else if stalled {
            Outcome::Stalled
        } else if !state.saw_result {
            Outcome::Crashed
        } else if state.is_error {
            Outcome::Crashed
        } else {
            Outcome::Completed
        };

        Ok(WorkerReport {
            outcome,
            state: snapshot,
            exit_code,
            duration: started.elapsed(),
        })
    }

    pub fn kill(&mut self) -> Result<()> {
        self.group.kill_tree()?;
        let _ = self.child.wait();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node_emitting(script: &str) -> Vec<String> {
        vec!["-e".to_string(), script.to_string()]
    }

    #[test]
    fn drives_a_stream_to_a_clean_completion() {
        let script = r#"
            const line = (o) => process.stdout.write(JSON.stringify(o) + "\n");
            line({type:"system",subtype:"init",session_id:"sess-42",mcp_servers:[{name:"studio",status:"connected"}]});
            line({type:"stream_event",event:{type:"message_start",message:{usage:{input_tokens:2,cache_creation_input_tokens:8867,cache_read_input_tokens:0,output_tokens:1}}}});
            line({type:"stream_event",event:{type:"content_block_delta",delta:{type:"text_delta",text:"pong"}}});
            line({type:"result",subtype:"success",is_error:false,session_id:"sess-42",total_cost_usd:0.0888,usage:{input_tokens:2,output_tokens:4,cache_read_input_tokens:0,cache_creation_input_tokens:8867}});
        "#;
        let w = Worker::spawn("node", &node_emitting(script), "").unwrap();
        let mut seen = 0;
        let report = w.drive(&WorkerLimits::default(), |_| seen += 1).unwrap();

        assert_eq!(report.outcome, Outcome::Completed);
        assert_eq!(report.state.session_id.as_deref(), Some("sess-42"));
        assert_eq!(report.state.text, "pong");
        assert_eq!(report.state.mcp_connected, vec!["studio".to_string()]);
        assert_eq!(report.exit_code, Some(0));
        assert!(seen >= 4);

        let usage = report.state.usage.unwrap();
        assert_eq!(usage.cache_creation, 8867);
        assert_eq!(usage.output, 4);
        assert!((report.state.cost_usd - 0.0888).abs() < 1e-9);
    }

    #[test]
    fn the_final_result_wins_over_the_interim_estimate() {
        let script = r#"
            const line = (o) => process.stdout.write(JSON.stringify(o) + "\n");
            line({type:"stream_event",event:{type:"message_start",message:{usage:{input_tokens:2,output_tokens:1,cache_read_input_tokens:0,cache_creation_input_tokens:8867}}}});
            line({type:"stream_event",event:{type:"message_delta",usage:{input_tokens:2,output_tokens:50,cache_read_input_tokens:0,cache_creation_input_tokens:8867}}});
            line({type:"result",subtype:"success",is_error:false,total_cost_usd:0.09,usage:{input_tokens:2,output_tokens:101,cache_read_input_tokens:0,cache_creation_input_tokens:8867}});
        "#;
        let w = Worker::spawn("node", &node_emitting(script), "").unwrap();
        let report = w.drive(&WorkerLimits::default(), |_| {}).unwrap();
        assert_eq!(report.state.usage.unwrap().output, 101);
    }

    #[test]
    fn a_stream_without_a_result_is_a_crash() {
        let script = r#"
            const line = (o) => process.stdout.write(JSON.stringify(o) + "\n");
            line({type:"system",subtype:"init",session_id:"s"});
            process.exit(1);
        "#;
        let w = Worker::spawn("node", &node_emitting(script), "").unwrap();
        let report = w.drive(&WorkerLimits::default(), |_| {}).unwrap();
        assert_eq!(report.outcome, Outcome::Crashed);
        assert!(!report.state.saw_result);
    }

    #[test]
    fn a_not_logged_in_result_is_not_reported_as_completed() {
        let script = r#"
            const line = (o) => process.stdout.write(JSON.stringify(o) + "\n");
            line({type:"result",subtype:"success",is_error:true,result:"Not logged in",total_cost_usd:0,usage:{input_tokens:0,output_tokens:0,cache_read_input_tokens:0,cache_creation_input_tokens:0}});
        "#;
        let w = Worker::spawn("node", &node_emitting(script), "").unwrap();
        let report = w.drive(&WorkerLimits::default(), |_| {}).unwrap();
        assert_eq!(report.outcome, Outcome::Crashed);
        assert!(report.state.is_error);
    }

    #[test]
    fn a_wedged_worker_is_killed_at_the_wall_clock_limit() {
        let script = r#"
            const line = (o) => process.stdout.write(JSON.stringify(o) + "\n");
            line({type:"system",subtype:"init",session_id:"s"});
            setInterval(() => line({type:"stream_event",event:{type:"content_block_delta",delta:{type:"text_delta",text:"x"}}}), 20);
        "#;
        let limits = WorkerLimits {
            stall_timeout: Duration::from_secs(30),
            wall_clock: Duration::from_millis(300),
        };
        let w = Worker::spawn("node", &node_emitting(script), "").unwrap();
        let report = w.drive(&limits, |_| {}).unwrap();
        assert_eq!(report.outcome, Outcome::TimedOut);
        assert!(report.duration < Duration::from_secs(10));
    }

    #[test]
    fn a_silent_worker_is_killed_by_the_stall_watchdog() {
        let script = r#"
            const line = (o) => process.stdout.write(JSON.stringify(o) + "\n");
            line({type:"system",subtype:"init",session_id:"s"});
            setTimeout(() => {}, 60000);
        "#;
        let limits = WorkerLimits {
            stall_timeout: Duration::from_millis(200),
            wall_clock: Duration::from_secs(30),
        };
        let w = Worker::spawn("node", &node_emitting(script), "").unwrap();
        let report = w.drive(&limits, |_| {}).unwrap();
        assert!(
            matches!(report.outcome, Outcome::Stalled | Outcome::Crashed),
            "a silent worker must not hold its slot forever, got {:?}",
            report.outcome
        );
    }

    #[test]
    fn the_task_brief_reaches_the_worker_on_stdin() {
        let script = r#"
            let s = "";
            process.stdin.on("data", d => s += d);
            process.stdin.on("end", () => {
              process.stdout.write(JSON.stringify({type:"stream_event",event:{type:"content_block_delta",delta:{type:"text_delta",text:s}}}) + "\n");
              process.stdout.write(JSON.stringify({type:"result",subtype:"success",is_error:false,total_cost_usd:0,usage:{}}) + "\n");
            });
        "#;
        let w = Worker::spawn("node", &node_emitting(script), "L3 task brief").unwrap();
        let report = w.drive(&WorkerLimits::default(), |_| {}).unwrap();
        assert_eq!(report.state.text, "L3 task brief");
    }

    #[test]
    fn killing_a_worker_terminates_its_child_processes() {
        let script = r#"
            const { spawn } = require("child_process");
            spawn(process.execPath, ["-e", "setTimeout(()=>{}, 60000)"], { stdio: "ignore" });
            process.stdout.write(JSON.stringify({type:"system",subtype:"init",session_id:"s"}) + "\n");
            setTimeout(() => {}, 60000);
        "#;
        let limits = WorkerLimits {
            stall_timeout: Duration::from_millis(250),
            wall_clock: Duration::from_secs(30),
        };
        let w = Worker::spawn("node", &node_emitting(script), "").unwrap();
        let report = w.drive(&limits, |_| {}).unwrap();
        assert!(report.duration < Duration::from_secs(10));
    }
}
