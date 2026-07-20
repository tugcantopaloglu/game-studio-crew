pub mod exec;
pub use exec::{execute, Admission, GateOutcome, NodeOutcome, RunOutcome, RunReport, WorkflowHost, MAX_REPAIR_ROUNDS};

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub const FEATURE: &str = include_str!("../workflows/feature.toml");
pub const BUGFIX: &str = include_str!("../workflows/bugfix.toml");
pub const SPRINT_PLANNING: &str = include_str!("../workflows/sprint_planning.toml");
pub const RELEASE: &str = include_str!("../workflows/release.toml");

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum WorkflowError {
    #[error("parse failed: {0}")]
    Parse(String),

    #[error("unsupported schema_version {0}; this daemon speaks 1")]
    SchemaVersion(u32),

    #[error("node '{0}' is declared twice")]
    DuplicateNode(String),

    #[error("node '{node}' binds unknown role '{role}'")]
    UnknownRole { node: String, role: String },

    #[error("edge {from} -> {to} names unknown node '{missing}'")]
    UnknownEdgeNode { from: String, to: String, missing: String },

    #[error("gate after '{0}' names a node that does not exist")]
    UnknownGateNode(String),

    #[error("the graph has a cycle through {0}")]
    Cycle(String),

    #[error("node '{0}' lists an input that is not an upstream node")]
    UnreachableInput(String),

    #[error("a verify gate after '{0}' must name a scope")]
    GateWithoutScope(String),

    #[error("workflow has no nodes")]
    Empty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateKind {
    Verify,
    Approval,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnFail {
    Repair,
    Block,
    Escalate,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Node {
    pub id: String,
    pub role: String,
    #[serde(default)]
    pub inputs: Vec<String>,
    #[serde(default)]
    pub budget_tokens: u64,
    #[serde(default)]
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub carries: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Gate {
    pub after: String,
    pub kind: GateKind,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default = "default_on_fail")]
    pub on_fail: OnFail,
}

fn default_on_fail() -> OnFail {
    OnFail::Block
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Workflow {
    pub schema_version: u32,
    pub id: String,
    pub title: String,
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    #[serde(default)]
    pub gates: Vec<Gate>,
}

impl Workflow {
    pub fn parse(src: &str) -> Result<Self, WorkflowError> {
        let w: Workflow =
            toml::from_str(src).map_err(|e| WorkflowError::Parse(e.to_string()))?;
        w.validate()?;
        Ok(w)
    }

    pub fn builtin() -> Vec<Workflow> {
        [FEATURE, BUGFIX, SPRINT_PLANNING, RELEASE]
            .iter()
            .map(|s| Workflow::parse(s).expect("builtin workflow must parse"))
            .collect()
    }

    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn gates_after(&self, node: &str) -> Vec<&Gate> {
        self.gates.iter().filter(|g| g.after == node).collect()
    }

    pub fn total_budget(&self) -> u64 {
        self.nodes.iter().map(|n| n.budget_tokens).sum()
    }

    pub fn validate(&self) -> Result<(), WorkflowError> {
        if self.schema_version != 1 {
            return Err(WorkflowError::SchemaVersion(self.schema_version));
        }
        if self.nodes.is_empty() {
            return Err(WorkflowError::Empty);
        }

        let mut seen = BTreeSet::new();
        for n in &self.nodes {
            if !seen.insert(n.id.as_str()) {
                return Err(WorkflowError::DuplicateNode(n.id.clone()));
            }
            if studio_agents::role(&n.role).is_none() {
                return Err(WorkflowError::UnknownRole {
                    node: n.id.clone(),
                    role: n.role.clone(),
                });
            }
        }

        for e in &self.edges {
            for end in [&e.from, &e.to] {
                if !seen.contains(end.as_str()) {
                    return Err(WorkflowError::UnknownEdgeNode {
                        from: e.from.clone(),
                        to: e.to.clone(),
                        missing: end.clone(),
                    });
                }
            }
        }

        for g in &self.gates {
            if !seen.contains(g.after.as_str()) {
                return Err(WorkflowError::UnknownGateNode(g.after.clone()));
            }
            if g.kind == GateKind::Verify && g.scope.is_none() {
                return Err(WorkflowError::GateWithoutScope(g.after.clone()));
            }
            if let Some(scope) = &g.scope {
                let known = studio_engine::VerifyScope::ALL
                    .iter()
                    .any(|s| s.key() == scope);
                if !known {
                    return Err(WorkflowError::GateWithoutScope(g.after.clone()));
                }
            }
        }

        let order = self.topological_order()?;

        let mut upstream: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for id in &order {
            let mut set = BTreeSet::new();
            for e in self.edges.iter().filter(|e| &e.to == id) {
                set.insert(e.from.as_str());
                if let Some(parents) = upstream.get(e.from.as_str()) {
                    set.extend(parents.iter().copied());
                }
            }
            upstream.insert(id.as_str(), set);
        }

        for n in &self.nodes {
            for input in &n.inputs {
                let reachable = upstream
                    .get(n.id.as_str())
                    .map(|s| s.contains(input.as_str()))
                    .unwrap_or(false);
                if !reachable {
                    return Err(WorkflowError::UnreachableInput(n.id.clone()));
                }
            }
        }

        Ok(())
    }

    pub fn topological_order(&self) -> Result<Vec<String>, WorkflowError> {
        let mut indegree: BTreeMap<&str, usize> =
            self.nodes.iter().map(|n| (n.id.as_str(), 0)).collect();
        let mut out: BTreeMap<&str, Vec<&str>> = BTreeMap::new();

        for e in &self.edges {
            out.entry(e.from.as_str()).or_default().push(e.to.as_str());
            *indegree.entry(e.to.as_str()).or_insert(0) += 1;
        }

        let mut queue: VecDeque<&str> = self
            .nodes
            .iter()
            .filter(|n| indegree.get(n.id.as_str()) == Some(&0))
            .map(|n| n.id.as_str())
            .collect();

        let mut order = Vec::new();
        while let Some(id) = queue.pop_front() {
            order.push(id.to_string());
            for next in out.get(id).into_iter().flatten() {
                let d = indegree.get_mut(next).expect("edge target exists");
                *d -= 1;
                if *d == 0 {
                    queue.push_back(next);
                }
            }
        }

        if order.len() != self.nodes.len() {
            let stuck = self
                .nodes
                .iter()
                .find(|n| !order.contains(&n.id))
                .map(|n| n.id.clone())
                .unwrap_or_default();
            return Err(WorkflowError::Cycle(stuck));
        }
        Ok(order)
    }

    pub fn resume_from(&self, completed: &BTreeSet<String>) -> Result<Vec<String>, WorkflowError> {
        Ok(self
            .topological_order()?
            .into_iter()
            .filter(|id| !completed.contains(id))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(body: &str) -> String {
        format!("schema_version = 1\nid = \"t\"\ntitle = \"T\"\n{body}")
    }

    #[test]
    fn all_four_builtin_workflows_parse_and_validate() {
        let all = Workflow::builtin();
        assert_eq!(all.len(), 4);
        let ids: Vec<&str> = all.iter().map(|w| w.id.as_str()).collect();
        for want in ["feature", "bugfix", "sprint_planning", "release"] {
            assert!(ids.contains(&want), "missing workflow {want}");
        }
    }

    #[test]
    fn every_builtin_node_binds_a_real_role() {
        for w in Workflow::builtin() {
            for n in &w.nodes {
                assert!(
                    studio_agents::role(&n.role).is_some(),
                    "{}/{} binds unknown role {}",
                    w.id,
                    n.id,
                    n.role
                );
            }
        }
    }

    #[test]
    fn every_builtin_workflow_is_acyclic_and_ordered() {
        for w in Workflow::builtin() {
            let order = w.topological_order().unwrap();
            assert_eq!(order.len(), w.nodes.len(), "{} lost a node", w.id);
            for e in &w.edges {
                let a = order.iter().position(|x| x == &e.from).unwrap();
                let b = order.iter().position(|x| x == &e.to).unwrap();
                assert!(a < b, "{}: {} must run before {}", w.id, e.from, e.to);
            }
        }
    }

    #[test]
    fn the_four_workflows_have_distinct_gate_structures() {
        let shapes: BTreeSet<String> = Workflow::builtin()
            .iter()
            .map(|w| {
                let mut g: Vec<String> = w
                    .gates
                    .iter()
                    .map(|g| format!("{:?}:{}", g.kind, g.scope.clone().unwrap_or_default()))
                    .collect();
                g.sort();
                g.join(",")
            })
            .collect();
        assert_eq!(
            shapes.len(),
            4,
            "a workflow is justified by a new gate structure, not a new topic"
        );
    }

    #[test]
    fn bugfix_reproduces_before_it_fixes() {
        let w = Workflow::parse(BUGFIX).unwrap();
        let order = w.topological_order().unwrap();
        let triage = order.iter().position(|n| n == "triage").unwrap();
        let fix = order.iter().position(|n| n == "fix").unwrap();
        assert!(triage < fix);
        assert!(
            !w.gates_after("triage").is_empty(),
            "the reproduction gate is what makes bugfix its own workflow"
        );
    }

    #[test]
    fn sprint_planning_runs_no_engine_verification() {
        let w = Workflow::parse(SPRINT_PLANNING).unwrap();
        assert!(w.gates.iter().all(|g| g.kind == GateKind::Approval));
    }

    #[test]
    fn release_gates_on_an_export_and_a_director_approval() {
        let w = Workflow::parse(RELEASE).unwrap();
        assert!(w
            .gates
            .iter()
            .any(|g| g.scope.as_deref() == Some("export")));
        assert!(w.gates.iter().any(|g| g.kind == GateKind::Approval));
        assert!(w.nodes.iter().any(|n| n.role == "studio_director"));
    }

    #[test]
    fn a_cycle_is_rejected() {
        let s = src(
            "[[nodes]]\nid=\"a\"\nrole=\"producer\"\n\
             [[nodes]]\nid=\"b\"\nrole=\"producer\"\n\
             [[edges]]\nfrom=\"a\"\nto=\"b\"\ncarries=\"task_return\"\n\
             [[edges]]\nfrom=\"b\"\nto=\"a\"\ncarries=\"task_return\"\n",
        );
        assert!(matches!(
            Workflow::parse(&s).unwrap_err(),
            WorkflowError::Cycle(_)
        ));
    }

    #[test]
    fn an_unknown_role_is_rejected_at_parse_time() {
        let s = src("[[nodes]]\nid=\"a\"\nrole=\"chief_vibes_officer\"\n");
        assert!(matches!(
            Workflow::parse(&s).unwrap_err(),
            WorkflowError::UnknownRole { .. }
        ));
    }

    #[test]
    fn an_edge_to_nowhere_is_rejected() {
        let s = src(
            "[[nodes]]\nid=\"a\"\nrole=\"producer\"\n\
             [[edges]]\nfrom=\"a\"\nto=\"ghost\"\ncarries=\"task_return\"\n",
        );
        assert!(matches!(
            Workflow::parse(&s).unwrap_err(),
            WorkflowError::UnknownEdgeNode { .. }
        ));
    }

    #[test]
    fn a_verify_gate_without_a_scope_is_rejected() {
        let s = src(
            "[[nodes]]\nid=\"a\"\nrole=\"producer\"\n\
             [[gates]]\nafter=\"a\"\nkind=\"verify\"\n",
        );
        assert!(matches!(
            Workflow::parse(&s).unwrap_err(),
            WorkflowError::GateWithoutScope(_)
        ));
    }

    #[test]
    fn a_verify_gate_naming_an_unknown_scope_is_rejected() {
        let s = src(
            "[[nodes]]\nid=\"a\"\nrole=\"producer\"\n\
             [[gates]]\nafter=\"a\"\nkind=\"verify\"\nscope=\"vibe_check\"\n",
        );
        assert!(matches!(
            Workflow::parse(&s).unwrap_err(),
            WorkflowError::GateWithoutScope(_)
        ));
    }

    #[test]
    fn a_node_cannot_consume_a_capsule_from_a_node_that_never_runs_before_it() {
        let s = src(
            "[[nodes]]\nid=\"a\"\nrole=\"producer\"\n\
             [[nodes]]\nid=\"b\"\nrole=\"producer\"\ninputs=[\"a\"]\n",
        );
        assert!(matches!(
            Workflow::parse(&s).unwrap_err(),
            WorkflowError::UnreachableInput(_)
        ));
    }

    #[test]
    fn duplicate_node_ids_are_rejected() {
        let s = src(
            "[[nodes]]\nid=\"a\"\nrole=\"producer\"\n\
             [[nodes]]\nid=\"a\"\nrole=\"producer\"\n",
        );
        assert!(matches!(
            Workflow::parse(&s).unwrap_err(),
            WorkflowError::DuplicateNode(_)
        ));
    }

    #[test]
    fn a_future_schema_version_is_refused() {
        let s = "schema_version = 2\nid=\"t\"\ntitle=\"T\"\n[[nodes]]\nid=\"a\"\nrole=\"producer\"\n";
        assert!(matches!(
            Workflow::parse(s).unwrap_err(),
            WorkflowError::SchemaVersion(2)
        ));
    }

    #[test]
    fn resuming_skips_the_nodes_that_already_returned_capsules() {
        let w = Workflow::parse(FEATURE).unwrap();
        let done: BTreeSet<String> = ["design".to_string()].into_iter().collect();
        let rest = w.resume_from(&done).unwrap();
        assert!(!rest.contains(&"design".to_string()));
        assert_eq!(rest.len(), w.nodes.len() - 1);
        assert_eq!(rest.first().map(String::as_str), Some("implement"));
    }

    #[test]
    fn resuming_a_finished_workflow_leaves_nothing_to_do() {
        let w = Workflow::parse(FEATURE).unwrap();
        let all: BTreeSet<String> = w.nodes.iter().map(|n| n.id.clone()).collect();
        assert!(w.resume_from(&all).unwrap().is_empty());
    }

    #[test]
    fn every_workflow_declares_a_budget_for_every_node() {
        for w in Workflow::builtin() {
            for n in &w.nodes {
                assert!(n.budget_tokens > 0, "{}/{} has no budget", w.id, n.id);
            }
            assert!(w.total_budget() > 0);
        }
    }
}
