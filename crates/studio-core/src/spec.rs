use studio_context::Model;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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

    pub fn downshift(&self, floor: Effort) -> Effort {
        let lower = match self {
            Effort::Max => Effort::XHigh,
            Effort::XHigh => Effort::High,
            Effort::High => Effort::Medium,
            Effort::Medium => Effort::Low,
            Effort::Low => Effort::Low,
        };
        if lower < floor {
            floor
        } else {
            lower
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionMode {
    New(String),
    Resume(String),
    ForkFrom(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkerSpec {
    pub system_prompt_file: String,
    pub tools: Vec<String>,
    pub allowed_tools: Vec<String>,
    pub model: Model,
    pub effort: Effort,
    pub session: SessionMode,
    pub mcp_config: Option<String>,
    pub json_schema: Option<String>,
}

impl WorkerSpec {
    pub fn to_args(&self) -> Vec<String> {
        let mut a: Vec<String> = vec!["-p".into()];

        a.push("--setting-sources".into());
        a.push(String::new());

        a.push("--system-prompt-file".into());
        a.push(self.system_prompt_file.clone());

        a.push("--tools".into());
        a.push(self.tools.join(","));

        if !self.allowed_tools.is_empty() {
            a.push("--allowedTools".into());
            a.push(self.allowed_tools.join(","));
        }

        a.push("--model".into());
        a.push(self.model.cli_alias().into());

        a.push("--effort".into());
        a.push(self.effort.as_str().into());

        match &self.session {
            SessionMode::New(id) => {
                a.push("--session-id".into());
                a.push(id.clone());
            }
            SessionMode::Resume(id) => {
                a.push("--resume".into());
                a.push(id.clone());
            }
            SessionMode::ForkFrom(id) => {
                a.push("--resume".into());
                a.push(id.clone());
                a.push("--fork-session".into());
            }
        }

        a.push("--permission-mode".into());
        a.push("dontAsk".into());

        if let Some(cfg) = &self.mcp_config {
            a.push("--mcp-config".into());
            a.push(cfg.clone());
            a.push("--strict-mcp-config".into());
        }

        if let Some(schema) = &self.json_schema {
            a.push("--json-schema".into());
            a.push(schema.clone());
        }

        a.push("--output-format".into());
        a.push("stream-json".into());
        a.push("--include-partial-messages".into());
        a.push("--verbose".into());

        a
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> WorkerSpec {
        WorkerSpec {
            system_prompt_file: "C:/charters/gameplay.txt".into(),
            tools: vec!["Read".into(), "Grep".into(), "Glob".into()],
            allowed_tools: vec!["Read".into(), "mcp__studio__capsule_submit".into()],
            model: Model::Opus,
            effort: Effort::High,
            session: SessionMode::New("11111111-2222-3333-4444-555555555555".into()),
            mcp_config: Some("C:/run/mcp.json".into()),
            json_schema: None,
        }
    }

    fn pair(args: &[String], flag: &str) -> Option<String> {
        args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned())
    }

    #[test]
    fn never_passes_bare_or_safe_mode() {
        let args = spec().to_args();
        assert!(!args.iter().any(|a| a == "--bare"), "ADR 0004: --bare breaks subscription auth");
        assert!(!args.iter().any(|a| a == "--safe-mode"), "ADR 0004: --safe-mode disables MCP");
    }

    #[test]
    fn strips_context_the_way_adr_0004_specifies() {
        let args = spec().to_args();
        assert_eq!(pair(&args, "--setting-sources"), Some(String::new()));
        assert_eq!(pair(&args, "--system-prompt-file"), Some("C:/charters/gameplay.txt".into()));
        assert_eq!(pair(&args, "--tools"), Some("Read,Grep,Glob".into()));
    }

    #[test]
    fn stream_json_always_carries_verbose() {
        let args = spec().to_args();
        assert_eq!(pair(&args, "--output-format"), Some("stream-json".into()));
        assert!(
            args.iter().any(|a| a == "--verbose"),
            "stream-json without --verbose makes the CLI error out"
        );
        assert!(args.iter().any(|a| a == "--include-partial-messages"));
    }

    #[test]
    fn mcp_is_attached_strictly_when_configured() {
        let args = spec().to_args();
        assert_eq!(pair(&args, "--mcp-config"), Some("C:/run/mcp.json".into()));
        assert!(args.iter().any(|a| a == "--strict-mcp-config"));
    }

    #[test]
    fn mcp_flags_are_absent_when_unconfigured() {
        let mut s = spec();
        s.mcp_config = None;
        let args = s.to_args();
        assert!(!args.iter().any(|a| a == "--mcp-config"));
        assert!(!args.iter().any(|a| a == "--strict-mcp-config"));
    }

    #[test]
    fn resume_replaces_session_id_for_repair_rounds() {
        let mut s = spec();
        s.session = SessionMode::Resume("sess-1".into());
        let args = s.to_args();
        assert_eq!(pair(&args, "--resume"), Some("sess-1".into()));
        assert!(!args.iter().any(|a| a == "--session-id"));
    }

    #[test]
    fn fork_session_is_resume_plus_fork() {
        let mut s = spec();
        s.session = SessionMode::ForkFrom("sess-1".into());
        let args = s.to_args();
        assert_eq!(pair(&args, "--resume"), Some("sess-1".into()));
        assert!(args.iter().any(|a| a == "--fork-session"));
    }

    #[test]
    fn a_structured_output_schema_is_passed_through_when_set() {
        let mut s = spec();
        s.json_schema = Some("{\"type\":\"object\"}".into());
        let args = s.to_args();
        assert_eq!(pair(&args, "--json-schema"), Some("{\"type\":\"object\"}".into()));
    }

    #[test]
    fn no_schema_flag_appears_when_none_is_wanted() {
        let args = spec().to_args();
        assert!(!args.iter().any(|a| a == "--json-schema"));
    }

    #[test]
    fn effort_downshift_respects_the_role_floor() {
        assert_eq!(Effort::XHigh.downshift(Effort::Low), Effort::High);
        assert_eq!(Effort::Medium.downshift(Effort::Medium), Effort::Medium);
        assert_eq!(Effort::Low.downshift(Effort::Low), Effort::Low);
        assert_eq!(Effort::High.downshift(Effort::High), Effort::High);
    }
}
