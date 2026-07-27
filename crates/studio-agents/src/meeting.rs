use serde::Deserialize;

pub const MAX_DISSENTS: usize = 8;
pub const CLAIM_CHARS: usize = 400;
pub const RATIONALE_CHARS: usize = 1200;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Decision {
    pub claim: String,
    pub rationale: String,
    #[serde(default)]
    pub dissent: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionError {
    NotJson(String),
    EmptyClaim,
    EmptyRationale,
}

impl std::fmt::Display for DecisionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecisionError::NotJson(why) => {
                write!(f, "the chair did not return a decision object: {why}")
            }
            DecisionError::EmptyClaim => write!(f, "the chair recorded no decision"),
            DecisionError::EmptyRationale => write!(f, "the chair gave no reason for the decision"),
        }
    }
}

impl std::error::Error for DecisionError {}

impl Decision {
    pub fn parse(raw: &str) -> Result<Self, DecisionError> {
        let mut d: Decision = serde_json::from_str(json_body(raw))
            .map_err(|e| DecisionError::NotJson(e.to_string()))?;

        d.claim = clamp(&d.claim, CLAIM_CHARS);
        d.rationale = clamp(&d.rationale, RATIONALE_CHARS);
        d.dissent = d
            .dissent
            .iter()
            .map(|s| clamp(s, CLAIM_CHARS))
            .filter(|s| !s.is_empty())
            .take(MAX_DISSENTS)
            .collect();

        if d.claim.is_empty() {
            return Err(DecisionError::EmptyClaim);
        }
        if d.rationale.is_empty() {
            return Err(DecisionError::EmptyRationale);
        }
        Ok(d)
    }
}

fn clamp(s: &str, max: usize) -> String {
    s.trim().chars().take(max).collect::<String>().trim_end().to_string()
}

fn json_body(raw: &str) -> &str {
    let t = raw.trim();
    if t.starts_with('{') {
        return t;
    }
    match (t.find('{'), t.rfind('}')) {
        (Some(a), Some(b)) if b > a => &t[a..=b],
        _ => t,
    }
}

pub fn decision_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "claim": {
                "type": "string",
                "description": "The decision, stated in one sentence as a rule the studio will follow. Not a summary of the discussion."
            },
            "rationale": {
                "type": "string",
                "description": "Why this decision and not the alternative the room raised, in one or two sentences."
            },
            "dissent": {
                "type": "array",
                "items": { "type": "string" },
                "maxItems": MAX_DISSENTS,
                "description": "Positions from the room that this decision overrules, each naming the role that held it. Empty if the room agreed."
            }
        },
        "required": ["claim", "rationale", "dissent"]
    })
}

pub const SLUG_CHARS: usize = 48;

pub fn slug(title: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;

    for c in title.chars().flat_map(|c| c.to_lowercase()) {
        if !c.is_ascii_alphanumeric() {
            pending_dash = !out.is_empty();
            continue;
        }
        let width = if pending_dash { 2 } else { 1 };
        if out.len() + width > SLUG_CHARS {
            break;
        }
        if pending_dash {
            out.push('-');
            pending_dash = false;
        }
        out.push(c);
    }

    if out.is_empty() {
        "decision".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_decision_object_parses() {
        let d = Decision::parse(
            r#"{"claim":"Dash is a state machine","rationale":"Coroutines drop frames","dissent":[]}"#,
        )
        .unwrap();
        assert_eq!(d.claim, "Dash is a state machine");
        assert_eq!(d.rationale, "Coroutines drop frames");
        assert!(d.dissent.is_empty());
    }

    #[test]
    fn prose_wrapped_around_the_object_is_stripped() {
        let d = Decision::parse(
            "Here is the decision:\n{\"claim\":\"c\",\"rationale\":\"r\",\"dissent\":[]}\nDone.",
        )
        .unwrap();
        assert_eq!(d.claim, "c");
    }

    #[test]
    fn a_missing_dissent_field_is_not_a_failure() {
        let d = Decision::parse(r#"{"claim":"c","rationale":"r"}"#).unwrap();
        assert!(d.dissent.is_empty());
    }

    #[test]
    fn a_chair_that_decided_nothing_is_rejected() {
        assert_eq!(
            Decision::parse(r#"{"claim":"   ","rationale":"r"}"#),
            Err(DecisionError::EmptyClaim)
        );
    }

    #[test]
    fn a_decision_with_no_reason_is_rejected() {
        assert_eq!(
            Decision::parse(r#"{"claim":"c","rationale":""}"#),
            Err(DecisionError::EmptyRationale)
        );
    }

    #[test]
    fn a_chair_that_answered_in_prose_is_rejected_not_guessed_at() {
        assert!(matches!(
            Decision::parse("We will use a state machine."),
            Err(DecisionError::NotJson(_))
        ));
    }

    #[test]
    fn blank_dissent_entries_are_dropped_rather_than_stored() {
        let d = Decision::parse(
            r#"{"claim":"c","rationale":"r","dissent":["qa_engineer wanted a test first","  ",""]}"#,
        )
        .unwrap();
        assert_eq!(d.dissent, vec!["qa_engineer wanted a test first"]);
    }

    #[test]
    fn a_runaway_chair_cannot_write_an_unbounded_row() {
        let long = "x".repeat(5_000);
        let raw = format!(
            r#"{{"claim":"{long}","rationale":"{long}","dissent":{}}}"#,
            serde_json::to_string(&vec![long.clone(); 40]).unwrap()
        );
        let d = Decision::parse(&raw).unwrap();
        assert_eq!(d.claim.chars().count(), CLAIM_CHARS);
        assert_eq!(d.rationale.chars().count(), RATIONALE_CHARS);
        assert_eq!(d.dissent.len(), MAX_DISSENTS);
    }

    #[test]
    fn the_schema_demands_every_field_so_the_cli_cannot_return_a_half_decision() {
        let s = decision_schema();
        let required: Vec<&str> =
            s["required"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(required, vec!["claim", "rationale", "dissent"]);
    }

    #[test]
    fn a_topic_becomes_a_filesystem_safe_slug() {
        assert_eq!(slug("Should dash cancel attacks?"), "should-dash-cancel-attacks");
        assert_eq!(slug("  Audio: bus layout  "), "audio-bus-layout");
    }

    #[test]
    fn a_topic_with_nothing_sluggable_still_yields_a_filename() {
        assert_eq!(slug("???"), "decision");
        assert_eq!(slug(""), "decision");
    }

    #[test]
    fn a_slug_stays_short_enough_for_a_path() {
        for title in ["word ".repeat(60), "w".repeat(200), "a-".repeat(90)] {
            let s = slug(&title);
            assert!(s.len() <= SLUG_CHARS, "{title:?} produced {} chars", s.len());
            assert!(!s.ends_with('-'), "a truncated slug must not end mid-separator: {s}");
        }
    }

    #[test]
    fn a_slug_is_always_a_safe_filename_fragment() {
        for title in ["Should dash cancel?", "  ", "??!", "Ünïcödé topic", "a/b\\c:d"] {
            let s = slug(title);
            assert!(!s.is_empty());
            assert!(
                s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{title:?} produced {s:?}"
            );
        }
    }
}
