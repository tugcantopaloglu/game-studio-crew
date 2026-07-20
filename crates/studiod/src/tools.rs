use studio_context::{render, Capsule, CapsuleKind};
use studio_mcp::{DecisionHit, StudioTools, SymbolHit, ToolError};
use studio_store::{CapsuleRow, DecisionRow, Store};

pub struct StoreTools {
    store: Store,
    now: fn() -> String,
    id: fn(&str) -> String,
    role: String,
    task: String,
    escalates_to: String,
}

impl StoreTools {
    pub fn new(
        store: Store,
        now: fn() -> String,
        id: fn(&str) -> String,
        role: String,
        task: String,
        escalates_to: String,
    ) -> Self {
        Self { store, now, id, role, task, escalates_to }
    }
}

impl StudioTools for StoreTools {
    fn capsule_submit(&mut self, capsule: Capsule) -> Result<String, ToolError> {
        if !self.task.is_empty() && capsule.task != self.task {
            return Err(ToolError::Rejected(format!(
                "this worker was spawned for task {}, but the capsule names {}. \
                 A worker may only return the task it was given.",
                self.task, capsule.task
            )));
        }

        let rendered = render(&capsule).map_err(|e| ToolError::Rejected(e.to_string()))?;

        let capsule_id = (self.id)("cap");
        let body = serde_json::to_string(&capsule)
            .map_err(|e| ToolError::Rejected(format!("capsule is not serializable: {e}")))?;

        self.store
            .insert_capsule(
                CapsuleRow {
                    id: capsule_id.clone(),
                    task: capsule.task.clone(),
                    kind: wire_tag(&capsule.kind),
                    from_role: self.role.clone(),
                    outcome: wire_tag(&capsule.outcome),
                    rendered_tokens: rendered.tokens,
                    truncated: rendered.truncated,
                    body_json: body,
                },
                (self.now)(),
            )
            .map_err(|e| ToolError::Rejected(format!("store rejected the capsule: {e}")))?;

        if capsule.kind == CapsuleKind::Decision {
            for d in &capsule.decisions {
                let _ = self.store.insert_decision(
                    DecisionRow {
                        id: (self.id)("adr"),
                        title: capsule.summary.lines().next().unwrap_or("decision").to_string(),
                        claim: d.claim.clone(),
                        rationale: d.rationale.clone(),
                        origin_capsule: Some(capsule_id.clone()),
                        supersedes: None,
                    },
                    (self.now)(),
                );
            }
        }

        Ok(capsule_id)
    }

    fn decision_search(&mut self, query: &str, limit: usize) -> Result<Vec<DecisionHit>, ToolError> {
        let rows = self
            .store
            .search_decisions(query, limit)
            .map_err(|e| ToolError::Rejected(format!("decision search failed: {e}")))?;
        Ok(rows
            .into_iter()
            .map(|r| DecisionHit { id: r.id, title: r.title, claim: r.claim })
            .collect())
    }

    fn symbol_lookup(&mut self, _name: &str) -> Result<Vec<SymbolHit>, ToolError> {
        Ok(Vec::new())
    }

    fn escalate(&mut self, _reason: &str, _capsule: Option<Capsule>) -> Result<String, ToolError> {
        Ok(self.escalates_to.clone())
    }

    fn request_meeting(&mut self, _kind: &str, _participants: Vec<String>) -> Result<String, ToolError> {
        Ok((self.id)("meeting"))
    }
}

fn wire_tag<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|x| x.as_str().map(str::to_string))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::wire_tag;
    use studio_context::{CapsuleKind, CapsuleOutcome};

    #[test]
    fn wire_tags_are_snake_case_not_debug_formatting() {
        assert_eq!(wire_tag(&CapsuleKind::TaskReturn), "task_return");
        assert_eq!(wire_tag(&CapsuleKind::ConsultAnswer), "consult_answer");
        assert_eq!(wire_tag(&CapsuleOutcome::NeedsVerify), "needs_verify");
        assert_eq!(wire_tag(&CapsuleOutcome::Done), "done");
    }
}
