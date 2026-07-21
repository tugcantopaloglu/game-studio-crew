use super::{
    collapse_whitespace, file_stem, first_line, last_line, leading_doc, named_child_text, qualify,
    row, text, truncate,
    Extraction, Ref, Symbol,
};
use tree_sitter::{Node, Parser};

const DOC_MARKERS: &[&str] = &["##", "#"];

pub fn extract(path: &str, src: &str) -> Extraction {
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_gdscript::LANGUAGE.into())
        .is_err()
    {
        return Extraction::default();
    }
    let Some(tree) = parser.parse(src, None) else {
        return Extraction::default();
    };

    let root = tree.root_node();
    let scope = declared_class_name(root, src).unwrap_or_else(|| file_stem(path));

    let mut out = Extraction::default();
    walk(root, src, &scope, &mut out);
    out
}

fn declared_class_name(root: Node, src: &str) -> Option<String> {
    let mut cursor = root.walk();
    let declared = root
        .named_children(&mut cursor)
        .find(|n| n.kind() == "class_name_statement")
        .and_then(|n| named_child_text(n, "name", src))
        .map(str::to_string);
    declared
}

fn walk(node: Node, src: &str, scope: &str, out: &mut Extraction) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "signal_statement" => {
                push(child, src, scope, "signal", out);
            }
            "const_statement" => {
                push(child, src, scope, "const", out);
            }
            "enum_definition" => {
                push(child, src, scope, "enum", out);
            }
            "variable_statement" => {
                push(child, src, scope, "var", out);
            }
            "function_definition" => {
                if let Some(fqname) = push(child, src, scope, "func", out) {
                    if let Some(body) = child.child_by_field_name("body") {
                        collect_refs(body, src, &fqname, out);
                    }
                }
            }
            "class_definition" => {
                if let Some(name) = named_child_text(child, "name", src) {
                    let inner = qualify(scope, name);
                    push(child, src, scope, "class", out);
                    if let Some(body) = child.child_by_field_name("body") {
                        walk(body, src, &inner, out);
                    }
                }
            }
            "class_body" | "source" => walk(child, src, scope, out),
            _ => {}
        }
    }
}

fn push(
    node: Node,
    src: &str,
    scope: &str,
    kind: &str,
    out: &mut Extraction,
) -> Option<String> {
    let name = named_child_text(node, "name", src)?;
    let fqname = qualify(scope, name);
    let declaration_row = row(node);

    out.symbols.push(Symbol {
        fqname: fqname.clone(),
        kind: kind.to_string(),
        signature: Some(signature(node, src)),
        doc: leading_doc(src, declaration_row, DOC_MARKERS),
        line_start: first_line(node),
        line_end: last_line(node),
    });

    Some(fqname)
}

fn signature(node: Node, src: &str) -> String {
    let full = text(node, src);
    let head = match node.child_by_field_name("body") {
        Some(body) => {
            let end = body.start_byte().saturating_sub(node.start_byte());
            full.get(..end).unwrap_or(full)
        }
        None => full.lines().next().unwrap_or(full),
    };
    truncate(&collapse_whitespace(head.trim_end_matches([':', ' ', '\n', '\t'])), 300)
}

fn collect_refs(node: Node, src: &str, from: &str, out: &mut Extraction) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        let callee = match child.kind() {
            "call" => child.named_child(0).filter(|n| n.kind() == "identifier"),
            "attribute_call" => child.named_child(0).filter(|n| n.kind() == "identifier"),
            _ => None,
        };

        if let Some(name) = callee {
            out.refs.push(Ref {
                from_symbol: from.to_string(),
                to_name: text(name, src).to_string(),
                line: first_line(child),
            });
        }

        collect_refs(child, src, from, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYER: &str = r#"class_name Player
extends CharacterBody2D

## the player died
signal died(cause: String)

const SPEED := 300.0
enum State { IDLE, RUN }

@export var health: int = 100

class Inner:
	var x = 1

func _physics_process(delta: float) -> void:
	move_and_slide()
	_apply(delta)
"#;

    fn extraction() -> Extraction {
        extract("scripts/player.gd", PLAYER)
    }

    fn kinds(e: &Extraction, fqname: &str) -> Option<String> {
        e.symbols.iter().find(|s| s.fqname == fqname).map(|s| s.kind.clone())
    }

    #[test]
    fn class_name_wins_over_the_file_stem_as_the_scope() {
        let e = extraction();
        assert!(e.symbols.iter().any(|s| s.fqname == "Player._physics_process"));
        assert!(!e.symbols.iter().any(|s| s.fqname.starts_with("player.")));
    }

    #[test]
    fn a_file_without_class_name_falls_back_to_its_stem() {
        let e = extract("scripts/util.gd", "func helper():\n\tpass\n");
        assert_eq!(e.symbols[0].fqname, "util.helper");
    }

    #[test]
    fn every_declaration_kind_is_extracted() {
        let e = extraction();
        assert_eq!(kinds(&e, "Player.died").as_deref(), Some("signal"));
        assert_eq!(kinds(&e, "Player.SPEED").as_deref(), Some("const"));
        assert_eq!(kinds(&e, "Player.State").as_deref(), Some("enum"));
        assert_eq!(kinds(&e, "Player.health").as_deref(), Some("var"));
        assert_eq!(kinds(&e, "Player.Inner").as_deref(), Some("class"));
        assert_eq!(kinds(&e, "Player._physics_process").as_deref(), Some("func"));
    }

    #[test]
    fn an_inner_class_member_is_scoped_under_the_inner_class() {
        let e = extraction();
        assert!(e.symbols.iter().any(|s| s.fqname == "Player.Inner.x"));
    }

    #[test]
    fn a_signature_stops_before_the_body() {
        let e = extraction();
        let f = e.symbols.iter().find(|s| s.fqname == "Player._physics_process").unwrap();
        assert_eq!(
            f.signature.as_deref(),
            Some("func _physics_process(delta: float) -> void")
        );
    }

    #[test]
    fn a_double_hash_doc_comment_is_attached_to_the_signal() {
        let e = extraction();
        let s = e.symbols.iter().find(|s| s.fqname == "Player.died").unwrap();
        assert_eq!(s.doc.as_deref(), Some("the player died"));
    }

    #[test]
    fn calls_inside_a_function_become_refs_owned_by_that_function() {
        let e = extraction();
        let names: Vec<&str> = e
            .refs
            .iter()
            .filter(|r| r.from_symbol == "Player._physics_process")
            .map(|r| r.to_name.as_str())
            .collect();
        assert!(names.contains(&"move_and_slide"));
        assert!(names.contains(&"_apply"));
    }

    #[test]
    fn line_numbers_are_one_based_the_way_an_editor_counts_them() {
        let e = extraction();
        let f = e.symbols.iter().find(|s| s.fqname == "Player._physics_process").unwrap();

        let declared_at = PLAYER
            .lines()
            .position(|l| l.starts_with("func _physics_process"))
            .unwrap() as u32;

        assert_eq!(f.line_start, declared_at + 1);
        assert_eq!(PLAYER.lines().nth(f.line_start as usize - 1).unwrap(), "func _physics_process(delta: float) -> void:");
    }

    #[test]
    fn a_ref_line_is_one_based_too() {
        let e = extraction();
        let call = e.refs.iter().find(|r| r.to_name == "move_and_slide").unwrap();
        assert!(PLAYER.lines().nth(call.line as usize - 1).unwrap().contains("move_and_slide"));
    }

    #[test]
    fn a_syntax_error_yields_what_parsed_rather_than_panicking() {
        let e = extract("broken.gd", "func ok():\n\tpass\n\nfunc (((\n");
        assert!(e.symbols.iter().any(|s| s.fqname == "broken.ok"));
    }
}
