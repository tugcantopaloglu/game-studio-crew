use studio_server::health::{self, Kind, Requirements};

pub const NOTHING_TO_CODE_WITH: i32 = 2;
pub const NOTHING_THE_STUDIO_CAN_DRIVE: i32 = 3;

pub const ART_PIPELINE_INCOMPLETE: i32 = 4;

pub fn report() -> anyhow::Result<()> {
    if std::env::args().any(|a| a == "--fix") {
        return fix();
    }
    let found = health::probe();
    if std::env::args().any(|a| a == "--porcelain") {
        print!("{}", porcelain(&found));
        let code = exit_code(&found);
        if code != 0 {
            std::process::exit(code);
        }
        return Ok(());
    }
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
        (true, true) if !asset_gaps(found).is_empty() => ART_PIPELINE_INCOMPLETE,
        (true, true) => 0,
    }
}

fn actionable_clause(reason: &str) -> String {
    let cut = [", so ", "; ", ", and "]
        .iter()
        .filter_map(|mark| reason.find(mark))
        .min()
        .unwrap_or(reason.len());
    reason[..cut].trim().to_string()
}

fn render(found: &Requirements) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "game studio crew {} on {}\n",
        found.app_version, found.os
    ));

    struct Row {
        kind: Kind,
        label: String,
        state: String,
        tag: &'static str,
        why: Option<String>,
    }

    let rows: Vec<Row> = found
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
            Row {
                kind: tool.kind,
                label: tool.label.clone(),
                state,
                tag,
                why: tool.cannot_drive.as_deref().map(actionable_clause),
            }
        })
        .collect();

    let widest = |pick: fn(&Row) -> usize| rows.iter().map(pick).max().unwrap_or(0).max(8);
    let labels = widest(|r| r.label.chars().count());
    let states = widest(|r| r.state.chars().count());

    for kind in Kind::ALL {
        out.push('\n');
        out.push_str(kind.heading());
        out.push('\n');
        for row in rows.iter().filter(|r| r.kind == kind) {
            let line = format!(
                "  {:<labels$}  {:<states$}  {}",
                row.label, row.state, row.tag
            );
            out.push_str(line.trim_end());
            out.push('\n');
            if let Some(why) = &row.why {
                out.push_str(&format!("  {:<labels$}  why: {why}\n", ""));
            }
        }
    }

    out.push('\n');
    if !found.ready {
        out.push_str("nothing to code with. Install one of ");
        out.push_str(&health::CODING_CLIS.join(", "));
        out.push_str(" and put it on PATH,\nthen run studiod doctor again.\n");
        out.push_str(&asset_advice(found));
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
    out.push_str(&asset_advice(found));
    out
}

fn recommended(name: &str) -> bool {
    matches!(name, "claude" | "codex")
}

fn porcelain_field(text: &str) -> String {
    text.replace(['\t', '\r', '\n'], " ").trim().to_string()
}

pub fn porcelain(found: &Requirements) -> String {
    let mut out = String::new();
    for kind in Kind::ALL {
        for tool in found.tools.iter().filter(|t| t.kind == kind) {
            let state = match (tool.present, tool.kind, tool.drivable) {
                (false, _, _) => "absent",
                (true, Kind::CodingCli, false) => "unusable",
                (true, _, _) => "ok",
            };
            let detail = match (tool.present, &tool.version, &tool.cannot_drive) {
                (_, _, Some(why)) => actionable_clause(why),
                (true, Some(v), None) => v.clone(),
                (true, None, None) => "present".into(),
                (false, _, None) => String::new(),
            };
            let fix = tool
                .install
                .as_ref()
                .and_then(|r| r.run.as_ref())
                .map(|run| run.join(" "))
                .unwrap_or_default();
            let advice = tool
                .install
                .as_ref()
                .map(|r| r.says.clone())
                .unwrap_or_default();

            let ticked = if recommended(&tool.name) { "on" } else { "off" };

            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                kind.key(),
                porcelain_field(&tool.label),
                state,
                porcelain_field(&detail),
                porcelain_field(&fix),
                porcelain_field(&advice),
                ticked,
            ));
        }
    }
    out
}

pub fn asset_gaps(found: &Requirements) -> Vec<&health::Tool> {
    found
        .tools
        .iter()
        .filter(|t| t.kind == Kind::Asset && !t.present)
        .collect()
}

fn asset_advice(found: &Requirements) -> String {
    let missing = asset_gaps(found);
    if missing.is_empty() {
        return "\nthe crew can draw, rig and animate its own art.\n".to_string();
    }

    let mut out = String::from(
        "\nthe crew cannot generate its own art yet. Each line below is one thing to install,\n\
         in this order, and the studio works without any of them:\n",
    );
    for (at, tool) in missing.iter().enumerate() {
        out.push_str(&format!("\n  {}. {}", at + 1, tool.label));
        if let Some(what) = &tool.needed_for {
            out.push_str(&format!("\n     needed for {what}"));
        }
        if let Some(why) = &tool.cannot_drive {
            out.push_str(&format!("\n     {}", actionable_clause(why)));
        }
        if let Some(remedy) = &tool.install {
            out.push_str(&format!("\n     {}", remedy.says));
            if let Some(run) = &remedy.run {
                out.push_str(&format!("\n     run: {}", run.join(" ")));
            }
        }
        out.push('\n');
    }
    out.push_str(
        "\nrun `studiod doctor --fix` to have the studio install the ones it can, or follow the\n\
         lines above by hand. Signing codex in is `codex login` and only you can do it.\n",
    );
    out
}

fn agreed() -> bool {
    if std::env::args().any(|a| a == "--yes") {
        return true;
    }
    print!("\ninstall these? [y/N] ");
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let mut said = String::new();
    if std::io::stdin().read_line(&mut said).is_err() {
        return false;
    }
    matches!(said.trim().to_lowercase().as_str(), "y" | "yes")
}

pub fn fix() -> anyhow::Result<()> {
    let found = health::probe();
    let missing = asset_gaps(&found);
    if missing.is_empty() {
        println!("nothing to install: the crew can already draw, rig and animate its own art.");
        return Ok(());
    }

    println!("this will change what is installed on this machine:\n");
    for tool in &missing {
        match tool.install.as_ref().and_then(|r| r.run.as_ref()) {
            Some(run) => println!("  {:<22} {}", tool.label, run.join(" ")),
            None => println!(
                "  {:<22} nothing to run: {}",
                tool.label,
                tool.install
                    .as_ref()
                    .map(|r| r.says.as_str())
                    .unwrap_or("no advice")
            ),
        }
    }
    if !agreed() {
        println!("\nnothing was installed.");
        return Ok(());
    }

    let mut ran = 0;
    let mut left = Vec::new();
    for tool in missing {
        let Some(remedy) = &tool.install else {
            left.push(format!("{}: {}", tool.label, "nothing to run"));
            continue;
        };
        let Some(run) = &remedy.run else {
            left.push(format!("{}: {}", tool.label, remedy.says));
            continue;
        };
        let Some((program, args)) = run.split_first() else {
            continue;
        };

        println!("\n{} is missing.", tool.label);
        println!("  running: {}", run.join(" "));
        let said = studio_core::command(program).args(args).status();
        match said {
            Ok(status) if status.success() => {
                println!("  installed {}", tool.label);
                ran += 1;
            }
            Ok(status) => left.push(format!(
                "{}: `{}` exited {}",
                tool.label,
                run.join(" "),
                status.code().unwrap_or(-1)
            )),
            Err(e) => left.push(format!("{}: could not run {program} ({e})", tool.label)),
        }
    }

    println!();
    if ran > 0 {
        println!("installed {ran}; open a new terminal so PATH is picked up, then run studiod doctor again.");
    }
    if !left.is_empty() {
        println!("still to do by hand:");
        for one in &left {
            println!("  {one}");
        }
    }
    Ok(())
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

    fn art_is_missing() -> Requirements {
        Requirements::new(vec![
            Tool::found("claude", "claude", Kind::CodingCli, Some("2.1.0".into())),
            Tool::absent("node", "node", Kind::Asset)
                .for_what("baking a model into a .glb")
                .fixed_by(health::node_remedy()),
            Tool::absent("python", "python with pillow", Kind::Asset)
                .for_what("removing the background from a generated sprite")
                .fixed_by(health::pillow_remedy("C:/Python313/python.exe")),
            Tool::absent("imagegen", "codex imagegen skill", Kind::Asset)
                .fixed_by(health::Remedy::told("open codex once so it unpacks its skills")),
        ])
    }

    #[test]
    fn a_studio_that_cannot_draw_is_told_what_to_install_one_line_at_a_time() {
        let said = render(&art_is_missing());

        assert!(said.contains("asset pipeline"));
        assert!(said.contains("1. node"), "{said}");
        assert!(said.contains("2. python with pillow"));
        assert!(said.contains("3. codex imagegen skill"));
        assert!(
            said.contains("run: C:/Python313/python.exe -m pip install pillow"),
            "the command has to name the interpreter that will run it: {said}"
        );
        assert!(said.contains("needed for removing the background"));
        assert!(said.contains("studiod doctor --fix"));
        assert!(
            said.contains("codex login"),
            "the one step the studio cannot do for the user has to be named"
        );
        assert!(
            said.contains("the studio works without any of them"),
            "an optional pipeline must never read as a broken install"
        );
    }

    #[test]
    fn a_complete_art_pipeline_says_so_and_asks_for_nothing() {
        let whole = Requirements::new(vec![
            Tool::found("claude", "claude", Kind::CodingCli, Some("2.1.0".into())),
            Tool::found("node", "node", Kind::Asset, Some("v22".into())),
            Tool::found("python", "python with pillow", Kind::Asset, None),
            Tool::found("imagegen", "codex imagegen skill", Kind::Asset, None),
        ]);
        let said = render(&whole);
        assert!(said.contains("the crew can draw, rig and animate its own art"));
        assert!(!said.contains("doctor --fix"));
        assert_eq!(exit_code(&whole), 0);
    }

    #[test]
    fn a_missing_art_pipeline_is_its_own_exit_code_and_never_the_two_that_mean_broken() {
        let found = art_is_missing();
        assert_eq!(exit_code(&found), ART_PIPELINE_INCOMPLETE);
        assert_ne!(ART_PIPELINE_INCOMPLETE, NOTHING_TO_CODE_WITH);
        assert_ne!(ART_PIPELINE_INCOMPLETE, NOTHING_THE_STUDIO_CAN_DRIVE);

        let nothing = Requirements::new(vec![Tool::absent("claude", "claude", Kind::CodingCli)]);
        assert_eq!(
            exit_code(&nothing),
            NOTHING_TO_CODE_WITH,
            "a studio with no CLI has a bigger problem than art"
        );
        assert!(
            render(&nothing).contains("asset pipeline"),
            "the art advice still has to be in the report the installer shows"
        );
    }

    #[test]
    fn every_missing_asset_tool_says_either_a_command_to_run_or_what_to_do_by_hand() {
        for tool in asset_gaps(&art_is_missing()) {
            let remedy = tool
                .install
                .as_ref()
                .unwrap_or_else(|| panic!("{} offers the user nothing", tool.label));
            assert!(!remedy.says.trim().is_empty());
            if let Some(run) = &remedy.run {
                assert!(!run.is_empty(), "{} has an empty command", tool.label);
            }
        }
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
        assert!(
            text.lines().any(|l| l.trim_start().starts_with("codex: ")),
            "an undrivable CLI is listed with the provider table's reason: {text}"
        );
    }

    #[test]
    fn the_reason_a_cli_cannot_drive_the_crew_is_shown_even_when_another_one_can() {
        let codex = Tool::found("codex", "codex", Kind::CodingCli, Some("0.9.1".into()));
        let reason = codex
            .cannot_drive
            .clone()
            .expect("a recognised but undrivable provider always says why");

        let found = Requirements::new(vec![
            Tool::found("claude", "claude", Kind::CodingCli, Some("2.1.0".into())),
            codex,
        ]);
        let text = render(&found);
        assert_eq!(exit_code(&found), 0, "claude can still drive the crew");

        let shown = text
            .lines()
            .find_map(|l| l.trim_start().strip_prefix("why: "))
            .expect("an undrivable CLI is never reported as a bare boolean");
        assert!(!shown.is_empty());
        assert!(
            reason.starts_with(shown),
            "the doctor must show the provider table's own words, not its own gloss:\n\
             shown:  {shown}\n\
             reason: {reason}"
        );
    }

    #[test]
    fn a_drivable_cli_is_not_given_a_reason_it_does_not_need() {
        let found = Requirements::new(vec![Tool::found(
            "claude",
            "claude",
            Kind::CodingCli,
            Some("2.1.0".into()),
        )]);
        assert!(!render(&found).contains("why:"));
    }

    #[test]
    fn a_long_blocker_is_cut_to_the_clause_that_names_the_problem() {
        let reason = "gemini has no flag that replaces the system prompt, so the frozen charter \
                      cannot be delivered and every spawn would pay full price; pick claude";
        assert_eq!(
            actionable_clause(reason),
            "gemini has no flag that replaces the system prompt"
        );
        assert_eq!(actionable_clause("no provider for codex"), "no provider for codex");
    }

    #[test]
    fn every_group_the_probe_fills_gets_a_heading() {
        let text = render(&only_claude_and_no_engine());
        for kind in Kind::ALL {
            assert!(text.contains(kind.heading()), "{:?} is never headed", kind);
        }
    }
}
