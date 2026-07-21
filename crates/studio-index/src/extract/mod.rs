mod csharp;
mod gdscript;

use crate::lang::Lang;
use tree_sitter::Node;

#[derive(Debug, Clone, PartialEq)]
pub struct Symbol {
    pub fqname: String,
    pub kind: String,
    pub signature: Option<String>,
    pub doc: Option<String>,
    pub line_start: u32,
    pub line_end: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ref {
    pub from_symbol: String,
    pub to_name: String,
    pub line: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Extraction {
    pub symbols: Vec<Symbol>,
    pub refs: Vec<Ref>,
}

pub fn extract(lang: Lang, path: &str, src: &str) -> Extraction {
    match lang {
        Lang::GdScript => gdscript::extract(path, src),
        Lang::CSharp => csharp::extract(src),
        Lang::Cpp => Extraction::default(),
    }
}

pub(crate) fn text<'a>(node: Node, src: &'a str) -> &'a str {
    node.utf8_text(src.as_bytes()).unwrap_or_default()
}

pub(crate) fn named_child_text<'a>(node: Node, field: &str, src: &'a str) -> Option<&'a str> {
    node.child_by_field_name(field).map(|n| text(n, src))
}

pub(crate) fn file_stem(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.rfind('.') {
        Some(dot) if dot > 0 => name[..dot].to_string(),
        _ => name.to_string(),
    }
}

pub(crate) fn qualify(scope: &str, name: &str) -> String {
    if scope.is_empty() {
        name.to_string()
    } else {
        format!("{scope}.{name}")
    }
}

pub(crate) fn leading_doc(src: &str, line_start: u32, markers: &[&str]) -> Option<String> {
    let lines: Vec<&str> = src.lines().collect();
    let mut collected: Vec<&str> = Vec::new();
    let mut cursor = line_start as usize;

    while cursor > 0 {
        let line = lines.get(cursor - 1)?.trim();
        let marker = markers.iter().find(|m| line.starts_with(**m));
        match marker {
            Some(m) => {
                collected.push(line[m.len()..].trim());
                cursor -= 1;
            }
            None => break,
        }
    }

    if collected.is_empty() {
        return None;
    }
    collected.reverse();
    let doc = collected.join(" ").trim().to_string();
    if doc.is_empty() {
        None
    } else {
        Some(truncate(&doc, 400))
    }
}

pub(crate) fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

pub(crate) fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stem_survives_both_separators_and_no_extension() {
        assert_eq!(file_stem("scripts/player.gd"), "player");
        assert_eq!(file_stem("scripts\\player.gd"), "player");
        assert_eq!(file_stem("Makefile"), "Makefile");
    }

    #[test]
    fn qualify_does_not_leave_a_leading_dot_at_file_scope() {
        assert_eq!(qualify("", "run"), "run");
        assert_eq!(qualify("Player", "run"), "Player.run");
    }

    #[test]
    fn a_doc_comment_block_is_read_upward_and_stops_at_code() {
        let src = "var unrelated = 1\n# first line\n# second line\nfunc go():\n\tpass\n";
        assert_eq!(
            leading_doc(src, 3, &["#"]),
            Some("first line second line".to_string())
        );
    }

    #[test]
    fn no_comment_above_a_declaration_yields_no_doc() {
        let src = "func go():\n\tpass\n";
        assert_eq!(leading_doc(src, 0, &["#"]), None);
    }

    #[test]
    fn truncation_is_by_character_not_byte_so_it_cannot_split_utf8() {
        let s = "ğüşiöç".repeat(100);
        let out = truncate(&s, 10);
        assert_eq!(out.chars().count(), 11);
    }
}
