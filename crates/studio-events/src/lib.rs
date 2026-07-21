pub mod coalesce;
pub use coalesce::{plan_resume, Coalescer, ResumePlan, FLUSH_INTERVAL, SNAPSHOT_GAP_THRESHOLD};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u32,
    pub seq: u64,
    pub ts: String,
    pub run: String,
    pub actor: String,
    pub scene: Scene,
    #[serde(rename = "type")]
    pub event_type: EventType,
    pub data: Value,
}

impl Envelope {
    pub fn new(
        seq: u64,
        ts: impl Into<String>,
        run: impl Into<String>,
        actor: impl Into<String>,
        scene: Scene,
        event_type: EventType,
        data: Value,
    ) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            seq,
            ts: ts.into(),
            run: run.into(),
            actor: actor.into(),
            scene,
            event_type,
            data,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub room: Option<String>,
    pub desk: Option<String>,
    pub meeting: Option<String>,
}

impl Scene {
    pub fn daemon() -> Self {
        Self { room: None, desk: None, meeting: None }
    }

    pub fn desk(room: impl Into<String>, desk: impl Into<String>) -> Self {
        Self { room: Some(room.into()), desk: Some(desk.into()), meeting: None }
    }

    pub fn in_meeting(mut self, meeting: impl Into<String>) -> Self {
        self.meeting = Some(meeting.into());
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    RunStarted,
    RunEnded,
    WorkerSpawned,
    WorkerStateChanged,
    WorkerExited,
    ToolCall,
    ToolResult,

    PromptFrozen,
    CacheHit,
    TokenUsage,
    SummaryCreated,

    TaskDelegated,
    TaskReturned,
    ConsultRequested,
    ConsultAnswered,
    Escalated,
    CapsuleSubmitted,
    MeetingStarted,
    MeetingEnded,
    DecisionRecorded,

    VerifyStarted,
    VerifyResult,
    RepairRound,
    InconclusiveFlagged,

    BudgetWarning,
    DegradationApplied,
    BudgetExhausted,

    WorkflowStarted,
    NodeEntered,
    GateEvaluated,
    WorkflowEnded,

    IndexUpdated,
    CommitRecorded,
    BudgetApprovalNeeded,
}

impl EventType {
    pub const ALL: [EventType; 34] = [
        EventType::RunStarted,
        EventType::RunEnded,
        EventType::WorkerSpawned,
        EventType::WorkerStateChanged,
        EventType::WorkerExited,
        EventType::ToolCall,
        EventType::ToolResult,
        EventType::PromptFrozen,
        EventType::CacheHit,
        EventType::TokenUsage,
        EventType::SummaryCreated,
        EventType::TaskDelegated,
        EventType::TaskReturned,
        EventType::ConsultRequested,
        EventType::ConsultAnswered,
        EventType::Escalated,
        EventType::CapsuleSubmitted,
        EventType::MeetingStarted,
        EventType::MeetingEnded,
        EventType::DecisionRecorded,
        EventType::VerifyStarted,
        EventType::VerifyResult,
        EventType::RepairRound,
        EventType::InconclusiveFlagged,
        EventType::BudgetWarning,
        EventType::DegradationApplied,
        EventType::BudgetExhausted,
        EventType::WorkflowStarted,
        EventType::NodeEntered,
        EventType::GateEvaluated,
        EventType::WorkflowEnded,
        EventType::IndexUpdated,
        EventType::CommitRecorded,
        EventType::BudgetApprovalNeeded,
    ];

    pub fn wire_name(&self) -> &'static str {
        WIRE_NAMES[*self as usize]
    }

    pub fn never_coalesced(&self) -> bool {
        matches!(
            self,
            EventType::RunEnded
                | EventType::MeetingEnded
                | EventType::WorkflowEnded
                | EventType::DecisionRecorded
                | EventType::Escalated
                | EventType::VerifyResult
                | EventType::BudgetExhausted
        )
    }
}

const WIRE_NAMES: [&str; 34] = [
    "run_started",
    "run_ended",
    "worker_spawned",
    "worker_state_changed",
    "worker_exited",
    "tool_call",
    "tool_result",
    "prompt_frozen",
    "cache_hit",
    "token_usage",
    "summary_created",
    "task_delegated",
    "task_returned",
    "consult_requested",
    "consult_answered",
    "escalated",
    "capsule_submitted",
    "meeting_started",
    "meeting_ended",
    "decision_recorded",
    "verify_started",
    "verify_result",
    "repair_round",
    "inconclusive_flagged",
    "budget_warning",
    "degradation_applied",
    "budget_exhausted",
    "workflow_started",
    "node_entered",
    "gate_evaluated",
    "workflow_ended",
    "index_updated",
    "commit_recorded",
    "budget_approval_needed",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerState {
    Queued,
    Admitted,
    Running,
    Streaming,
    Settling,
    Reaped,
    Stalled,
    RateLimited,
    TimedOut,
    Refused,
}

impl WorkerState {
    pub fn is_terminal(&self) -> bool {
        matches!(self, WorkerState::Reaped | WorkerState::Refused)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Completed,
    CompletedNoCapsule,
    Refused,
    Stalled,
    TimedOut,
    RateLimitedOut,
    Crashed,
    Killed,
}

impl Outcome {
    pub fn is_clean(&self) -> bool {
        matches!(self, Outcome::Completed)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
}

impl Usage {
    pub fn total_input(&self) -> u64 {
        self.input + self.cache_read + self.cache_creation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_has_exactly_the_thirty_four_types_in_doc_05() {
        assert_eq!(EventType::ALL.len(), 34);
        assert_eq!(WIRE_NAMES.len(), 34);
    }

    #[test]
    fn wire_names_match_serde_tags() {
        for (i, ev) in EventType::ALL.iter().enumerate() {
            let json = serde_json::to_string(ev).unwrap();
            let expected = format!("\"{}\"", WIRE_NAMES[i]);
            assert_eq!(json, expected, "variant at index {i} disagrees with WIRE_NAMES");
            assert_eq!(ev.wire_name(), WIRE_NAMES[i]);
        }
    }

    #[test]
    fn all_variants_are_distinct() {
        let mut seen = std::collections::HashSet::new();
        for ev in EventType::ALL {
            assert!(seen.insert(ev), "duplicate variant in ALL: {ev:?}");
        }
    }

    #[test]
    fn envelope_round_trips_and_renames_type() {
        let env = Envelope::new(
            148213,
            "2026-07-20T09:12:44.118Z",
            "run_01J",
            "gameplay_engineer#7",
            Scene::desk("engineering", "gameplay_engineer#7"),
            EventType::ToolCall,
            serde_json::json!({"tool": "Read", "args_digest": "b3:abc"}),
        );

        let s = serde_json::to_string(&env).unwrap();
        assert!(s.contains("\"type\":\"tool_call\""));
        assert!(s.contains("\"v\":1"));

        let back: Envelope = serde_json::from_str(&s).unwrap();
        assert_eq!(back, env);
    }

    #[test]
    fn unknown_data_keys_survive_round_trip() {
        let raw = r#"{"v":1,"seq":1,"ts":"t","run":"r","actor":"daemon",
            "scene":{"room":null,"desk":null,"meeting":null},
            "type":"index_updated","data":{"paths_changed":3,"future_key":"kept"}}"#;
        let env: Envelope = serde_json::from_str(raw).unwrap();
        assert_eq!(env.data["future_key"], "kept");
    }

    #[test]
    fn terminal_events_are_exempt_from_coalescing() {
        assert!(EventType::VerifyResult.never_coalesced());
        assert!(EventType::BudgetExhausted.never_coalesced());
        assert!(!EventType::TokenUsage.never_coalesced());
        assert!(!EventType::WorkerStateChanged.never_coalesced());
    }
}
