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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Claude,
    Gemini,
    Copilot,
    Kimi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BriefDelivery {
    Stdin,
    PromptArgument,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub streaming_events: bool,
    pub usage_reporting: bool,
    pub tool_restriction: bool,
    pub system_prompt_file: bool,
    pub structured_output: bool,
    pub session_control: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RoleNeeds {
    pub structured_output: bool,
    pub restricted_tools: bool,
}

impl Provider {
    pub const ALL: [Provider; 4] =
        [Provider::Claude, Provider::Gemini, Provider::Copilot, Provider::Kimi];

    pub fn id(&self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Gemini => "gemini",
            Provider::Copilot => "copilot",
            Provider::Kimi => "kimi",
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Provider::Claude => "Claude Code",
            Provider::Gemini => "Gemini CLI",
            Provider::Copilot => "GitHub Copilot CLI",
            Provider::Kimi => "Kimi CLI",
        }
    }

    pub fn program(&self) -> &'static str {
        self.id()
    }

    pub fn from_id(id: &str) -> Option<Provider> {
        Provider::ALL.into_iter().find(|p| p.id() == id)
    }

    pub fn flags_were_read(&self) -> bool {
        !matches!(self, Provider::Kimi)
    }

    pub fn brief_delivery(&self) -> BriefDelivery {
        match self {
            Provider::Claude => BriefDelivery::Stdin,
            _ => BriefDelivery::PromptArgument,
        }
    }

    pub fn capabilities(&self) -> Capabilities {
        match self {
            Provider::Claude => Capabilities {
                streaming_events: true,
                usage_reporting: true,
                tool_restriction: true,
                system_prompt_file: true,
                structured_output: true,
                session_control: true,
            },
            Provider::Gemini => Capabilities {
                streaming_events: true,
                usage_reporting: false,
                tool_restriction: false,
                system_prompt_file: false,
                structured_output: false,
                session_control: false,
            },
            Provider::Copilot => Capabilities {
                streaming_events: true,
                usage_reporting: false,
                tool_restriction: true,
                system_prompt_file: false,
                structured_output: false,
                session_control: true,
            },
            Provider::Kimi => Capabilities {
                streaming_events: false,
                usage_reporting: false,
                tool_restriction: false,
                system_prompt_file: false,
                structured_output: false,
                session_control: false,
            },
        }
    }

    pub fn blockers(&self, needs: RoleNeeds) -> Vec<&'static str> {
        if !self.flags_were_read() {
            return vec![
                "the studio has never read this CLI's flags, so spawning it would mean guessing them; install it and re-run the provider probe, or pick claude",
            ];
        }

        let caps = self.capabilities();
        let mut out = Vec::new();

        if !caps.system_prompt_file {
            out.push(match self {
                Provider::Gemini => "gemini has no flag that replaces the system prompt, so the frozen charter cannot be delivered and every spawn would pay full price for context the studio already froze; pick claude",
                _ => "this CLI has no flag that replaces the system prompt, so the frozen charter and its cache cannot be delivered; pick claude",
            });
        }
        if !caps.streaming_events {
            out.push("this CLI does not stream events the studio can reduce, so the floor would show nothing while it works; pick claude");
        }
        if !caps.usage_reporting {
            out.push("this CLI does not report token usage the studio can read, so budgets and the ledger would be blind; pick claude");
        }
        if needs.structured_output && !caps.structured_output {
            out.push("this role's answer is read back as JSON against a schema and this CLI has no equivalent of --json-schema; pick claude for it");
        }
        if needs.restricted_tools && !caps.tool_restriction {
            out.push("this CLI cannot limit the model to a named tool set, and the tool allowlist is the studio's largest token lever; pick claude");
        }

        out
    }

    pub fn can_serve(&self, needs: RoleNeeds) -> bool {
        self.blockers(needs).is_empty()
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
        self.to_args_for(Provider::Claude, self.model.cli_alias())
    }

    pub fn to_args_for(&self, provider: Provider, model: &str) -> Vec<String> {
        match provider {
            Provider::Claude => self.claude_args(model),
            Provider::Gemini => self.gemini_args(model),
            Provider::Copilot => self.copilot_args(model),
            Provider::Kimi => Vec::new(),
        }
    }

    fn gemini_args(&self, model: &str) -> Vec<String> {
        let mut a: Vec<String> = vec!["--output-format".into(), "stream-json".into()];

        if !model.is_empty() {
            a.push("--model".into());
            a.push(model.into());
        }

        if self.allowed_tools.is_empty() {
            a.push("--approval-mode".into());
            a.push("plan".into());
        } else {
            a.push("--approval-mode".into());
            a.push("yolo".into());
            a.push("--allowed-tools".into());
            a.push(self.allowed_tools.join(","));
        }

        a
    }

    fn copilot_args(&self, model: &str) -> Vec<String> {
        let mut a: Vec<String> = vec!["--output-format".into(), "json".into()];

        a.push("--stream".into());
        a.push("on".into());

        if !model.is_empty() {
            a.push("--model".into());
            a.push(model.into());
        }

        a.push("--available-tools".into());
        a.push(self.tools.join(","));

        if self.allowed_tools.is_empty() {
            a.push("--plan".into());
        } else {
            a.push("--allow-all-tools".into());
        }

        a.push("--no-custom-instructions".into());
        a.push("--no-ask-user".into());
        a.push("--no-color".into());

        if let SessionMode::New(id) = &self.session {
            a.push("--session-id".into());
            a.push(id.clone());
        }

        a
    }

    fn claude_args(&self, model: &str) -> Vec<String> {
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
        a.push(model.into());

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
    fn the_claude_command_line_is_frozen_byte_for_byte() {
        let expected: Vec<String> = [
            "-p",
            "--setting-sources",
            "",
            "--system-prompt-file",
            "C:/charters/gameplay.txt",
            "--tools",
            "Read,Grep,Glob",
            "--allowedTools",
            "Read,mcp__studio__capsule_submit",
            "--model",
            "opus",
            "--effort",
            "high",
            "--session-id",
            "11111111-2222-3333-4444-555555555555",
            "--permission-mode",
            "dontAsk",
            "--mcp-config",
            "C:/run/mcp.json",
            "--strict-mcp-config",
            "--output-format",
            "stream-json",
            "--include-partial-messages",
            "--verbose",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        assert_eq!(
            spec().to_args(),
            expected,
            "these bytes are the prompt-cache key; changing them throws away every warm prefix"
        );
    }

    #[test]
    fn asking_for_claude_by_name_produces_the_same_line_as_the_default() {
        let s = spec();
        assert_eq!(s.to_args(), s.to_args_for(Provider::Claude, s.model.cli_alias()));
    }

    #[test]
    fn a_model_override_reaches_the_command_line_because_it_is_part_of_the_cache_key() {
        let args = spec().to_args_for(Provider::Claude, "haiku");
        assert_eq!(pair(&args, "--model"), Some("haiku".into()));
    }

    #[test]
    fn only_claude_takes_its_brief_on_stdin() {
        assert_eq!(Provider::Claude.brief_delivery(), BriefDelivery::Stdin);
        for p in [Provider::Gemini, Provider::Copilot, Provider::Kimi] {
            assert_eq!(p.brief_delivery(), BriefDelivery::PromptArgument);
        }
    }

    #[test]
    fn every_provider_id_round_trips_so_a_stored_setting_resolves() {
        for p in Provider::ALL {
            assert_eq!(Provider::from_id(p.id()), Some(p));
        }
        assert_eq!(Provider::from_id("nonesuch"), None);
    }

    #[test]
    fn claude_is_the_only_provider_that_can_serve_the_directors_schema_bound_plan() {
        let needs = RoleNeeds { structured_output: true, restricted_tools: true };
        assert!(Provider::Claude.can_serve(needs));
        for p in [Provider::Gemini, Provider::Copilot, Provider::Kimi] {
            assert!(!p.can_serve(needs), "{} claims it can hold a schema", p.id());
        }
    }

    #[test]
    fn a_blocked_provider_says_what_is_missing_and_what_to_pick_instead() {
        for p in [Provider::Gemini, Provider::Copilot, Provider::Kimi] {
            let reasons = p.blockers(RoleNeeds::default());
            assert!(!reasons.is_empty());
            for r in reasons {
                assert!(r.contains("claude"), "{} gives no way out: {r}", p.id());
            }
        }
    }

    #[test]
    fn a_cli_the_studio_never_probed_is_refused_before_any_flag_is_guessed() {
        assert!(!Provider::Kimi.flags_were_read());
        assert!(spec().to_args_for(Provider::Kimi, "kimi-k2").is_empty());
        assert_eq!(Provider::Kimi.blockers(RoleNeeds::default()).len(), 1);
    }

    #[test]
    fn gemini_is_asked_for_the_stream_the_studio_reduces_into_events() {
        let args = spec().to_args_for(Provider::Gemini, "gemini-3-pro");
        assert_eq!(pair(&args, "--output-format"), Some("stream-json".into()));
        assert_eq!(pair(&args, "--model"), Some("gemini-3-pro".into()));
        assert_eq!(pair(&args, "--approval-mode"), Some("yolo".into()));
    }

    #[test]
    fn an_advisory_spawn_never_asks_a_provider_for_write_permission() {
        let mut s = spec();
        s.allowed_tools = Vec::new();
        assert_eq!(
            pair(&s.to_args_for(Provider::Gemini, ""), "--approval-mode"),
            Some("plan".into())
        );
        assert!(s
            .to_args_for(Provider::Copilot, "")
            .iter()
            .any(|a| a == "--plan"));
    }

    #[test]
    fn a_provider_with_no_model_named_is_left_on_its_own_default() {
        assert!(!spec().to_args_for(Provider::Gemini, "").iter().any(|a| a == "--model"));
        assert!(!spec().to_args_for(Provider::Copilot, "").iter().any(|a| a == "--model"));
    }

    #[test]
    fn copilot_is_held_to_the_same_tool_allowlist_the_role_carries() {
        let args = spec().to_args_for(Provider::Copilot, "gpt-5.4");
        assert_eq!(pair(&args, "--available-tools"), Some("Read,Grep,Glob".into()));
        assert!(args.iter().any(|a| a == "--no-custom-instructions"));
    }

    #[test]
    fn effort_downshift_respects_the_role_floor() {
        assert_eq!(Effort::XHigh.downshift(Effort::Low), Effort::High);
        assert_eq!(Effort::Medium.downshift(Effort::Medium), Effort::Medium);
        assert_eq!(Effort::Low.downshift(Effort::Low), Effort::Low);
        assert_eq!(Effort::High.downshift(Effort::High), Effort::High);
    }
}
