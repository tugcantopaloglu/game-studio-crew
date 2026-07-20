use crate::{Edge, Node, Workflow, WorkflowError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const DEFAULT_NODE_BUDGET: u64 = 120_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanTask {
    pub id: String,
    pub role: String,
    pub brief: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub tasks: Vec<PlanTask>,
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum PlanError {
    #[error("the plan has no tasks")]
    Empty,

    #[error("the planner did not return usable json: {0}")]
    NotJson(String),

    #[error("the plan has {0} tasks; a single prompt should not fan out past {1}")]
    TooLarge(usize, usize),

    #[error("task '{0}' is declared twice")]
    DuplicateTask(String),

    #[error("task '{task}' names role '{role}', which is not in the registry")]
    UnknownRole { task: String, role: String },

    #[error("task '{task}' depends on '{missing}', which is not in the plan")]
    UnknownDependency { task: String, missing: String },

    #[error("task '{0}' depends on itself")]
    SelfDependency(String),

    #[error("task '{0}' has an empty brief")]
    EmptyBrief(String),

    #[error("the plan is cyclic through '{0}'")]
    Cycle(String),
}

pub const MAX_TASKS: usize = 12;

pub fn plan_schema() -> serde_json::Value {
    let roles: Vec<&str> = studio_agents::REGISTRY.iter().map(|r| r.id).collect();
    serde_json::json!({
        "type": "object",
        "properties": {
            "title": {
                "type": "string",
                "description": "A short name for what is being built."
            },
            "tasks": {
                "type": "array",
                "minItems": 1,
                "maxItems": MAX_TASKS,
                "items": {
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "string",
                            "description": "Short unique id, for example t1."
                        },
                        "role": {
                            "type": "string",
                            "enum": roles,
                            "description": "Which studio role does this task."
                        },
                        "brief": {
                            "type": "string",
                            "description": "What this role must do, in enough detail that it needs no further decisions."
                        },
                        "depends_on": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Ids of tasks whose capsules this task needs."
                        }
                    },
                    "required": ["id", "role", "brief", "depends_on"]
                }
            }
        },
        "required": ["title", "tasks"]
    })
}

impl Plan {
    pub fn parse(json: &str) -> Result<Self, PlanError> {
        let plan: Plan = serde_json::from_str(json)
            .map_err(|e| PlanError::NotJson(format!("{e}; got: {}", json.chars().take(160).collect::<String>())))?;
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), PlanError> {
        if self.tasks.is_empty() {
            return Err(PlanError::Empty);
        }
        if self.tasks.len() > MAX_TASKS {
            return Err(PlanError::TooLarge(self.tasks.len(), MAX_TASKS));
        }

        let mut ids = BTreeSet::new();
        for t in &self.tasks {
            if !ids.insert(t.id.as_str()) {
                return Err(PlanError::DuplicateTask(t.id.clone()));
            }
            if studio_agents::role(&t.role).is_none() {
                return Err(PlanError::UnknownRole {
                    task: t.id.clone(),
                    role: t.role.clone(),
                });
            }
            if t.brief.trim().is_empty() {
                return Err(PlanError::EmptyBrief(t.id.clone()));
            }
        }

        for t in &self.tasks {
            for dep in &t.depends_on {
                if dep == &t.id {
                    return Err(PlanError::SelfDependency(t.id.clone()));
                }
                if !ids.contains(dep.as_str()) {
                    return Err(PlanError::UnknownDependency {
                        task: t.id.clone(),
                        missing: dep.clone(),
                    });
                }
            }
        }

        self.to_workflow().map_err(|e| match e {
            WorkflowError::Cycle(n) => PlanError::Cycle(n),
            _ => PlanError::Empty,
        })?;

        Ok(())
    }

    pub fn to_workflow(&self) -> Result<Workflow, WorkflowError> {
        let nodes: Vec<Node> = self
            .tasks
            .iter()
            .map(|t| Node {
                id: t.id.clone(),
                role: t.role.clone(),
                inputs: t.depends_on.clone(),
                budget_tokens: DEFAULT_NODE_BUDGET,
                optional: false,
            })
            .collect();

        let edges: Vec<Edge> = self
            .tasks
            .iter()
            .flat_map(|t| {
                t.depends_on.iter().map(move |d| Edge {
                    from: d.clone(),
                    to: t.id.clone(),
                    carries: "task_return".to_string(),
                })
            })
            .collect();

        let wf = Workflow {
            schema_version: 1,
            id: "planned".to_string(),
            title: if self.title.trim().is_empty() {
                "Planned run".to_string()
            } else {
                self.title.clone()
            },
            nodes,
            edges,
            gates: Vec::new(),
        };
        wf.validate()?;
        Ok(wf)
    }

    pub fn brief_for(&self, id: &str) -> Option<&str> {
        self.tasks.iter().find(|t| t.id == id).map(|t| t.brief.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, role: &str, deps: &[&str]) -> PlanTask {
        PlanTask {
            id: id.into(),
            role: role.into(),
            brief: format!("do {id}"),
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn plan(tasks: Vec<PlanTask>) -> Plan {
        Plan { tasks, title: "Snake".into() }
    }

    #[test]
    fn a_linear_plan_becomes_a_runnable_workflow() {
        let p = plan(vec![
            task("t1", "game_designer", &[]),
            task("t2", "gameplay_engineer", &["t1"]),
            task("t3", "qa_engineer", &["t2"]),
        ]);
        let wf = p.to_workflow().unwrap();
        let order = wf.topological_order().unwrap();
        assert_eq!(order, vec!["t1", "t2", "t3"]);
        assert_eq!(wf.title, "Snake");
    }

    #[test]
    fn a_fan_out_plan_keeps_both_branches() {
        let p = plan(vec![
            task("design", "game_designer", &[]),
            task("code", "gameplay_engineer", &["design"]),
            task("art", "artist", &["design"]),
            task("test", "qa_engineer", &["code", "art"]),
        ]);
        let wf = p.to_workflow().unwrap();
        let order = wf.topological_order().unwrap();
        let pos = |id: &str| order.iter().position(|x| x == id).unwrap();
        assert!(pos("design") < pos("code"));
        assert!(pos("design") < pos("art"));
        assert!(pos("code") < pos("test"));
        assert!(pos("art") < pos("test"));
    }

    #[test]
    fn the_schema_only_offers_roles_that_exist() {
        let schema = plan_schema();
        let roles = schema["properties"]["tasks"]["items"]["properties"]["role"]["enum"]
            .as_array()
            .unwrap();
        assert_eq!(roles.len(), studio_agents::REGISTRY.len());
        for r in roles {
            assert!(studio_agents::role(r.as_str().unwrap()).is_some());
        }
    }

    #[test]
    fn the_schema_caps_how_far_one_prompt_can_fan_out() {
        let schema = plan_schema();
        assert_eq!(
            schema["properties"]["tasks"]["maxItems"].as_u64().unwrap() as usize,
            MAX_TASKS
        );
    }

    #[test]
    fn an_invented_role_is_rejected() {
        let p = plan(vec![task("t1", "chief_vibes_officer", &[])]);
        assert!(matches!(p.validate().unwrap_err(), PlanError::UnknownRole { .. }));
    }

    #[test]
    fn a_dependency_on_a_task_that_does_not_exist_is_rejected() {
        let p = plan(vec![task("t1", "game_designer", &["ghost"])]);
        assert!(matches!(
            p.validate().unwrap_err(),
            PlanError::UnknownDependency { .. }
        ));
    }

    #[test]
    fn a_cyclic_plan_is_rejected() {
        let p = plan(vec![
            task("a", "game_designer", &["b"]),
            task("b", "gameplay_engineer", &["a"]),
        ]);
        assert!(matches!(p.validate().unwrap_err(), PlanError::Cycle(_)));
    }

    #[test]
    fn a_task_depending_on_itself_is_rejected() {
        let p = plan(vec![task("a", "game_designer", &["a"])]);
        assert_eq!(p.validate().unwrap_err(), PlanError::SelfDependency("a".into()));
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let p = plan(vec![
            task("a", "game_designer", &[]),
            task("a", "qa_engineer", &[]),
        ]);
        assert_eq!(p.validate().unwrap_err(), PlanError::DuplicateTask("a".into()));
    }

    #[test]
    fn an_empty_brief_is_rejected() {
        let mut t = task("a", "game_designer", &[]);
        t.brief = "   ".into();
        assert_eq!(plan(vec![t]).validate().unwrap_err(), PlanError::EmptyBrief("a".into()));
    }

    #[test]
    fn an_empty_plan_is_rejected() {
        assert_eq!(plan(vec![]).validate().unwrap_err(), PlanError::Empty);
    }

    #[test]
    fn a_runaway_plan_is_rejected_rather_than_spawning_forty_workers() {
        let tasks: Vec<PlanTask> = (0..MAX_TASKS + 5)
            .map(|i| task(&format!("t{i}"), "gameplay_engineer", &[]))
            .collect();
        assert!(matches!(
            plan(tasks).validate().unwrap_err(),
            PlanError::TooLarge(_, _)
        ));
    }

    #[test]
    fn a_real_planner_response_parses_and_validates() {
        let json = r#"{
          "title": "Snake",
          "tasks": [
            {"id":"t1","role":"game_designer","brief":"Write the snake spec.","depends_on":[]},
            {"id":"t2","role":"gameplay_engineer","brief":"Implement it.","depends_on":["t1"]},
            {"id":"t3","role":"qa_engineer","brief":"Write tests.","depends_on":["t2"]}
          ]
        }"#;
        let p = Plan::parse(json).unwrap();
        assert_eq!(p.tasks.len(), 3);
        assert_eq!(p.title, "Snake");
        assert_eq!(p.brief_for("t2"), Some("Implement it."));
        assert!(p.to_workflow().is_ok());
    }

    #[test]
    fn a_planned_workflow_carries_no_engine_gates() {
        let p = plan(vec![task("t1", "game_designer", &[])]);
        let wf = p.to_workflow().unwrap();
        assert!(
            wf.gates.is_empty(),
            "a planned run gates on nothing until the plan says which engine it targets"
        );
    }

    #[test]
    fn every_task_gets_a_budget() {
        let p = plan(vec![task("t1", "game_designer", &[]), task("t2", "artist", &["t1"])]);
        let wf = p.to_workflow().unwrap();
        assert!(wf.nodes.iter().all(|n| n.budget_tokens > 0));
        assert_eq!(wf.total_budget(), DEFAULT_NODE_BUDGET * 2);
    }
}
