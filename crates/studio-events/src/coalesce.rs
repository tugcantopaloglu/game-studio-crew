use crate::{Envelope, EventType};
use std::collections::HashMap;
use std::time::Duration;

pub const FLUSH_INTERVAL: Duration = Duration::from_millis(100);
pub const SNAPSHOT_GAP_THRESHOLD: u64 = 5000;

#[derive(Debug, Default)]
pub struct Coalescer {
    pending: HashMap<(String, EventType), Envelope>,
    order: Vec<(String, EventType)>,
    passthrough: Vec<Envelope>,
}

impl Coalescer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, event: Envelope) {
        if event.event_type.never_coalesced() {
            self.passthrough.push(event);
            return;
        }

        let key = (event.actor.clone(), event.event_type);
        if self.pending.insert(key.clone(), event).is_none() {
            self.order.push(key);
        }
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len() + self.passthrough.len()
    }

    pub fn flush(&mut self) -> Vec<Envelope> {
        let mut out = std::mem::take(&mut self.passthrough);
        for key in self.order.drain(..) {
            if let Some(e) = self.pending.remove(&key) {
                out.push(e);
            }
        }
        out.sort_by_key(|e| e.seq);
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumePlan {
    Replay { from_seq: u64, to_seq: u64 },
    Snapshot { head: u64 },
    UpToDate,
}

pub fn plan_resume(since_seq: u64, head: u64) -> ResumePlan {
    if since_seq >= head {
        return ResumePlan::UpToDate;
    }
    if head - since_seq > SNAPSHOT_GAP_THRESHOLD {
        return ResumePlan::Snapshot { head };
    }
    ResumePlan::Replay { from_seq: since_seq + 1, to_seq: head }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Scene;
    use serde_json::json;

    fn ev(seq: u64, actor: &str, ty: EventType) -> Envelope {
        Envelope::new(seq, "t", "run", actor, Scene::daemon(), ty, json!({"seq": seq}))
    }

    #[test]
    fn repeated_updates_from_one_actor_collapse_to_the_latest() {
        let mut c = Coalescer::new();
        for seq in 1..=50 {
            c.push(ev(seq, "gameplay_engineer#1", EventType::TokenUsage));
        }
        let out = c.flush();
        assert_eq!(out.len(), 1, "a burst of token updates is one flush");
        assert_eq!(out[0].seq, 50, "the latest wins");
        assert_eq!(out[0].data["seq"], 50);
    }

    #[test]
    fn different_actors_are_coalesced_independently() {
        let mut c = Coalescer::new();
        c.push(ev(1, "a", EventType::TokenUsage));
        c.push(ev(2, "b", EventType::TokenUsage));
        c.push(ev(3, "a", EventType::TokenUsage));
        let out = c.flush();
        assert_eq!(out.len(), 2);
        assert_eq!(out.iter().find(|e| e.actor == "a").unwrap().seq, 3);
        assert_eq!(out.iter().find(|e| e.actor == "b").unwrap().seq, 2);
    }

    #[test]
    fn different_types_from_one_actor_are_coalesced_independently() {
        let mut c = Coalescer::new();
        c.push(ev(1, "a", EventType::TokenUsage));
        c.push(ev(2, "a", EventType::WorkerStateChanged));
        let out = c.flush();
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn terminal_events_are_never_dropped_even_under_a_burst() {
        let mut c = Coalescer::new();
        c.push(ev(1, "a", EventType::TokenUsage));
        c.push(ev(2, "a", EventType::VerifyResult));
        c.push(ev(3, "a", EventType::TokenUsage));
        c.push(ev(4, "a", EventType::VerifyResult));
        c.push(ev(5, "a", EventType::BudgetExhausted));

        let out = c.flush();
        let verify: Vec<&Envelope> = out
            .iter()
            .filter(|e| e.event_type == EventType::VerifyResult)
            .collect();
        assert_eq!(verify.len(), 2, "both verify results must survive");
        assert!(out.iter().any(|e| e.event_type == EventType::BudgetExhausted));
    }

    #[test]
    fn a_decision_is_never_coalesced_away() {
        let mut c = Coalescer::new();
        for seq in 1..=5 {
            c.push(ev(seq, "a", EventType::DecisionRecorded));
        }
        assert_eq!(c.flush().len(), 5, "every recorded decision reaches the client");
    }

    #[test]
    fn a_flush_emits_in_sequence_order() {
        let mut c = Coalescer::new();
        c.push(ev(9, "z", EventType::TokenUsage));
        c.push(ev(3, "a", EventType::VerifyResult));
        c.push(ev(7, "m", EventType::WorkerStateChanged));
        let out = c.flush();
        let seqs: Vec<u64> = out.iter().map(|e| e.seq).collect();
        let mut sorted = seqs.clone();
        sorted.sort_unstable();
        assert_eq!(seqs, sorted, "clients detect loss by seq order");
    }

    #[test]
    fn flushing_twice_yields_nothing_the_second_time() {
        let mut c = Coalescer::new();
        c.push(ev(1, "a", EventType::TokenUsage));
        assert_eq!(c.flush().len(), 1);
        assert_eq!(c.flush().len(), 0);
        assert_eq!(c.pending_len(), 0);
    }

    #[test]
    fn a_client_that_is_current_needs_nothing() {
        assert_eq!(plan_resume(100, 100), ResumePlan::UpToDate);
        assert_eq!(plan_resume(101, 100), ResumePlan::UpToDate);
    }

    #[test]
    fn a_small_gap_is_replayed_from_the_next_sequence() {
        assert_eq!(
            plan_resume(100, 150),
            ResumePlan::Replay { from_seq: 101, to_seq: 150 }
        );
    }

    #[test]
    fn a_large_gap_is_answered_with_a_snapshot_instead_of_a_replay() {
        assert_eq!(
            plan_resume(0, SNAPSHOT_GAP_THRESHOLD + 1),
            ResumePlan::Snapshot { head: SNAPSHOT_GAP_THRESHOLD + 1 }
        );
    }

    #[test]
    fn the_snapshot_threshold_is_inclusive_of_the_replay_case() {
        assert!(matches!(
            plan_resume(0, SNAPSHOT_GAP_THRESHOLD),
            ResumePlan::Replay { .. }
        ));
    }

    #[test]
    fn a_fresh_client_replays_from_the_first_event() {
        assert_eq!(plan_resume(0, 3), ResumePlan::Replay { from_seq: 1, to_seq: 3 });
    }

    #[test]
    fn a_burst_from_one_actor_keys_the_order_list_once() {
        let mut c = Coalescer::new();
        for seq in 1..=5000 {
            c.push(ev(seq, "gameplay_engineer#1", EventType::TokenUsage));
        }
        assert_eq!(c.order.len(), 1, "a repeat must not grow the order list");
        assert_eq!(c.flush().len(), 1);
    }
}

#[cfg(test)]
mod scale_probe {
    use super::*;
    use crate::Scene;
    use serde_json::json;
    use std::time::Instant;

    fn log(events: u64) -> Vec<Envelope> {
        let types = [
            EventType::TokenUsage,
            EventType::ToolCall,
            EventType::WorkerStateChanged,
            EventType::AgentThought,
            EventType::MeetingSpoke,
        ];
        (1..=events)
            .map(|seq| {
                Envelope::new(
                    seq,
                    "2026-07-25T09:12:44.118Z",
                    "run",
                    format!("gameplay_engineer#{}", seq % 13),
                    Scene::daemon(),
                    types[(seq % types.len() as u64) as usize],
                    json!({"seq": seq, "bytes": 4096}),
                )
            })
            .collect()
    }

    #[test]
    #[ignore]
    fn compacting_a_long_log_stays_linear_in_its_length() {
        println!("events    compact      per event   emitted");
        for n in [1_000u64, 10_000, 50_000, 200_000] {
            let events = log(n);
            let started = Instant::now();
            let mut c = Coalescer::new();
            for e in events {
                c.push(e);
            }
            let out = c.flush();
            let took = started.elapsed();
            println!(
                "{n:<9} {:>8.2}ms {:>10.3}us {:>9}",
                took.as_secs_f64() * 1000.0,
                took.as_secs_f64() * 1_000_000.0 / n as f64,
                out.len(),
            );
        }
    }
}
