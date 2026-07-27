use anyhow::{Context, Result};
use serde::Deserialize;
use studio_agents::role;
use studio_events::{EventType, Scene};
use studio_server::games::{record_summary, summary_state, Mechanic, SummaryState};
use studio_server::SummarizeRequest;

use crate::m4::Emitter;

const MAX_MECHANICS: usize = 6;
const MAX_SUMMARY_CHARS: usize = 900;

#[derive(Debug, Deserialize)]
struct Glimpse {
    summary: String,
    #[serde(default)]
    mechanics: Vec<Mechanic>,
}

pub fn summarize(em: &Emitter, req: &SummarizeRequest, seq: &mut usize) -> Result<()> {
    let root = em
        .project
        .clone()
        .context("summarizing needs a project; pick a game first")?;

    if let SummaryState::Fresh(cached) = summary_state(&root) {
        println!("  summary: already current, nothing to bill");
        return announce(em, req, &cached.text, &cached.mechanics, true);
    }

    let survey = crate::survey::survey(&root).with_context(|| {
        format!("{} has no files in it yet, so there is nothing to read", root.display())
    })?;

    let designer =
        role("game_designer").context("the game designer is missing from the registry")?;
    let brief = format!(
        "Here is a survey of a game, taken straight from its files:\n{survey}\n\n\
         Say in three or four sentences what this game is and how it plays, using only \
         the evidence above. Then name at most {MAX_MECHANICS} mechanics the crew can point \
         at, each with one clause on what it does, named the way the code names them. \
         Do not describe the folder layout, and do not invent features the files do not show."
    );

    *seq += 1;
    let metered = crate::m4::run_worker_metered_json(em, designer, &brief, *seq, schema())?;
    let glimpse = parse(&metered.text)?;

    let mut mechanics = glimpse.mechanics;
    mechanics.truncate(MAX_MECHANICS);
    let text: String = glimpse.summary.trim().chars().take(MAX_SUMMARY_CHARS).collect();

    let stored = record_summary(&root, &text, mechanics)
        .with_context(|| format!("could not cache the summary in {}", root.display()))?;

    println!(
        "  summary: {} mechanic(s), {} tokens, cached until the game changes",
        stored.mechanics.len(),
        metered.billed_tokens
    );
    announce(em, req, &stored.text, &stored.mechanics, false)
}

fn announce(
    em: &Emitter,
    req: &SummarizeRequest,
    summary: &str,
    mechanics: &[Mechanic],
    cached: bool,
) -> Result<()> {
    em.emit(
        "daemon",
        EventType::GameSummarized,
        Scene::daemon(),
        serde_json::json!({
            "project": req.project,
            "summary": summary,
            "mechanics": mechanics,
            "cached": cached,
        }),
    )
}

fn schema() -> String {
    serde_json::json!({
        "type": "object",
        "properties": {
            "summary": {
                "type": "string",
                "description": "Three or four sentences on what the game is and how it plays."
            },
            "mechanics": {
                "type": "array",
                "minItems": 1,
                "maxItems": MAX_MECHANICS,
                "items": {
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "The mechanic, named the way the code names it."
                        },
                        "note": {
                            "type": "string",
                            "description": "One clause on what it does."
                        }
                    },
                    "required": ["name", "note"]
                }
            }
        },
        "required": ["summary", "mechanics"]
    })
    .to_string()
}

fn parse(raw: &str) -> Result<Glimpse> {
    let body = match (raw.find('{'), raw.rfind('}')) {
        (Some(a), Some(b)) if b > a => &raw[a..=b],
        _ => raw,
    };
    let glimpse: Glimpse = serde_json::from_str(body).map_err(|e| {
        anyhow::anyhow!(
            "the designer did not return a summary I can store ({e}): {}",
            raw.lines().next().unwrap_or("no output")
        )
    })?;
    if glimpse.summary.trim().is_empty() {
        anyhow::bail!("the designer returned an empty summary, so nothing was cached");
    }
    Ok(glimpse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fenced_reply_still_parses_into_a_summary_and_its_mechanics() {
        let raw = "```json\n{\"summary\":\"A tiny runner.\",\
                   \"mechanics\":[{\"name\":\"dash\",\"note\":\"a short burst of speed\"}]}\n```";
        let glimpse = parse(raw).unwrap();
        assert_eq!(glimpse.summary, "A tiny runner.");
        assert_eq!(glimpse.mechanics[0].name, "dash");
    }

    #[test]
    fn a_reply_with_no_summary_is_refused_rather_than_cached_empty() {
        assert!(parse(r#"{"summary":"   ","mechanics":[]}"#).is_err());
        assert!(parse("I could not read the game").is_err());
    }

    #[test]
    fn the_schema_holds_the_designer_to_a_glimpse() {
        let schema: serde_json::Value = serde_json::from_str(&schema()).unwrap();
        assert_eq!(schema["properties"]["mechanics"]["maxItems"], MAX_MECHANICS);
        assert_eq!(schema["required"], serde_json::json!(["summary", "mechanics"]));
    }
}
