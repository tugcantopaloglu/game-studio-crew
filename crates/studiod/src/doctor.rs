use studio_server::health::{self, Kind, Requirements};

pub const NOTHING_TO_CODE_WITH: i32 = 2;
pub const NOTHING_THE_STUDIO_CAN_DRIVE: i32 = 3;

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
    match (found.ready, found.can_spawn) {
        (false, _) => NOTHING_TO_CODE_WITH,
        (true, false) => NOTHING_THE_STUDIO_CAN_DRIVE,
        (true, true) => 0,
    }
}

fn render(found: &Requirements) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "game studio crew {} on {}\n",
        found.app_version, found.os
    ));

    let rows: Vec<(Kind, String, String, &str)> = found
        .tools
        .iter()
        .map(|tool| {
            let state = match (&tool.present, &tool.version) {
                (true, Some(v)) => v.clone(),
                (true, None) => "present".into(),
                (false, _) => "absent".into(),
            };
            let tag = match (tool.kind, tool.present, tool.drivable) {
                (Kind::CodingCli, true, true) => "the studio spawns workers through this",
                (Kind::CodingCli, true, false) => "installed, but the studio cannot drive it",
                _ => "",
            };
            (tool.kind, tool.label.clone(), state, tag)
        })
        .collect();

    let widest = |pick: fn(&(Kind, String, String, &str)) -> usize| {
        rows.iter().map(pick).max().unwrap_or(0).max(8)
    };
    let labels = widest(|r| r.1.chars().count());
    let states = widest(|r| r.2.chars().count());

    for kind in Kind::ALL {
        out.push('\n');
        out.push_str(kind.heading());
        out.push('\n');
        for (_, label, state, tag) in rows.iter().filter(|r| r.0 == kind) {
            let line = format!("  {label:<labels$}  {state:<states$}  {tag}");
            out.push_str(line.trim_end());
            out.push('\n');
        }
    }

    out.push('\n');
    if !found.ready {
        out.push_str("nothing to code with. Install one of ");
        out.push_str(&health::CODING_CLIS.join(", "));
        out.push_str(" and put it on PATH,\nthen run studiod doctor again.\n");
        return out;
    }

    out.push_str(&format!(
        "installed: {}\n",
        found.coding_clis_found().join(", ")
    ));

    if found.can_spawn {
        out.push_str(&format!(
            "the studio can spawn workers through: {}\n",
            found.coding_clis_the_studio_can_drive().join(", ")
        ));
    } else {
        out.push_str("but nothing installed can run the crew yet:\n");
        for (name, why) in found.installed_but_undrivable() {
            out.push_str(&format!("  {name}: {why}\n"));
        }
        out.push_str("\nInstall Claude Code and put claude on PATH; it is the only CLI the studio\n");
        out.push_str("can hand a frozen charter to today.\n");
    }

    out.push_str("anything else reported absent above is optional; the studio runs without it.\n");
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
        assert!(text.contains("the studio can spawn workers through: claude"));
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
    fn a_cli_that_is_installed_but_undrivable_is_never_reported_as_a_green_tick() {
        let found = Requirements::new(vec![
            Tool::absent("claude", "claude", Kind::CodingCli),
            Tool::found("gemini", "gemini", Kind::CodingCli, Some("0.4.1".into())),
        ]);
        let text = render(&found);

        assert_eq!(
            exit_code(&found),
            NOTHING_THE_STUDIO_CAN_DRIVE,
            "an install with only gemini succeeds but cannot run the crew"
        );
        assert!(text.contains("installed, but the studio cannot drive it"));
        assert!(text.contains("but nothing installed can run the crew yet"));
        assert!(
            text.contains("system prompt"),
            "the reason must be the provider table's own words: {text}"
        );
        assert!(text.contains("Install Claude Code"));
        assert!(
            !text.contains("the studio can spawn workers through"),
            "nothing may read as ready to spawn: {text}"
        );
    }

    #[test]
    fn an_install_with_only_codex_reports_what_it_has_and_proceeds() {
        let found = Requirements::new(vec![
            Tool::absent("claude", "claude", Kind::CodingCli),
            Tool::found("codex", "codex", Kind::CodingCli, Some("0.9.1".into())),
        ]);
        assert_ne!(
            exit_code(&found),
            NOTHING_TO_CODE_WITH,
            "codex on PATH is still a successful install"
        );
        let text = render(&found);
        assert!(text.contains("installed: codex"));
        assert!(text.contains("codex: the studio has no provider for codex"));
    }

    #[test]
    fn every_group_the_probe_fills_gets_a_heading() {
        let text = render(&only_claude_and_no_engine());
        for kind in Kind::ALL {
            assert!(text.contains(kind.heading()), "{:?} is never headed", kind);
        }
    }
}
