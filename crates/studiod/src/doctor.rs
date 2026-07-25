use studio_server::health::{self, Kind, Requirements};

pub const NOTHING_TO_CODE_WITH: i32 = 2;

pub fn report() -> anyhow::Result<()> {
    let found = health::probe();
    print!("{}", render(&found));
    let code = exit_code(&found);
    if code != 0 {
        std::process::exit(code);
    }
    Ok(())
}

fn exit_code(found: &Requirements) -> i32 {
    if found.ready {
        0
    } else {
        NOTHING_TO_CODE_WITH
    }
}

fn render(found: &Requirements) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "game studio crew {} on {}\n",
        found.app_version, found.os
    ));

    let column = found
        .tools
        .iter()
        .map(|t| t.label.chars().count())
        .max()
        .unwrap_or(0)
        .max(8);

    for kind in Kind::ALL {
        out.push('\n');
        out.push_str(kind.heading());
        out.push('\n');
        for tool in found.of_kind(kind) {
            let state = match (&tool.present, &tool.version) {
                (true, Some(v)) => v.clone(),
                (true, None) => "present".into(),
                (false, _) => "absent".into(),
            };
            out.push_str(&format!(
                "  {:<width$}  {}\n",
                tool.label,
                state,
                width = column
            ));
        }
    }

    out.push('\n');
    if found.ready {
        out.push_str(&format!(
            "ready: the studio can drive {}.\n",
            found.coding_clis_found().join(", ")
        ));
        out.push_str("anything reported absent above is optional; the studio runs without it.\n");
    } else {
        out.push_str("nothing to code with. Install one of ");
        out.push_str(&health::CODING_CLIS.join(", "));
        out.push_str(" and put it on PATH,\nthen run studiod doctor again.\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_server::health::Tool;

    fn only_claude_and_no_engine() -> Requirements {
        Requirements::new(vec![
            Tool::found("claude", "claude", Kind::CodingCli, Some("2.1.0".into())),
            Tool::absent("codex", "codex", Kind::CodingCli),
            Tool::found("git", "git", Kind::Toolchain, Some("2.47.0".into())),
            Tool::absent("godot", "Godot 4", Kind::Engine),
        ])
    }

    #[test]
    fn a_missing_optional_tool_is_reported_as_absent_rather_than_failing() {
        let found = only_claude_and_no_engine();
        let text = render(&found);
        assert!(text.contains("Godot 4"), "an absent engine is still listed");
        assert!(text.contains("absent"));
        assert_eq!(
            exit_code(&found),
            0,
            "a missing engine must not stop an install that can code"
        );
        assert!(text.contains("ready: the studio can drive claude."));
    }

    #[test]
    fn the_doctor_fails_when_there_is_no_coding_cli_at_all() {
        let found = Requirements::new(vec![
            Tool::absent("claude", "claude", Kind::CodingCli),
            Tool::absent("codex", "codex", Kind::CodingCli),
            Tool::found("git", "git", Kind::Toolchain, Some("2.47.0".into())),
            Tool::found("godot", "Godot 4", Kind::Engine, Some("4.5".into())),
        ]);
        assert_eq!(exit_code(&found), NOTHING_TO_CODE_WITH);
        let text = render(&found);
        assert!(text.contains("nothing to code with"));
        assert!(
            text.contains("claude, codex, gemini, copilot, kimi"),
            "the message must name what would fix it"
        );
    }

    #[test]
    fn an_install_with_only_codex_reports_what_it_has_and_proceeds() {
        let found = Requirements::new(vec![
            Tool::absent("claude", "claude", Kind::CodingCli),
            Tool::found("codex", "codex", Kind::CodingCli, Some("0.9.1".into())),
        ]);
        assert_eq!(exit_code(&found), 0);
        assert!(render(&found).contains("ready: the studio can drive codex."));
    }

    #[test]
    fn every_group_the_probe_fills_gets_a_heading() {
        let text = render(&only_claude_and_no_engine());
        for kind in Kind::ALL {
            assert!(text.contains(kind.heading()), "{:?} is never headed", kind);
        }
    }
}
