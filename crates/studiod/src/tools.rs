use studio_context::{render, Capsule, CapsuleKind};
use studio_index::Index;
use studio_mcp::{DecisionHit, StudioTools, SymbolHit, ToolError};
use studio_store::{CapsuleRow, DecisionRow, Store};

const SYMBOL_LOOKUP_LIMIT: usize = 8;

pub struct StoreTools {
    store: Store,
    index: Option<Index>,
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
        Self { store, index: None, now, id, role, task, escalates_to }
    }

    pub fn with_index(mut self, index: Index) -> Self {
        self.index = Some(index);
        self
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

    fn symbol_lookup(&mut self, name: &str) -> Result<Vec<SymbolHit>, ToolError> {
        let Some(index) = &self.index else {
            return Err(ToolError::Rejected(
                "this worker was started without a code index, so symbol lookup is unavailable. \
                 Run `studiod index` in the project first."
                    .into(),
            ));
        };

        let records = index
            .lookup(name, SYMBOL_LOOKUP_LIMIT)
            .map_err(|e| ToolError::Rejected(format!("symbol lookup failed: {e}")))?;

        let mut hits = Vec::with_capacity(records.len());
        for record in records {
            let slice = index
                .slice(&record.fqname, &record.path)
                .map_err(|e| ToolError::Rejected(format!("symbol slice failed: {e}")))?;

            let (calls, called_by) = match slice {
                Some(s) => (s.calls, s.called_by),
                None => (Vec::new(), Vec::new()),
            };

            let mounted_at = index
                .scenes_using(&record.path)
                .map_err(|e| ToolError::Rejected(format!("scene lookup failed: {e}")))?
                .into_iter()
                .map(|use_site| match use_site.node_type {
                    Some(node_type) => {
                        format!("{} as {} ({})", use_site.asset, use_site.node_path, node_type)
                    }
                    None => format!("{} as {}", use_site.asset, use_site.node_path),
                })
                .collect();

            hits.push(SymbolHit {
                fqname: record.fqname,
                path: record.path,
                signature: record.signature,
                line_start: Some(record.line_start),
                line_end: Some(record.line_end),
                doc: record.doc,
                calls,
                called_by,
                mounted_at,
            });
        }

        Ok(hits)
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
