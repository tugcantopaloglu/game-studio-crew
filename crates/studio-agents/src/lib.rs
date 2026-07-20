pub mod layout;

pub use layout::{pack_floor, Desk, Floor, Room, TILE};

use serde::{Deserialize, Serialize};
use studio_context::Model;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Department {
    Leadership,
    Production,
    Design,
    Engineering,
    Art,
    Audio,
    Qa,
    Infra,
}

impl Department {
    pub fn id(&self) -> &'static str {
        match self {
            Department::Leadership => "leadership",
            Department::Production => "production",
            Department::Design => "design",
            Department::Engineering => "engineering",
            Department::Art => "art",
            Department::Audio => "audio",
            Department::Qa => "qa",
            Department::Infra => "infra",
        }
    }

    pub fn visual_family(&self) -> &'static str {
        match self {
            Department::Leadership | Department::Production => "leadership",
            Department::Qa | Department::Infra => "qa",
            Department::Design => "design",
            Department::Engineering => "engineering",
            Department::Art => "art",
            Department::Audio => "audio",
        }
    }

    pub const ALL: [Department; 8] = [
        Department::Leadership,
        Department::Production,
        Department::Design,
        Department::Engineering,
        Department::Art,
        Department::Audio,
        Department::Qa,
        Department::Infra,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effort {
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl Effort {
    pub fn as_str(&self) -> &'static str {
        match self {
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
            Effort::XHigh => "xhigh",
            Effort::Max => "max",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolClass {
    Coordination,
    Designer,
    Engineer,
    ArtAudio,
    Qa,
}

impl ToolClass {
    pub fn tools(&self) -> &'static [&'static str] {
        match self {
            ToolClass::Coordination => &[],
            ToolClass::Designer => &["Read", "Grep", "Glob", "Edit", "Write"],
            ToolClass::Engineer => &["Read", "Grep", "Glob", "Edit", "Write"],
            ToolClass::ArtAudio => &["Read", "Grep", "Glob", "Edit", "Write"],
            ToolClass::Qa => &["Read", "Grep", "Glob", "Edit", "Write"],
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Role {
    pub id: &'static str,
    pub title: &'static str,
    pub tier: u8,
    pub department: Department,
    pub model: Model,
    pub effort: Effort,
    pub escalates_to: Option<&'static str>,
    pub tool_class: ToolClass,
}

impl Role {
    pub fn tools(&self) -> Vec<String> {
        self.tool_class.tools().iter().map(|s| s.to_string()).collect()
    }

    pub fn escalates_to_human(&self) -> bool {
        self.escalates_to.is_none()
    }
}

pub const REGISTRY: [Role; 13] = [
    Role {
        id: "studio_director",
        title: "Studio Director",
        tier: 1,
        department: Department::Leadership,
        model: Model::Fable,
        effort: Effort::XHigh,
        escalates_to: None,
        tool_class: ToolClass::Coordination,
    },
    Role {
        id: "producer",
        title: "Producer",
        tier: 2,
        department: Department::Production,
        model: Model::Opus,
        effort: Effort::High,
        escalates_to: Some("studio_director"),
        tool_class: ToolClass::Coordination,
    },
    Role {
        id: "game_designer",
        title: "Game Designer",
        tier: 2,
        department: Department::Design,
        model: Model::Opus,
        effort: Effort::High,
        escalates_to: Some("producer"),
        tool_class: ToolClass::Designer,
    },
    Role {
        id: "systems_engineer",
        title: "Systems & Tools Engineer",
        tier: 2,
        department: Department::Engineering,
        model: Model::Opus,
        effort: Effort::XHigh,
        escalates_to: Some("studio_director"),
        tool_class: ToolClass::Engineer,
    },
    Role {
        id: "gameplay_engineer",
        title: "Gameplay Engineer",
        tier: 3,
        department: Department::Engineering,
        model: Model::Opus,
        effort: Effort::High,
        escalates_to: Some("systems_engineer"),
        tool_class: ToolClass::Engineer,
    },
    Role {
        id: "infra_engineer",
        title: "Build & Infra Engineer",
        tier: 3,
        department: Department::Infra,
        model: Model::Opus,
        effort: Effort::High,
        escalates_to: Some("systems_engineer"),
        tool_class: ToolClass::Engineer,
    },
    Role {
        id: "tech_artist",
        title: "Technical Artist",
        tier: 3,
        department: Department::Art,
        model: Model::Opus,
        effort: Effort::Medium,
        escalates_to: Some("systems_engineer"),
        tool_class: ToolClass::ArtAudio,
    },
    Role {
        id: "qa_engineer",
        title: "QA Engineer",
        tier: 3,
        department: Department::Qa,
        model: Model::Opus,
        effort: Effort::Medium,
        escalates_to: Some("producer"),
        tool_class: ToolClass::Qa,
    },
    Role {
        id: "level_designer",
        title: "Level Designer",
        tier: 3,
        department: Department::Design,
        model: Model::Opus,
        effort: Effort::Medium,
        escalates_to: Some("game_designer"),
        tool_class: ToolClass::Designer,
    },
    Role {
        id: "narrative_designer",
        title: "Narrative Designer",
        tier: 3,
        department: Department::Design,
        model: Model::Opus,
        effort: Effort::Medium,
        escalates_to: Some("game_designer"),
        tool_class: ToolClass::Designer,
    },
    Role {
        id: "ux_designer",
        title: "UX/UI Designer",
        tier: 3,
        department: Department::Design,
        model: Model::Opus,
        effort: Effort::Medium,
        escalates_to: Some("game_designer"),
        tool_class: ToolClass::Designer,
    },
    Role {
        id: "artist",
        title: "Artist",
        tier: 3,
        department: Department::Art,
        model: Model::Opus,
        effort: Effort::Low,
        escalates_to: Some("tech_artist"),
        tool_class: ToolClass::ArtAudio,
    },
    Role {
        id: "audio_designer",
        title: "Audio Designer",
        tier: 3,
        department: Department::Audio,
        model: Model::Opus,
        effort: Effort::Low,
        escalates_to: Some("game_designer"),
        tool_class: ToolClass::ArtAudio,
    },
];

pub fn role(id: &str) -> Option<&'static Role> {
    REGISTRY.iter().find(|r| r.id == id)
}

pub fn escalation_chain(id: &str) -> Vec<&'static str> {
    let mut chain = Vec::new();
    let mut current = id;
    while let Some(r) = role(current) {
        match r.escalates_to {
            Some(parent) => {
                chain.push(parent);
                current = parent;
            }
            None => break,
        }
        if chain.len() > REGISTRY.len() {
            break;
        }
    }
    chain
}

pub fn nearest_common_ancestor(a: &str, b: &str) -> Option<&'static str> {
    let chain_a: Vec<&str> = std::iter::once(a).chain(escalation_chain(a)).collect();
    let chain_b: Vec<&str> = std::iter::once(b).chain(escalation_chain(b)).collect();
    chain_a
        .iter()
        .find(|candidate| chain_b.contains(candidate))
        .and_then(|id| role(id).map(|r| r.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn the_registry_holds_exactly_thirteen_roles() {
        assert_eq!(REGISTRY.len(), 13);
    }

    #[test]
    fn every_role_id_is_unique() {
        let ids: HashSet<&str> = REGISTRY.iter().map(|r| r.id).collect();
        assert_eq!(ids.len(), REGISTRY.len());
    }

    #[test]
    fn exactly_one_seat_sits_on_fable_and_it_is_tier_one() {
        let fable: Vec<&Role> = REGISTRY.iter().filter(|r| r.model == Model::Fable).collect();
        assert_eq!(fable.len(), 1, "fable is twice the price; it stays on one low-volume seat");
        assert_eq!(fable[0].tier, 1);
        assert_eq!(fable[0].id, "studio_director");
    }

    #[test]
    fn only_the_director_escalates_to_a_human() {
        let roots: Vec<&Role> = REGISTRY.iter().filter(|r| r.escalates_to_human()).collect();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].id, "studio_director");
    }

    #[test]
    fn every_escalation_target_exists_in_the_registry() {
        for r in &REGISTRY {
            if let Some(parent) = r.escalates_to {
                assert!(role(parent).is_some(), "{} escalates to unknown {parent}", r.id);
            }
        }
    }

    #[test]
    fn every_escalation_chain_terminates_at_the_director() {
        for r in &REGISTRY {
            let chain = escalation_chain(r.id);
            if r.id == "studio_director" {
                assert!(chain.is_empty());
            } else {
                assert_eq!(
                    chain.last(),
                    Some(&"studio_director"),
                    "{} does not reach the director: {chain:?}",
                    r.id
                );
            }
        }
    }

    #[test]
    fn no_escalation_chain_loops() {
        for r in &REGISTRY {
            let chain = escalation_chain(r.id);
            let unique: HashSet<&&str> = chain.iter().collect();
            assert_eq!(unique.len(), chain.len(), "{} has a cycle: {chain:?}", r.id);
        }
    }

    #[test]
    fn arbitration_convenes_the_nearest_common_ancestor() {
        assert_eq!(
            nearest_common_ancestor("level_designer", "narrative_designer"),
            Some("game_designer")
        );
        assert_eq!(
            nearest_common_ancestor("gameplay_engineer", "artist"),
            Some("systems_engineer")
        );
        assert_eq!(
            nearest_common_ancestor("qa_engineer", "gameplay_engineer"),
            Some("studio_director")
        );
    }

    #[test]
    fn a_role_is_its_own_ancestor_when_paired_with_a_descendant() {
        assert_eq!(
            nearest_common_ancestor("game_designer", "level_designer"),
            Some("game_designer")
        );
    }

    #[test]
    fn coordination_roles_carry_no_filesystem_tools() {
        for id in ["studio_director", "producer"] {
            assert!(
                role(id).unwrap().tools().is_empty(),
                "{id} coordinates through capsules; tools would only cost prefix tokens"
            );
        }
    }

    #[test]
    fn working_roles_share_one_allowlist_so_the_cache_does_not_fragment() {
        let allowlists: HashSet<Vec<String>> = REGISTRY
            .iter()
            .map(|r| r.tools())
            .collect();
        assert_eq!(
            allowlists.len(),
            2,
            "the allowlist is part of the cache key; distinct lists mint distinct prefixes"
        );
    }

    #[test]
    fn every_department_label_maps_to_one_of_six_visual_families() {
        let families: HashSet<&str> =
            Department::ALL.iter().map(|d| d.visual_family()).collect();
        assert_eq!(families.len(), 6, "the floor renders six fills from eight labels");
    }

    #[test]
    fn tiers_are_only_one_two_or_three() {
        for r in &REGISTRY {
            assert!((1..=3).contains(&r.tier), "{} has tier {}", r.id, r.tier);
        }
    }
}
