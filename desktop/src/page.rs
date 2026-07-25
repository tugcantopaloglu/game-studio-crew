use crate::daemon::Failure;

const MARK: &str = include_str!("../assets/mark.b64");

const STYLE: &str = r#"
:root {
  --bg: #08090d; --panel: #0d1015; --line: rgba(148,163,184,.17);
  --text: #e4e8f0; --dim: #8a93a4; --faint: #545d6d; --error: #e2636d;
  --sans: "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif;
  --mono: "Cascadia Code", ui-monospace, Consolas, monospace;
}
* { box-sizing: border-box; }
html, body { height: 100%; margin: 0; }
body { background: var(--bg); color: var(--text); font: 13px/1.6 var(--sans);
  display: flex; align-items: center; justify-content: center; padding: 40px; }
main { max-width: 720px; width: 100%; }
h1 { font-size: 17px; font-weight: 600; letter-spacing: .2px; margin: 0 0 6px; }
h1.bad { color: var(--error); }
p { color: var(--dim); margin: 0 0 18px; }
pre { background: var(--panel); border: 1px solid var(--line); border-radius: 8px;
  padding: 14px 16px; margin: 0 0 18px; max-height: 46vh; overflow: auto;
  font: 11.5px/1.5 var(--mono); color: var(--dim); white-space: pre-wrap; }
.hint { color: var(--faint); font-size: 12px; }
.mark { display: block; height: 46px; width: auto; margin: 0 0 22px; opacity: .9; }
.dots::after { content: ""; animation: dots 1.4s steps(4, end) infinite; }
@keyframes dots { 0% { content: ""; } 25% { content: "."; } 50% { content: ".."; } 75% { content: "..."; } }
"#;

fn page(title: &str, body: String) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <title>{title}</title><style>{STYLE}</style></head><body><main>\
         <img class=\"mark\" src=\"data:image/png;base64,{MARK}\" alt=\"\">\
         {body}</main></body></html>"
    )
}

pub fn starting() -> String {
    page(
        "Game Studio Crew",
        "<h1>Starting the studio<span class=\"dots\"></span></h1>\
         <p>Checking what is installed, then bringing the daemon up on 127.0.0.1.</p>"
            .into(),
    )
}

pub fn failure(failure: &Failure) -> String {
    let mut body = format!("<h1 class=\"bad\">{}</h1>", escape(&failure.headline));
    if !failure.what_to_do.is_empty() {
        body.push_str(&format!("<p>{}</p>", escape(&failure.what_to_do)));
    }
    if !failure.detail.trim().is_empty() {
        body.push_str(&format!("<pre>{}</pre>", escape(failure.detail.trim())));
    }
    body.push_str(
        "<div class=\"hint\">Closing this window stops the daemon and every worker it started.</div>",
    );
    page("Game Studio Crew", body)
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_daemon_message_cannot_inject_markup_into_the_window() {
        let stopped = Failure {
            headline: "the studio daemon stopped".into(),
            detail: "<script>alert('x')</script> & <b>bold</b>".into(),
            what_to_do: "Start it again.".into(),
        };
        let html = failure(&stopped);
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("&amp;"));
    }

    #[test]
    fn a_failure_page_always_says_what_to_do_next() {
        let html = failure(&Failure {
            headline: "there is nothing to code with".into(),
            detail: String::new(),
            what_to_do: "Install one coding CLI.".into(),
        });
        assert!(html.contains("Install one coding CLI."));
        assert!(!html.contains("<pre>"), "an empty detail leaves no empty box");
    }

    #[test]
    fn the_splash_says_what_is_happening_before_the_floor_loads() {
        assert!(starting().contains("Starting the studio"));
    }

    #[test]
    fn both_shell_pages_carry_the_studio_mark() {
        let broken = failure(&Failure {
            headline: "stopped".into(),
            detail: String::new(),
            what_to_do: String::new(),
        });
        for html in [starting(), broken] {
            assert!(html.contains("data:image/png;base64,iVBOR"), "the mark is missing");
        }
        assert!(!MARK.contains(char::is_whitespace), "a wrapped blob breaks the data url");
    }
}
