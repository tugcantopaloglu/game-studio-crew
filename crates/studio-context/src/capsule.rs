use crate::estimate_tokens;
use serde::{Deserialize, Serialize};

pub const CAPSULE_TOKEN_CAP: usize = 4096;
pub const SUMMARY_TOKEN_CAP: usize = 512;
pub const HANDOFF_TOKEN_CAP: usize = 1024;
pub const OPEN_QUESTIONS_KEPT: usize = 3;
pub const ARTIFACTS_KEPT: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapsuleKind {
    TaskReturn,
    ConsultAnswer,
    Decision,
    Escalation,
    Status,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapsuleOutcome {
    Done,
    Blocked,
    NeedsVerify,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Added,
    Modified,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artifact {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    pub change: Change,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionClaim {
    pub claim: String,
    #[serde(default)]
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Capsule {
    pub v: u32,
    pub kind: CapsuleKind,
    pub from: String,
    pub task: String,
    pub summary: String,
    pub outcome: CapsuleOutcome,
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
    #[serde(default)]
    pub decisions: Vec<DecisionClaim>,
    #[serde(default)]
    pub open_questions: Vec<String>,
    #[serde(default)]
    pub do_not_revisit: Vec<String>,
    #[serde(default)]
    pub handoff: String,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum CapsuleError {
    #[error("unsupported capsule version {0}; this daemon speaks v1")]
    Version(u32),

    #[error("field '{0}' is required and must not be empty")]
    MissingField(&'static str),

    #[error("summary is {actual} tokens, over the {cap} token cap")]
    SummaryTooLong { actual: usize, cap: usize },

    #[error(
        "the untruncatable fields alone render to {actual} tokens, over the {cap} token cap; \
         the worker must resummarize"
    )]
    IrreducibleOverflow { actual: usize, cap: usize },

    #[error("a decision capsule must carry at least one decision")]
    DecisionWithoutClaims,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TruncationStep {
    OpenQuestions,
    Artifacts,
    Handoff,
    DecisionRationales,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedCapsule {
    pub text: String,
    pub tokens: usize,
    pub truncated: bool,
    pub steps_applied: Vec<TruncationStep>,
}

fn truncate_tokens(s: &str, cap: usize) -> String {
    if estimate_tokens(s) <= cap {
        return s.to_string();
    }
    let max_chars = (cap as f64 * crate::CHARS_PER_TOKEN) as usize;
    let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    while !out.is_empty() && !out.ends_with(char::is_whitespace) {
        out.pop();
    }
    let trimmed = out.trim_end();
    format!("{trimmed}…")
}

pub fn validate(c: &Capsule) -> Result<(), CapsuleError> {
    if c.v != 1 {
        return Err(CapsuleError::Version(c.v));
    }
    if c.from.trim().is_empty() {
        return Err(CapsuleError::MissingField("from"));
    }
    if c.task.trim().is_empty() {
        return Err(CapsuleError::MissingField("task"));
    }
    if c.summary.trim().is_empty() {
        return Err(CapsuleError::MissingField("summary"));
    }
    let summary_tokens = estimate_tokens(&c.summary);
    if summary_tokens > SUMMARY_TOKEN_CAP {
        return Err(CapsuleError::SummaryTooLong {
            actual: summary_tokens,
            cap: SUMMARY_TOKEN_CAP,
        });
    }
    if c.kind == CapsuleKind::Decision && c.decisions.is_empty() {
        return Err(CapsuleError::DecisionWithoutClaims);
    }
    Ok(())
}

fn render_parts(
    c: &Capsule,
    open_questions: &[String],
    artifacts: &[Artifact],
    dropped_artifacts: usize,
    handoff: &str,
    rationales: bool,
) -> String {
    let mut s = String::new();

    s.push_str(&format!("from: {}\n", c.from));
    s.push_str(&format!("task: {}\n", c.task));
    s.push_str(&format!("kind: {:?}\n", c.kind));
    s.push_str(&format!("outcome: {:?}\n", c.outcome));
    s.push_str("\nsummary:\n");
    s.push_str(c.summary.trim());
    s.push('\n');

    if !c.do_not_revisit.is_empty() {
        s.push_str("\ndo_not_revisit:\n");
        for d in &c.do_not_revisit {
            s.push_str(&format!("  - {d}\n"));
        }
    }

    if !c.decisions.is_empty() {
        s.push_str("\ndecisions:\n");
        for d in &c.decisions {
            s.push_str(&format!("  - claim: {}\n", d.claim));
            if rationales && !d.rationale.trim().is_empty() {
                s.push_str(&format!("    rationale: {}\n", d.rationale));
            }
        }
    }

    if !artifacts.is_empty() || dropped_artifacts > 0 {
        s.push_str("\nartifacts:\n");
        for a in artifacts {
            match &a.symbol {
                Some(sym) => {
                    s.push_str(&format!("  - {} {} ({:?})\n", a.path, sym, a.change))
                }
                None => s.push_str(&format!("  - {} ({:?})\n", a.path, a.change)),
            }
        }
        if dropped_artifacts > 0 {
            s.push_str(&format!("  - and {dropped_artifacts} more not listed\n"));
        }
    }

    if !open_questions.is_empty() {
        s.push_str("\nopen_questions:\n");
        for q in open_questions {
            s.push_str(&format!("  - {q}\n"));
        }
    }

    if !handoff.trim().is_empty() {
        s.push_str("\nhandoff:\n");
        s.push_str(handoff.trim());
        s.push('\n');
    }

    s
}

pub fn render(c: &Capsule) -> Result<RenderedCapsule, CapsuleError> {
    validate(c)?;

    let mut open_questions = c.open_questions.clone();
    let mut artifacts = c.artifacts.clone();
    let mut dropped_artifacts = 0usize;
    let mut handoff = c.handoff.clone();
    let mut rationales = true;
    let mut steps = Vec::new();

    let build = |oq: &[String], ar: &[Artifact], dropped: usize, ho: &str, rat: bool| {
        render_parts(c, oq, ar, dropped, ho, rat)
    };

    let mut text = build(&open_questions, &artifacts, dropped_artifacts, &handoff, rationales);

    if estimate_tokens(&text) > CAPSULE_TOKEN_CAP && open_questions.len() > OPEN_QUESTIONS_KEPT {
        open_questions.truncate(OPEN_QUESTIONS_KEPT);
        steps.push(TruncationStep::OpenQuestions);
        text = build(&open_questions, &artifacts, dropped_artifacts, &handoff, rationales);
    }

    if estimate_tokens(&text) > CAPSULE_TOKEN_CAP && artifacts.len() > ARTIFACTS_KEPT {
        dropped_artifacts = artifacts.len() - ARTIFACTS_KEPT;
        artifacts = artifacts.split_off(artifacts.len() - ARTIFACTS_KEPT);
        steps.push(TruncationStep::Artifacts);
        text = build(&open_questions, &artifacts, dropped_artifacts, &handoff, rationales);
    }

    if estimate_tokens(&text) > CAPSULE_TOKEN_CAP && estimate_tokens(&handoff) > HANDOFF_TOKEN_CAP {
        handoff = truncate_tokens(&handoff, HANDOFF_TOKEN_CAP);
        steps.push(TruncationStep::Handoff);
        text = build(&open_questions, &artifacts, dropped_artifacts, &handoff, rationales);
    }

    if estimate_tokens(&text) > CAPSULE_TOKEN_CAP
        && c.decisions.iter().any(|d| !d.rationale.trim().is_empty())
    {
        rationales = false;
        steps.push(TruncationStep::DecisionRationales);
        text = build(&open_questions, &artifacts, dropped_artifacts, &handoff, rationales);
    }

    let tokens = estimate_tokens(&text);
    if tokens > CAPSULE_TOKEN_CAP {
        return Err(CapsuleError::IrreducibleOverflow { actual: tokens, cap: CAPSULE_TOKEN_CAP });
    }

    Ok(RenderedCapsule {
        text,
        tokens,
        truncated: !steps.is_empty(),
        steps_applied: steps,
    })
}

pub fn turn_digest(c: &Capsule) -> String {
    let summary = c.summary.trim();
    let first_line = summary.lines().next().unwrap_or(summary);
    format!("{} [{:?}] {}", c.from, c.outcome, truncate_tokens(first_line, 40))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Capsule {
        Capsule {
            v: 1,
            kind: CapsuleKind::TaskReturn,
            from: "gameplay_engineer#7".into(),
            task: "task_01J".into(),
            summary: "Added a dash ability with a cooldown.".into(),
            outcome: CapsuleOutcome::Done,
            artifacts: vec![],
            decisions: vec![],
            open_questions: vec![],
            do_not_revisit: vec![],
            handoff: String::new(),
        }
    }

    fn filler(tokens: usize) -> String {
        "lorem ipsum dolor sit amet ".repeat(tokens / 5 + 1)
    }

    #[test]
    fn a_minimal_capsule_validates_and_renders() {
        let c = base();
        let r = render(&c).unwrap();
        assert!(!r.truncated);
        assert!(r.text.contains("Added a dash ability"));
        assert!(r.text.contains("gameplay_engineer#7"));
        assert!(r.tokens > 0);
    }

    #[test]
    fn rejects_a_future_schema_version() {
        let mut c = base();
        c.v = 2;
        assert_eq!(validate(&c).unwrap_err(), CapsuleError::Version(2));
    }

    #[test]
    fn requires_the_load_bearing_fields() {
        for (field, mutate) in [
            ("from", (|c: &mut Capsule| c.from = "  ".into()) as fn(&mut Capsule)),
            ("task", |c: &mut Capsule| c.task = String::new()),
            ("summary", |c: &mut Capsule| c.summary = "\n".into()),
        ] {
            let mut c = base();
            mutate(&mut c);
            assert_eq!(validate(&c).unwrap_err(), CapsuleError::MissingField(field));
        }
    }

    #[test]
    fn rejects_an_oversized_summary() {
        let mut c = base();
        c.summary = filler(SUMMARY_TOKEN_CAP + 200);
        assert!(matches!(
            validate(&c).unwrap_err(),
            CapsuleError::SummaryTooLong { .. }
        ));
    }

    #[test]
    fn a_decision_capsule_must_carry_a_claim() {
        let mut c = base();
        c.kind = CapsuleKind::Decision;
        assert_eq!(validate(&c).unwrap_err(), CapsuleError::DecisionWithoutClaims);

        c.decisions.push(DecisionClaim {
            claim: "Dash is implemented as a state machine".into(),
            rationale: "Composability".into(),
        });
        assert!(validate(&c).is_ok());
    }

    #[test]
    fn truncation_drops_open_questions_first() {
        let mut c = base();
        c.open_questions = (0..40).map(|i| format!("question {i} {}", filler(60))).collect();
        let r = render(&c).unwrap();
        assert!(r.truncated);
        assert_eq!(r.steps_applied[0], TruncationStep::OpenQuestions);
        assert!(r.tokens <= CAPSULE_TOKEN_CAP);
    }

    #[test]
    fn truncation_keeps_the_ten_most_recent_artifacts_and_counts_the_rest() {
        let mut c = base();
        c.artifacts = (0..60)
            .map(|i| Artifact {
                path: format!("src/System{i}_{}.cs", filler(40)),
                symbol: None,
                change: Change::Modified,
            })
            .collect();
        let r = render(&c).unwrap();
        assert!(r.steps_applied.contains(&TruncationStep::Artifacts));
        assert!(r.text.contains("and 50 more not listed"));
        assert!(r.text.contains("src/System59"), "the most recent artifact must survive");
        assert!(!r.text.contains("src/System0_"), "the oldest artifact should be dropped");
    }

    #[test]
    fn the_summary_and_outcome_are_never_truncated() {
        let mut c = base();
        c.summary = "A precise and load-bearing summary line.".into();
        c.open_questions = (0..80).map(|i| format!("q{i} {}", filler(80))).collect();
        c.artifacts = (0..80)
            .map(|i| Artifact { path: format!("f{i}{}", filler(40)), symbol: None, change: Change::Added })
            .collect();
        let r = render(&c).unwrap();
        assert!(r.text.contains("A precise and load-bearing summary line."));
        assert!(r.text.contains("outcome: Done"));
    }

    #[test]
    fn do_not_revisit_survives_every_truncation_step() {
        let mut c = base();
        c.do_not_revisit = vec!["the IJobParallelFor path cannot see the generated struct".into()];
        c.open_questions = (0..80).map(|i| format!("q{i} {}", filler(80))).collect();
        c.artifacts = (0..80)
            .map(|i| Artifact { path: format!("f{i}{}", filler(40)), symbol: None, change: Change::Added })
            .collect();
        c.handoff = filler(4000);
        c.decisions = vec![DecisionClaim { claim: "c".into(), rationale: filler(2000) }];

        let r = render(&c).unwrap();
        assert!(
            r.text.contains("IJobParallelFor"),
            "do_not_revisit is what stops the studio looping; it must never be dropped"
        );
    }

    #[test]
    fn truncation_follows_the_documented_order() {
        let mut c = base();
        c.open_questions = (0..40).map(|i| format!("q{i} {}", filler(80))).collect();
        c.artifacts = (0..40)
            .map(|i| Artifact { path: format!("f{i}{}", filler(60)), symbol: None, change: Change::Added })
            .collect();
        c.handoff = filler(3000);
        c.decisions = vec![DecisionClaim { claim: "keep me".into(), rationale: filler(3000) }];

        let r = render(&c).unwrap();
        let expected = [
            TruncationStep::OpenQuestions,
            TruncationStep::Artifacts,
            TruncationStep::Handoff,
            TruncationStep::DecisionRationales,
        ];
        let applied = &r.steps_applied;
        let mut last = 0;
        for step in applied {
            let pos = expected.iter().position(|e| e == step).unwrap();
            assert!(pos >= last, "steps applied out of documented order: {applied:?}");
            last = pos;
        }
        assert!(r.tokens <= CAPSULE_TOKEN_CAP);
    }

    #[test]
    fn dropping_rationales_keeps_the_claims() {
        let mut c = base();
        c.decisions = vec![DecisionClaim {
            claim: "Dash uses a state machine".into(),
            rationale: filler(5000),
        }];
        let r = render(&c).unwrap();
        assert!(r.steps_applied.contains(&TruncationStep::DecisionRationales));
        assert!(r.text.contains("Dash uses a state machine"));
        assert!(r.tokens <= CAPSULE_TOKEN_CAP);
    }

    #[test]
    fn an_irreducible_capsule_is_rejected_rather_than_silently_cut() {
        let mut c = base();
        c.do_not_revisit = (0..400).map(|i| format!("dead end {i}: {}", filler(60))).collect();
        assert!(matches!(
            render(&c).unwrap_err(),
            CapsuleError::IrreducibleOverflow { .. }
        ));
    }

    #[test]
    fn a_rendered_capsule_never_exceeds_the_cap() {
        let mut c = base();
        c.open_questions = (0..200).map(|i| format!("q{i} {}", filler(50))).collect();
        c.artifacts = (0..200)
            .map(|i| Artifact { path: format!("f{i}{}", filler(30)), symbol: Some("Sym".into()), change: Change::Modified })
            .collect();
        c.handoff = filler(6000);
        let r = render(&c).unwrap();
        assert!(r.tokens <= CAPSULE_TOKEN_CAP, "rendered {} tokens", r.tokens);
    }

    #[test]
    fn the_turn_digest_costs_no_model_tokens() {
        let c = base();
        let d = turn_digest(&c);
        assert!(d.contains("gameplay_engineer#7"));
        assert!(d.contains("Done"));
        assert!(d.contains("Added a dash ability"));
    }

    #[test]
    fn json_round_trips_through_the_wire_shape() {
        let raw = r#"{
            "v": 1, "kind": "task_return", "from": "artist#2", "task": "task_9",
            "summary": "Exported the sprite sheet.", "outcome": "needs_verify",
            "artifacts": [{"path":"art/hero.png","change":"added"}],
            "unknown_future_field": 42
        }"#;
        let c: Capsule = serde_json::from_str(raw).unwrap();
        assert_eq!(c.kind, CapsuleKind::TaskReturn);
        assert_eq!(c.outcome, CapsuleOutcome::NeedsVerify);
        assert_eq!(c.artifacts[0].change, Change::Added);
        assert!(validate(&c).is_ok());
    }
}
