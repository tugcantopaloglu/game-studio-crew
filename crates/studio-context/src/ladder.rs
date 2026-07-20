use crate::capsule::{turn_digest, Capsule};
use crate::estimate_tokens;
use std::collections::BTreeMap;
use std::time::Duration;

pub const ROLLUP_AGENT_SPACING: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rung {
    Turn,
    Task,
    Sprint,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rollup {
    pub text: String,
    pub tokens: usize,
    pub source_capsules: usize,
    pub distilled_by_model: bool,
    pub tokens_saved_est: usize,
}

pub fn turn_digests(capsules: &[Capsule]) -> Vec<String> {
    capsules.iter().map(turn_digest).collect()
}

pub fn template_rollup(capsules: &[Capsule]) -> Rollup {
    let mut by_outcome: BTreeMap<String, usize> = BTreeMap::new();
    let mut claims: Vec<&str> = Vec::new();
    let mut questions: Vec<&str> = Vec::new();
    let mut dead_ends: Vec<&str> = Vec::new();
    let mut roles: BTreeMap<&str, usize> = BTreeMap::new();

    for c in capsules {
        *by_outcome.entry(format!("{:?}", c.outcome)).or_insert(0) += 1;
        *roles.entry(c.from.as_str()).or_insert(0) += 1;
        for d in &c.decisions {
            claims.push(d.claim.as_str());
        }
        for q in &c.open_questions {
            questions.push(q.as_str());
        }
        for d in &c.do_not_revisit {
            dead_ends.push(d.as_str());
        }
    }

    let mut s = String::new();
    s.push_str(&format!("{} task capsules across {} actors.\n", capsules.len(), roles.len()));

    if !by_outcome.is_empty() {
        let parts: Vec<String> = by_outcome.iter().map(|(k, n)| format!("{n} {k}")).collect();
        s.push_str(&format!("outcomes: {}\n", parts.join(", ")));
    }

    if !claims.is_empty() {
        s.push_str("\ndecisions:\n");
        for c in &claims {
            s.push_str(&format!("  - {c}\n"));
        }
    }

    if !dead_ends.is_empty() {
        s.push_str("\ndo_not_revisit:\n");
        for d in &dead_ends {
            s.push_str(&format!("  - {d}\n"));
        }
    }

    if !questions.is_empty() {
        s.push_str("\nopen_questions:\n");
        for q in &questions {
            s.push_str(&format!("  - {q}\n"));
        }
    }

    let source_tokens: usize = capsules.iter().map(|c| estimate_tokens(&c.summary)).sum();
    let tokens = estimate_tokens(&s);

    Rollup {
        tokens,
        source_capsules: capsules.len(),
        distilled_by_model: false,
        tokens_saved_est: source_tokens.saturating_sub(tokens),
        text: s,
    }
}

pub fn rollup_prompt(capsules: &[Capsule]) -> String {
    let mut s = String::new();
    s.push_str(
        "Distill these task capsules into one paragraph a sprint-level actor can act on.\n\
         Preserve every decision claim and every do_not_revisit entry verbatim.\n\
         Do not invent status. Do not list the capsules; synthesize them.\n\n",
    );
    for c in capsules {
        s.push_str(&format!(
            "- [{}] {:?}: {}\n",
            c.from,
            c.outcome,
            c.summary.trim()
        ));
        for d in &c.decisions {
            s.push_str(&format!("    decision: {}\n", d.claim));
        }
        for d in &c.do_not_revisit {
            s.push_str(&format!("    dead end: {d}\n"));
        }
    }
    s
}

pub fn accept_model_rollup(text: &str, capsules: &[Capsule]) -> Option<Rollup> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    for c in capsules {
        for d in &c.do_not_revisit {
            if !trimmed.contains(d.trim()) {
                return None;
            }
        }
    }

    let source_tokens: usize = capsules.iter().map(|c| estimate_tokens(&c.summary)).sum();
    let tokens = estimate_tokens(trimmed);

    Some(Rollup {
        text: trimmed.to_string(),
        tokens,
        source_capsules: capsules.len(),
        distilled_by_model: true,
        tokens_saved_est: source_tokens.saturating_sub(tokens),
    })
}

pub fn sprint_rollup(capsules: &[Capsule], model_output: Option<&str>) -> Rollup {
    model_output
        .and_then(|t| accept_model_rollup(t, capsules))
        .unwrap_or_else(|| template_rollup(capsules))
}

pub fn supersedes(needed: Rung, offered: Rung) -> bool {
    offered >= needed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capsule::{CapsuleKind, CapsuleOutcome, DecisionClaim};

    fn cap(from: &str, summary: &str, outcome: CapsuleOutcome) -> Capsule {
        Capsule {
            v: 1,
            kind: CapsuleKind::TaskReturn,
            from: from.into(),
            task: "t".into(),
            summary: summary.into(),
            outcome,
            artifacts: vec![],
            decisions: vec![],
            open_questions: vec![],
            do_not_revisit: vec![],
            handoff: String::new(),
        }
    }

    fn sprint() -> Vec<Capsule> {
        let mut a = cap("gameplay_engineer#1", "Added the dash ability.", CapsuleOutcome::Done);
        a.decisions.push(DecisionClaim {
            claim: "Dash is a state machine".into(),
            rationale: "composability".into(),
        });
        a.do_not_revisit.push("the animation-event path drops frames".into());

        let mut b = cap("qa_engineer#1", "Wrote dash regression tests.", CapsuleOutcome::NeedsVerify);
        b.open_questions.push("should dash cancel on hit?".into());

        let c = cap("artist#1", "Drew the dash trail.", CapsuleOutcome::Blocked);
        vec![a, b, c]
    }

    #[test]
    fn the_template_rollup_needs_no_model_and_always_works() {
        let r = template_rollup(&sprint());
        assert!(!r.distilled_by_model);
        assert!(r.text.contains("3 task capsules"));
        assert!(r.text.contains("1 Done"));
        assert!(r.text.contains("1 Blocked"));
        assert!(r.text.contains("1 NeedsVerify"));
    }

    #[test]
    fn the_template_rollup_keeps_decisions_and_dead_ends() {
        let r = template_rollup(&sprint());
        assert!(r.text.contains("Dash is a state machine"));
        assert!(r.text.contains("the animation-event path drops frames"));
        assert!(r.text.contains("should dash cancel on hit?"));
    }

    #[test]
    fn an_empty_sprint_still_produces_a_rollup() {
        let r = template_rollup(&[]);
        assert!(r.text.contains("0 task capsules"));
        assert_eq!(r.source_capsules, 0);
    }

    #[test]
    fn the_ladder_falls_back_to_the_template_when_the_model_call_fails() {
        let caps = sprint();
        let r = sprint_rollup(&caps, None);
        assert!(!r.distilled_by_model);
        assert!(r.text.contains("3 task capsules"));
    }

    #[test]
    fn an_empty_model_response_falls_back_rather_than_producing_nothing() {
        let caps = sprint();
        let r = sprint_rollup(&caps, Some("   \n  "));
        assert!(!r.distilled_by_model);
        assert!(!r.text.is_empty());
    }

    #[test]
    fn a_model_rollup_that_drops_a_dead_end_is_rejected() {
        let caps = sprint();
        let lossy = "The team added dash, tested it, and blocked on art.";
        let r = sprint_rollup(&caps, Some(lossy));
        assert!(
            !r.distilled_by_model,
            "a rollup that loses do_not_revisit would let the studio re-derive a known failure"
        );
    }

    #[test]
    fn a_faithful_model_rollup_is_accepted() {
        let caps = sprint();
        let good = "Dash landed as a state machine and is under regression test, with art \
                    blocked; the animation-event path drops frames so it stays off the table.";
        let r = sprint_rollup(&caps, Some(good));
        assert!(r.distilled_by_model);
        assert_eq!(r.text, good);
    }

    #[test]
    fn rollups_report_the_tokens_they_saved() {
        let caps = sprint();
        let good = "Dash landed; the animation-event path drops frames.";
        let r = sprint_rollup(&caps, Some(good));
        assert!(r.tokens_saved_est > 0 || r.tokens <= caps.len());
    }

    #[test]
    fn turn_digests_are_pure_extraction() {
        let d = turn_digests(&sprint());
        assert_eq!(d.len(), 3);
        assert!(d[0].contains("gameplay_engineer#1"));
        assert!(d[2].contains("Blocked"));
    }

    #[test]
    fn no_actor_receives_a_rung_below_the_one_it_needs() {
        assert!(supersedes(Rung::Task, Rung::Sprint));
        assert!(supersedes(Rung::Task, Rung::Task));
        assert!(!supersedes(Rung::Sprint, Rung::Task));
        assert!(!supersedes(Rung::Task, Rung::Turn));
    }

    #[test]
    fn the_rollup_prompt_carries_the_fields_the_model_must_preserve() {
        let p = rollup_prompt(&sprint());
        assert!(p.contains("Dash is a state machine"));
        assert!(p.contains("the animation-event path drops frames"));
        assert!(p.contains("verbatim"));
    }
}
