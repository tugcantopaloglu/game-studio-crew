use serde_json::Value;
use studio_core::CliEvent;
use studio_events::EventType;

const MIN_GAP_MS: u64 = 500;
const MAX_CHARS: usize = 200;
const MAX_BUFFER: usize = 2000;
const BOUNDARIES: [char; 4] = ['.', '!', '?', '\n'];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Thinking,
    Speaking,
}

impl Phase {
    fn wire(self) -> &'static str {
        match self {
            Phase::Thinking => "thinking",
            Phase::Speaking => "speaking",
        }
    }
}

#[derive(Debug, Default)]
pub struct Stream {
    buf: String,
    phase: Option<Phase>,
    last_emit_ms: Option<u64>,
}

impl Stream {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn observe(&mut self, ev: &CliEvent, role: &str) -> Option<(EventType, Value)> {
        self.observe_at(ev, role, now_ms())
    }

    pub fn flush(&mut self, role: &str) -> Option<(EventType, Value)> {
        self.flush_at(role, now_ms())
    }

    fn observe_at(&mut self, ev: &CliEvent, role: &str, now: u64) -> Option<(EventType, Value)> {
        let (phase, delta) = match ev {
            CliEvent::Thinking { text } => (Phase::Thinking, text.as_str()),
            CliEvent::Text { text } => (Phase::Speaking, text.as_str()),
            _ => return None,
        };
        if delta.is_empty() {
            return None;
        }

        let switched = self.phase.is_some_and(|p| p != phase);
        if switched {
            let closing = self.phase.unwrap();
            let tail = if self.due(now) { self.take(usize::MAX) } else { None };
            self.buf.clear();
            self.phase = Some(phase);
            self.buf.push_str(delta);
            if let Some(text) = tail {
                self.last_emit_ms = Some(now);
                return Some(thought(role, &text, closing.wire()));
            }
            return None;
        }

        if self.phase.is_none() {
            self.phase = Some(phase);
            self.last_emit_ms = Some(now);
        }
        self.buf.push_str(delta);
        self.trim_backlog();

        if !self.due(now) {
            return None;
        }
        let cut = self
            .buf
            .rfind(BOUNDARIES)
            .map(|i| i + self.buf[i..].chars().next().map_or(1, char::len_utf8))
            .unwrap_or(self.buf.len());
        let text = self.take(cut)?;
        self.last_emit_ms = Some(now);
        Some(thought(role, &text, phase.wire()))
    }

    fn flush_at(&mut self, role: &str, _now: u64) -> Option<(EventType, Value)> {
        let phase = self.phase.take()?;
        let text = self.take(usize::MAX).unwrap_or_default();
        Some(thought(role, &text, if text.is_empty() { "done" } else { phase.wire() }))
    }

    fn due(&self, now: u64) -> bool {
        self.last_emit_ms
            .is_none_or(|then| now.saturating_sub(then) >= MIN_GAP_MS)
    }

    fn take(&mut self, upto: usize) -> Option<String> {
        let cut = self.buf.len().min(upto);
        let cut = (0..=cut).rev().find(|i| self.buf.is_char_boundary(*i))?;
        let rest = self.buf.split_off(cut);
        let chunk = std::mem::replace(&mut self.buf, rest);
        let text = shorten(&squeeze(&chunk));
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }

    fn trim_backlog(&mut self) {
        if self.buf.len() <= MAX_BUFFER {
            return;
        }
        let drop = self.buf.len() - MAX_BUFFER;
        let cut = (drop..=self.buf.len())
            .find(|i| self.buf.is_char_boundary(*i))
            .unwrap_or(self.buf.len());
        self.buf = self.buf.split_off(cut);
    }
}

fn thought(role: &str, text: &str, phase: &str) -> (EventType, Value) {
    (
        EventType::AgentThought,
        serde_json::json!({"role": role, "text": text, "phase": phase}),
    )
}

fn squeeze(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut gap = false;
    for ch in raw.chars() {
        if ch.is_whitespace() {
            gap = !out.is_empty();
            continue;
        }
        if ch == '*' || ch == '#' || ch == '`' {
            continue;
        }
        if gap {
            out.push(' ');
            gap = false;
        }
        out.push(ch);
    }
    out
}

fn shorten(text: &str) -> String {
    let count = text.chars().count();
    if count <= MAX_CHARS {
        return text.to_string();
    }
    let tail: String = text.chars().skip(count - MAX_CHARS).collect();
    let start = tail.find(' ').map_or(0, |i| i + 1);
    format!("\u{2026} {}", &tail[start..])
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thinking(text: &str) -> CliEvent {
        CliEvent::Thinking { text: text.into() }
    }

    fn speaking(text: &str) -> CliEvent {
        CliEvent::Text { text: text.into() }
    }

    fn text_of(out: &Option<(EventType, Value)>) -> String {
        out.as_ref().unwrap().1["text"].as_str().unwrap().to_string()
    }

    #[test]
    fn a_token_by_token_burst_never_exceeds_two_events_a_second() {
        let mut s = Stream::new();
        let mut emitted = 0;
        for ms in 0..2000 {
            if s.observe_at(&thinking("tok. "), "producer", ms).is_some() {
                emitted += 1;
            }
        }
        assert!(emitted <= 4, "2000 deltas over 2s produced {emitted} events");
    }

    #[test]
    fn the_first_delta_waits_instead_of_shipping_a_single_token() {
        let mut s = Stream::new();
        assert!(s.observe_at(&thinking("the"), "producer", 0).is_none());
        assert!(s.observe_at(&thinking(" packer"), "producer", 200).is_none());
        assert!(s.observe_at(&thinking(" is deterministic"), "producer", 700).is_some());
    }

    #[test]
    fn a_thought_is_cut_at_the_last_sentence_boundary() {
        let mut s = Stream::new();
        s.observe_at(&thinking("Packing is deterministic. So a new "), "producer", 0);
        let out = s.observe_at(&thinking("role never redraws"), "producer", 600);
        assert_eq!(text_of(&out), "Packing is deterministic.");
        let rest = s.observe_at(&thinking(" the map."), "producer", 1200);
        assert_eq!(text_of(&rest), "So a new role never redraws the map.");
    }

    #[test]
    fn a_wall_of_reasoning_is_capped_before_it_reaches_the_wire() {
        let mut s = Stream::new();
        s.observe_at(&thinking("word "), "producer", 0);
        let out = s.observe_at(&thinking(&"word ".repeat(400)), "producer", 600);
        let text = text_of(&out);
        assert!(text.chars().count() <= MAX_CHARS + 2, "{} chars", text.chars().count());
        assert!(text.starts_with('\u{2026}'));
    }

    #[test]
    fn a_flood_of_reasoning_cannot_grow_the_buffer_without_bound() {
        let mut s = Stream::new();
        for i in 0..200 {
            s.observe_at(&thinking(&"nosentenceboundaryhere".repeat(20)), "producer", i);
        }
        assert!(s.buf.len() <= MAX_BUFFER + 440);
    }

    #[test]
    fn reasoning_and_the_answer_are_tagged_as_different_phases() {
        let mut s = Stream::new();
        s.observe_at(&thinking("weighing the two."), "game_designer", 0);
        let closing = s.observe_at(&speaking("I picked the second."), "game_designer", 600);
        assert_eq!(closing.as_ref().unwrap().1["phase"], "thinking");
        let answer = s.observe_at(&speaking(" It is cheaper."), "game_designer", 1200);
        assert_eq!(answer.as_ref().unwrap().1["phase"], "speaking");
        assert_eq!(text_of(&answer), "I picked the second. It is cheaper.");
    }

    #[test]
    fn the_answer_bubble_never_shows_leftover_reasoning() {
        let mut s = Stream::new();
        s.observe_at(&thinking("secret deliberation"), "artist", 0);
        assert!(s.observe_at(&speaking("the answer"), "artist", 100).is_none());
        let out = s.observe_at(&speaking(" is blue."), "artist", 700);
        assert_eq!(text_of(&out), "the answer is blue.");
    }

    #[test]
    fn every_thought_names_the_role_that_is_having_it() {
        let mut s = Stream::new();
        s.observe_at(&thinking("a."), "qa_engineer", 0);
        let out = s.observe_at(&thinking("b."), "qa_engineer", 600);
        assert_eq!(out.unwrap().1["role"], "qa_engineer");
    }

    #[test]
    fn the_last_flush_ships_the_tail_the_rate_limit_held_back() {
        let mut s = Stream::new();
        s.observe_at(&speaking("one last line"), "producer", 0);
        let out = s.flush("producer");
        assert_eq!(text_of(&out), "one last line");
        assert_eq!(out.unwrap().1["phase"], "speaking");
    }

    #[test]
    fn the_last_flush_closes_the_bubble_when_nothing_is_pending() {
        let mut s = Stream::new();
        s.observe_at(&speaking("said it all."), "producer", 0);
        s.observe_at(&speaking(" done."), "producer", 600);
        let out = s.flush("producer");
        assert_eq!(out.as_ref().unwrap().1["phase"], "done");
        assert_eq!(out.unwrap().1["text"], "");
    }

    #[test]
    fn a_worker_that_never_reasoned_out_loud_emits_nothing() {
        let mut s = Stream::new();
        assert!(s.flush("producer").is_none());
    }

    #[test]
    fn tool_traffic_and_usage_deltas_are_not_thoughts() {
        let mut s = Stream::new();
        let tool = CliEvent::ToolCall { tool: "Read".into(), args_digest: "d".into() };
        assert!(s.observe_at(&tool, "producer", 0).is_none());
        assert!(s.observe_at(&CliEvent::Other { kind: "x".into() }, "producer", 900).is_none());
    }

    #[test]
    fn markdown_scaffolding_is_stripped_out_of_the_bubble() {
        let mut s = Stream::new();
        s.observe_at(&speaking("## **Plan**\n\n"), "producer", 0);
        let out = s.observe_at(&speaking("write the `packer`."), "producer", 600);
        assert_eq!(text_of(&out), "Plan write the packer.");
    }
}
