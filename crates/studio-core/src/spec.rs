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

    pub const ALL: [Effort; 5] =
        [Effort::Low, Effort::Medium, Effort::High, Effort::XHigh, Effort::Max];

    pub fn named(name: &str) -> Option<Effort> {
        Effort::ALL.into_iter().find(|e| e.as_str() == name)
    }

    pub fn clamp_to(&self, supported: &[String]) -> Option<Effort> {
        if supported.is_empty() {
            return Some(*self);
        }
        let allowed: Vec<Effort> = supported.iter().filter_map(|s| Effort::named(s)).collect();
        if allowed.is_empty() {
            return None;
        }
        allowed.into_iter().filter(|e| e <= self).max()
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
    Codex,
    Gemini,
    Copilot,
    Kimi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BriefDelivery {
    Stdin,
    PromptArgument,
    Positional,
}

pub const PROBE_QUESTION: &str = "what is 17 plus 25? reply with just the number";
pub const PROBE_ANSWER: &str = "42";

pub fn probe_answered(output: &str) -> bool {
    output.replace(PROBE_QUESTION, " ").contains(PROBE_ANSWER)
}

const OUT_OF_ALLOWANCE: [&str; 5] = [
    "session limit",
    "usage limit",
    "rate limit",
    "quota",
    "limit reached",
];

const AND_SAYS_SO: [&str; 6] = [
    "resets",
    "reached",
    "hit your",
    "try again",
    "upgrade",
    "exceeded",
];

pub fn account_is_out_of_allowance(message: &str) -> Option<String> {
    let text = message.to_lowercase();
    let named = OUT_OF_ALLOWANCE.iter().any(|m| text.contains(m));
    let confirmed = AND_SAYS_SO.iter().any(|m| text.contains(m));
    if !named || !confirmed {
        return None;
    }

    let line = message
        .lines()
        .map(str::trim)
        .find(|l| {
            let lower = l.to_lowercase();
            OUT_OF_ALLOWANCE.iter().any(|m| lower.contains(m))
        })
        .unwrap_or(message.trim());
    Some(line.chars().take(200).collect())
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
    pub const ALL: [Provider; 5] = [
        Provider::Claude,
        Provider::Codex,
        Provider::Gemini,
        Provider::Copilot,
        Provider::Kimi,
    ];

    pub fn id(&self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
            Provider::Gemini => "gemini",
            Provider::Copilot => "copilot",
            Provider::Kimi => "kimi",
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Provider::Claude => "Claude Code",
            Provider::Codex => "Codex CLI",
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
            Provider::Codex => BriefDelivery::Positional,
            _ => BriefDelivery::PromptArgument,
        }
    }

    pub fn probe_args(&self, model: &str) -> Option<Vec<String>> {
        let mut a: Vec<String> = match self {
            Provider::Claude => vec![
                "-p".into(),
                "--setting-sources".into(),
                String::new(),
                "--tools".into(),
                String::new(),
                "--output-format".into(),
                "json".into(),
            ],
            Provider::Codex => vec![
                "exec".into(),
                "--sandbox".into(),
                "read-only".into(),
                "--skip-git-repo-check".into(),
            ],
            Provider::Gemini => vec!["--output-format".into(), "json".into()],
            Provider::Copilot => vec![
                "--output-format".into(),
                "json".into(),
                "--no-custom-instructions".into(),
                "--no-color".into(),
            ],
            Provider::Kimi => return None,
        };

        if !model.trim().is_empty() {
            a.push(match self {
                Provider::Codex => "-m".into(),
                _ => "--model".into(),
            });
            a.push(model.trim().to_string());
        }

        match self.brief_delivery() {
            BriefDelivery::Stdin => {}
            BriefDelivery::PromptArgument => {
                a.push("-p".into());
                a.push(PROBE_QUESTION.into());
            }
            BriefDelivery::Positional => a.push(PROBE_QUESTION.into()),
        }

        Some(a)
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
            Provider::Codex => Capabilities {
                streaming_events: true,
                usage_reporting: false,
                tool_restriction: false,
                system_prompt_file: false,
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
                Provider::Codex => "codex has no flag that replaces its system prompt: AGENTS.md is a per-project file the whole crew would share, not a per-spawn charter, and the instructions-file config override was measured to have no effect, so the frozen charter cannot be delivered; pick claude",
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
            Provider::Codex => self.codex_args(model),
            Provider::Gemini => self.gemini_args(model),
            Provider::Copilot => self.copilot_args(model),
            Provider::Kimi => Vec::new(),
        }
    }

    fn codex_args(&self, model: &str) -> Vec<String> {
        let mut a: Vec<String> = vec!["exec".into(), "--json".into(), "--skip-git-repo-check".into()];

        a.push("--sandbox".into());
        a.push(if self.allowed_tools.is_empty() {
            "read-only".into()
        } else {
            "workspace-write".into()
        });

        if !model.is_empty() {
            a.push("-m".into());
            a.push(model.into());
        }

        if let Some(schema) = &self.json_schema {
            a.push("--output-schema".into());
            a.push(schema.clone());
        }

        a
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
        assert_eq!(Provider::Codex.brief_delivery(), BriefDelivery::Positional);
        for p in [Provider::Gemini, Provider::Copilot, Provider::Kimi] {
            assert_eq!(p.brief_delivery(), BriefDelivery::PromptArgument);
        }
    }

    #[test]
    fn a_cli_that_echoes_the_question_cannot_pass_a_probe_on_the_echo_alone() {
        let echoed = format!("user\n{PROBE_QUESTION}\ncodex\n");
        assert!(
            !probe_answered(&echoed),
            "codex prints the prompt back; matching the prompt would grade every model as working"
        );
        assert!(probe_answered(&format!("{echoed}42\ntokens used\n1,668\n")));
    }

    #[test]
    fn the_probe_question_cannot_contain_its_own_answer() {
        assert!(!PROBE_QUESTION.contains(PROBE_ANSWER));
    }

    #[test]
    fn a_probe_reaches_each_cli_the_way_that_cli_takes_a_prompt() {
        let claude = Provider::Claude.probe_args("haiku").unwrap();
        assert!(claude.iter().any(|a| a == "-p"));
        assert!(!claude.iter().any(|a| a == PROBE_QUESTION), "claude reads it on stdin");
        assert_eq!(pair(&claude, "--model"), Some("haiku".into()));
        assert_eq!(pair(&claude, "--output-format"), Some("json".into()));

        let codex = Provider::Codex.probe_args("gpt-5.6-luna").unwrap();
        assert_eq!(codex.first().map(String::as_str), Some("exec"));
        assert_eq!(pair(&codex, "-m"), Some("gpt-5.6-luna".into()));
        assert_eq!(pair(&codex, "--sandbox"), Some("read-only".into()));
        assert_eq!(codex.last().map(String::as_str), Some(PROBE_QUESTION));
    }

    #[test]
    fn a_probe_of_a_cli_the_studio_never_read_is_not_offered_at_all() {
        assert!(Provider::Kimi.probe_args("kimi-k2").is_none());
    }

    #[test]
    fn a_probe_with_no_model_named_asks_the_cli_about_its_own_default() {
        let codex = Provider::Codex.probe_args("").unwrap();
        assert!(!codex.iter().any(|a| a == "-m"));
        assert_eq!(codex.last().map(String::as_str), Some(PROBE_QUESTION));
    }

    #[test]
    fn codex_gets_the_schema_flag_it_really_has_and_a_sandbox_matching_the_role() {
        let mut s = spec();
        s.json_schema = Some("C:/run/plan.schema.json".into());
        let args = s.to_args_for(Provider::Codex, "gpt-5.6-sol");
        assert_eq!(pair(&args, "--output-schema"), Some("C:/run/plan.schema.json".into()));
        assert_eq!(pair(&args, "--sandbox"), Some("workspace-write".into()));
        assert_eq!(pair(&args, "-m"), Some("gpt-5.6-sol".into()));

        s.allowed_tools = Vec::new();
        assert_eq!(
            pair(&s.to_args_for(Provider::Codex, ""), "--sandbox"),
            Some("read-only".into())
        );
    }

    #[test]
    fn codex_can_hold_a_schema_but_still_cannot_hold_the_frozen_charter() {
        let caps = Provider::Codex.capabilities();
        assert!(caps.structured_output, "codex exec --output-schema is a real flag");
        assert!(!caps.system_prompt_file);
        assert!(Provider::Codex
            .blockers(RoleNeeds::default())
            .iter()
            .any(|r| r.contains("per-project file") && r.contains("no effect")));
    }

    #[test]
    fn every_provider_still_appears_in_the_supervisor_s_own_list() {
        assert_eq!(Provider::ALL.len(), 5);
        for id in ["claude", "codex", "gemini", "copilot", "kimi"] {
            assert!(
                Provider::ALL.iter().any(|p| p.id() == id),
                "{id} fell out of Provider::ALL, so the doctor would stop reporting it"
            );
        }
    }

    #[test]
    fn an_effort_a_model_does_not_offer_is_clamped_down_to_the_best_it_does() {
        let older: Vec<String> = ["low", "medium", "high", "xhigh"].iter().map(|s| s.to_string()).collect();
        assert_eq!(Effort::Max.clamp_to(&older), Some(Effort::XHigh));
        assert_eq!(Effort::High.clamp_to(&older), Some(Effort::High));
        assert_eq!(Effort::Low.clamp_to(&older), Some(Effort::Low));
    }

    #[test]
    fn a_model_that_offers_everything_leaves_the_chosen_effort_alone() {
        let newest: Vec<String> = ["low", "medium", "high", "xhigh", "max", "ultra"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        for e in Effort::ALL {
            assert_eq!(e.clamp_to(&newest), Some(e));
        }
    }

    #[test]
    fn a_level_the_studio_has_no_concept_of_is_ignored_rather_than_passed_through() {
        let only_ultra = vec!["ultra".to_string()];
        assert_eq!(
            Effort::Max.clamp_to(&only_ultra),
            None,
            "ultra is codex's own level; the studio must not silently send a word it cannot express"
        );
    }

    #[test]
    fn a_model_with_no_published_levels_is_left_to_the_studios_own_choice() {
        assert_eq!(Effort::XHigh.clamp_to(&[]), Some(Effort::XHigh));
    }

    #[test]
    fn a_model_whose_floor_is_above_the_ask_reports_nothing_rather_than_guessing_up() {
        let high_only = vec!["high".to_string(), "max".to_string()];
        assert_eq!(Effort::Low.clamp_to(&high_only), None);
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

    #[test]
    fn the_cli_running_out_of_allowance_is_recognised_from_its_own_words() {
        let said = account_is_out_of_allowance(
            "You've hit your session limit · resets 7:40pm (Europe/Istanbul)",
        )
        .expect("the refusal that stopped a run must be recognised");
        assert!(said.contains("7:40pm"), "the reset time is the only actionable part: {said}");

        assert!(account_is_out_of_allowance("Claude usage limit reached").is_some());
        assert!(account_is_out_of_allowance("rate limit exceeded, try again later").is_some());
        assert!(account_is_out_of_allowance("quota exceeded for this account").is_some());
    }

    #[test]
    fn an_ordinary_failure_is_not_read_as_an_exhausted_account() {
        assert!(account_is_out_of_allowance("Not logged in").is_none());
        assert!(account_is_out_of_allowance("the file could not be written").is_none());
        assert!(
            account_is_out_of_allowance("add a rate limit to the boss fight spawner").is_none(),
            "a brief about a game mechanic must not stop the whole run"
        );
    }

    #[test]
    fn the_quoted_line_is_the_one_naming_the_limit_not_whatever_came_first() {
        let said = account_is_out_of_allowance(
            "the worker stopped
You have hit your usage limit; resets at 9pm
trailing noise",
        )
        .unwrap();
        assert!(said.starts_with("You have hit your usage limit"), "{said}");
    }
}
