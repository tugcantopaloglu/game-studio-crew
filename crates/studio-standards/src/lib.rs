use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleMode {
    Lint,
    Check,
    Prompt,
}

impl RuleMode {
    pub fn costs_tokens(&self) -> bool {
        matches!(self, RuleMode::Prompt)
    }

    pub fn runs_in_verify(&self) -> bool {
        matches!(self, RuleMode::Lint | RuleMode::Check)
    }

    pub fn is_authoritative(&self) -> bool {
        self.runs_in_verify()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub mode: RuleMode,
    pub statement: String,
    #[serde(default)]
    pub zone: Option<String>,
}

impl Rule {
    pub fn applies_to_zone(&self, path: &str) -> bool {
        match &self.zone {
            None => true,
            Some(z) => path.contains(z.as_str()),
        }
    }
}

pub fn prompt_rules_for<'a>(rules: &'a [Rule], paths: &[String]) -> Vec<&'a Rule> {
    rules
        .iter()
        .filter(|r| r.mode == RuleMode::Prompt)
        .filter(|r| paths.iter().any(|p| r.applies_to_zone(p)))
        .collect()
}

pub fn enforced_rules(rules: &[Rule]) -> Vec<&Rule> {
    rules.iter().filter(|r| r.mode.runs_in_verify()).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trust {
    R0,
    R1,
    R2,
    R3,
    R4,
}

impl Trust {
    pub fn escalate(self) -> Trust {
        match self {
            Trust::R0 => Trust::R1,
            Trust::R1 => Trust::R2,
            Trust::R2 => Trust::R3,
            Trust::R3 | Trust::R4 => Trust::R4,
        }
    }

    pub fn gates(self) -> Vec<&'static str> {
        match self {
            Trust::R0 => vec!["verify"],
            Trust::R1 => vec!["verify", "check_rules"],
            Trust::R2 => vec!["verify", "check_rules", "peer_consult"],
            Trust::R3 => vec!["verify", "check_rules", "peer_consult", "senior_review"],
            Trust::R4 => vec![
                "verify",
                "check_rules",
                "peer_consult",
                "senior_review",
                "human_approval",
            ],
        }
    }

    pub fn needs_human(self) -> bool {
        self == Trust::R4
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    #[serde(default)]
    pub public_signature_changed: bool,
    #[serde(default)]
    pub incoming_refs: u32,
    #[serde(default)]
    pub covered_by_passing_test: bool,
    #[serde(default)]
    pub only_comments_or_strings: bool,
}

pub const BINARY_EXTENSIONS: [&str; 6] = ["umap", "uasset", "png", "fbx", "wav", "blend"];
pub const DATA_EXTENSIONS: [&str; 5] = ["unity", "prefab", "tscn", "tres", "asset"];
pub const BUILD_CONFIG_MARKERS: [&str; 6] = [
    "ProjectSettings",
    "manifest.json",
    ".uproject",
    "project.godot",
    "Build.bat",
    "export_presets.cfg",
];

fn extension(path: &str) -> String {
    path.rsplit('.').next().unwrap_or("").to_lowercase()
}

pub fn is_binary(path: &str) -> bool {
    BINARY_EXTENSIONS.contains(&extension(path).as_str())
}

pub fn is_data_file(path: &str) -> bool {
    DATA_EXTENSIONS.contains(&extension(path).as_str())
}

pub fn is_build_config(path: &str) -> bool {
    BUILD_CONFIG_MARKERS.iter().any(|m| path.contains(m))
}

pub fn is_unreal_map(path: &str) -> bool {
    matches!(extension(path).as_str(), "umap" | "uasset")
}

#[derive(Debug, Clone, PartialEq)]
pub struct Assessment {
    pub tier: Trust,
    pub reason: String,
    pub escalated_for_binary: bool,
}

pub fn assess(changes: &[FileChange]) -> Assessment {
    if changes.is_empty() {
        return Assessment {
            tier: Trust::R0,
            reason: "no files changed".into(),
            escalated_for_binary: false,
        };
    }

    let mut tier = Trust::R0;
    let mut reason = String::new();

    let touched: BTreeSet<&str> = changes.iter().map(|c| c.path.as_str()).collect();

    let trivial = changes
        .iter()
        .all(|c| c.only_comments_or_strings && c.covered_by_passing_test);
    if trivial && touched.len() == 1 {
        return Assessment {
            tier: Trust::R0,
            reason: "comments or strings only, covered by a passing test".into(),
            escalated_for_binary: false,
        };
    }

    if touched.len() == 1 && !changes[0].public_signature_changed {
        tier = Trust::R1;
        reason = "one file, no public signature change".into();
    }

    if touched.len() > 1 || changes.iter().any(|c| c.public_signature_changed) {
        tier = Trust::R2;
        reason = if changes.iter().any(|c| c.public_signature_changed) {
            "a public signature changed".into()
        } else {
            format!("{} files touched", touched.len())
        };
    }

    if changes.iter().any(|c| c.incoming_refs >= 10) {
        tier = Trust::R3;
        reason = "touches a widely referenced symbol".into();
    }
    if changes.iter().any(|c| is_data_file(&c.path)) {
        tier = Trust::R3;
        reason = "touches a scene or data file".into();
    }

    if changes.iter().any(|c| is_build_config(&c.path)) {
        tier = Trust::R4;
        reason = "touches build configuration".into();
    }
    if changes.iter().any(|c| is_binary(&c.path) && !is_unreal_map(&c.path)) {
        tier = Trust::R4;
        reason = "touches a binary asset that cannot be reviewed as text".into();
    }

    let mut escalated_for_binary = false;
    if changes.iter().any(|c| is_unreal_map(&c.path)) && tier != Trust::R4 {
        tier = tier.escalate();
        escalated_for_binary = true;
        reason = format!("{reason}; escalated because a .umap change cannot be diffed");
    } else if changes.iter().any(|c| is_unreal_map(&c.path)) {
        escalated_for_binary = true;
    }

    Assessment { tier, reason, escalated_for_binary }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(path: &str) -> FileChange {
        FileChange {
            path: path.into(),
            public_signature_changed: false,
            incoming_refs: 0,
            covered_by_passing_test: false,
            only_comments_or_strings: false,
        }
    }

    #[test]
    fn only_prompt_rules_cost_tokens() {
        assert!(RuleMode::Prompt.costs_tokens());
        assert!(!RuleMode::Lint.costs_tokens());
        assert!(!RuleMode::Check.costs_tokens());
    }

    #[test]
    fn mechanical_rules_are_the_authoritative_ones() {
        assert!(RuleMode::Lint.is_authoritative());
        assert!(RuleMode::Check.is_authoritative());
        assert!(
            !RuleMode::Prompt.is_authoritative(),
            "a prompt rule buys fewer repair rounds; it never enforces"
        );
    }

    #[test]
    fn dropping_prompt_rules_never_weakens_enforcement() {
        let rules = vec![
            Rule { id: "no_debug_log".into(), mode: RuleMode::Check, statement: "".into(), zone: None },
            Rule { id: "prefer_composition".into(), mode: RuleMode::Prompt, statement: "".into(), zone: None },
        ];
        let before = enforced_rules(&rules).len();
        let trimmed: Vec<Rule> = rules.into_iter().filter(|r| r.mode != RuleMode::Prompt).collect();
        assert_eq!(enforced_rules(&trimmed).len(), before);
    }

    #[test]
    fn prompt_rules_are_scoped_by_zone() {
        let rules = vec![
            Rule { id: "netcode".into(), mode: RuleMode::Prompt, statement: "".into(), zone: Some("netcode".into()) },
            Rule { id: "global".into(), mode: RuleMode::Prompt, statement: "".into(), zone: None },
        ];
        let hits = prompt_rules_for(&rules, &["src/ui/menu.gd".to_string()]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "global");

        let hits = prompt_rules_for(&rules, &["src/netcode/sync.gd".to_string()]);
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn a_comment_only_change_under_test_is_r0() {
        let mut c = f("src/player.gd");
        c.only_comments_or_strings = true;
        c.covered_by_passing_test = true;
        let a = assess(&[c]);
        assert_eq!(a.tier, Trust::R0);
        assert_eq!(a.tier.gates(), vec!["verify"]);
    }

    #[test]
    fn a_single_file_edit_is_r1() {
        assert_eq!(assess(&[f("src/player.gd")]).tier, Trust::R1);
    }

    #[test]
    fn a_public_signature_change_is_r2() {
        let mut c = f("src/player.gd");
        c.public_signature_changed = true;
        let a = assess(&[c]);
        assert_eq!(a.tier, Trust::R2);
        assert!(a.tier.gates().contains(&"peer_consult"));
    }

    #[test]
    fn touching_many_files_is_at_least_r2() {
        let a = assess(&[f("a.gd"), f("b.gd")]);
        assert!(a.tier >= Trust::R2);
    }

    #[test]
    fn a_widely_referenced_symbol_is_r3() {
        let mut c = f("src/core/state.gd");
        c.incoming_refs = 24;
        let a = assess(&[c]);
        assert_eq!(a.tier, Trust::R3);
        assert!(a.tier.gates().contains(&"senior_review"));
    }

    #[test]
    fn a_scene_file_is_r3() {
        assert_eq!(assess(&[f("levels/forest.tscn")]).tier, Trust::R3);
        assert_eq!(assess(&[f("Assets/Main.unity")]).tier, Trust::R3);
    }

    #[test]
    fn build_configuration_is_r4_and_needs_a_human() {
        let a = assess(&[f("ProjectSettings/ProjectVersion.txt")]);
        assert_eq!(a.tier, Trust::R4);
        assert!(a.tier.needs_human());
        assert!(a.tier.gates().contains(&"human_approval"));
    }

    #[test]
    fn a_binary_asset_is_r4() {
        assert_eq!(assess(&[f("art/hero.png")]).tier, Trust::R4);
        assert_eq!(assess(&[f("audio/theme.wav")]).tier, Trust::R4);
    }

    #[test]
    fn a_umap_change_escalates_one_level_because_it_cannot_be_diffed() {
        let plain = assess(&[f("src/a.cpp"), f("src/b.cpp")]);
        assert_eq!(plain.tier, Trust::R2);

        let with_map = assess(&[f("src/a.cpp"), f("src/b.cpp"), f("Maps/Arena.umap")]);
        assert_eq!(with_map.tier, Trust::R3, "R2 plus a umap becomes R3");
        assert!(with_map.escalated_for_binary);
        assert!(with_map.reason.contains("cannot be diffed"));
    }

    #[test]
    fn a_umap_on_an_r3_change_becomes_r4() {
        let mut c = f("src/core.cpp");
        c.incoming_refs = 30;
        let a = assess(&[c, f("Maps/Arena.umap")]);
        assert_eq!(a.tier, Trust::R4);
        assert!(a.tier.needs_human());
    }

    #[test]
    fn escalation_saturates_at_r4() {
        assert_eq!(Trust::R4.escalate(), Trust::R4);
        assert_eq!(Trust::R3.escalate(), Trust::R4);
    }

    #[test]
    fn every_tier_includes_the_gates_of_the_tier_below() {
        let tiers = [Trust::R0, Trust::R1, Trust::R2, Trust::R3, Trust::R4];
        for pair in tiers.windows(2) {
            let lower = pair[0].gates();
            let higher = pair[1].gates();
            for g in &lower {
                assert!(higher.contains(g), "{:?} dropped gate {g}", pair[1]);
            }
            assert!(higher.len() > lower.len());
        }
    }

    #[test]
    fn an_empty_diff_is_r0() {
        assert_eq!(assess(&[]).tier, Trust::R0);
    }

    #[test]
    fn the_tier_follows_the_realized_diff_not_the_intent() {
        let sprawl: Vec<FileChange> = (0..12)
            .map(|i| {
                let mut c = f(&format!("src/system{i}.gd"));
                c.public_signature_changed = i == 0;
                c
            })
            .collect();
        let a = assess(&sprawl);
        assert!(
            a.tier >= Trust::R2,
            "an agent that meant to make a small change but sprawled is gated at the sprawl"
        );
    }
}
