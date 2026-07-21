use super::{
    collapse_whitespace, leading_doc, named_child_text, qualify, text, truncate, Extraction, Ref,
    Symbol,
};
use tree_sitter::{Node, Parser};

const DOC_MARKERS: &[&str] = &["///", "//"];

pub fn extract(src: &str) -> Extraction {
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
        .is_err()
    {
        return Extraction::default();
    }
    let Some(tree) = parser.parse(src, None) else {
        return Extraction::default();
    };

    let mut out = Extraction::default();
    walk(tree.root_node(), src, "", &mut out);
    out
}

fn walk(node: Node, src: &str, outer: &str, out: &mut Extraction) {
    let scope = match file_scoped_namespace(node, src) {
        Some(name) => qualify(outer, &name),
        None => outer.to_string(),
    };
    let scope = scope.as_str();

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "file_scoped_namespace_declaration" => {}
            "namespace_declaration" => descend(child, src, scope, None, out),
            "class_declaration" => descend(child, src, scope, Some("class"), out),
            "struct_declaration" => descend(child, src, scope, Some("struct"), out),
            "interface_declaration" => descend(child, src, scope, Some("interface"), out),
            "record_declaration" => descend(child, src, scope, Some("record"), out),
            "enum_declaration" => descend(child, src, scope, Some("enum"), out),
            "method_declaration" | "constructor_declaration" | "local_function_statement" => {
                if let Some(fqname) = push(child, src, scope, "method", out) {
                    if let Some(body) = child.child_by_field_name("body") {
                        collect_refs(body, src, &fqname, out);
                    }
                }
            }
            "property_declaration" => {
                push(child, src, scope, "property", out);
            }
            "field_declaration" => push_variables(child, src, scope, "field", out),
            "event_field_declaration" => push_variables(child, src, scope, "event", out),
            "enum_member_declaration" => {
                push(child, src, scope, "enum_member", out);
            }
            "compilation_unit" | "declaration_list" | "enum_member_declaration_list" => {
                walk(child, src, scope, out)
            }
            _ => {}
        }
    }
}

fn file_scoped_namespace(node: Node, src: &str) -> Option<String> {
    let declaration = child_of_kind(node, "file_scoped_namespace_declaration")?;
    named_child_text(declaration, "name", src).map(str::to_string)
}

fn descend(node: Node, src: &str, scope: &str, kind: Option<&str>, out: &mut Extraction) {
    let Some(name) = named_child_text(node, "name", src) else {
        return;
    };
    let inner = qualify(scope, name);

    if let Some(kind) = kind {
        push(node, src, scope, kind, out);
    }

    if let Some(body) = node.child_by_field_name("body") {
        walk(body, src, &inner, out);
    } else {
        walk(node, src, &inner, out);
    }
}

fn push_variables(node: Node, src: &str, scope: &str, kind: &str, out: &mut Extraction) {
    let Some(declaration) = child_of_kind(node, "variable_declaration") else {
        return;
    };
    let mut cursor = declaration.walk();
    for declarator in declaration.named_children(&mut cursor) {
        if declarator.kind() != "variable_declarator" {
            continue;
        }
        let Some(name) = named_child_text(declarator, "name", src) else {
            continue;
        };
        emit(node, src, qualify(scope, name), kind, out);
    }
}

fn push(node: Node, src: &str, scope: &str, kind: &str, out: &mut Extraction) -> Option<String> {
    let name = named_child_text(node, "name", src)?;
    let fqname = qualify(scope, name);
    emit(node, src, fqname.clone(), kind, out);
    Some(fqname)
}

fn emit(node: Node, src: &str, fqname: String, kind: &str, out: &mut Extraction) {
    let line_start = node.start_position().row as u32;
    out.symbols.push(Symbol {
        fqname,
        kind: kind.to_string(),
        signature: Some(signature(node, src)),
        doc: leading_doc(src, line_start, DOC_MARKERS),
        line_start,
        line_end: node.end_position().row as u32,
    });
}

fn signature(node: Node, src: &str) -> String {
    let full = text(node, src);
    let cut = node
        .child_by_field_name("body")
        .or_else(|| node.child_by_field_name("accessors"))
        .map(|b| b.start_byte().saturating_sub(node.start_byte()));

    let head = match cut {
        Some(end) => full.get(..end).unwrap_or(full),
        None => full,
    };
    truncate(&collapse_whitespace(head.trim_end_matches([';', ' ', '\n', '\t'])), 300)
}

fn collect_refs(node: Node, src: &str, from: &str, out: &mut Extraction) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "invocation_expression" {
            if let Some(function) = child.child_by_field_name("function") {
                let name = match function.kind() {
                    "identifier" => Some(text(function, src).to_string()),
                    "member_access_expression" => named_child_text(function, "name", src)
                        .map(str::to_string),
                    _ => None,
                };
                if let Some(name) = name {
                    out.refs.push(Ref {
                        from_symbol: from.to_string(),
                        to_name: name,
                        line: child.start_position().row as u32,
                    });
                }
            }
        }

        collect_refs(child, src, from, out);
    }
}

fn child_of_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let found = node.named_children(&mut cursor).find(|n| n.kind() == kind);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOVER: &str = r#"using UnityEngine;

namespace Game.Player {
  /// moves the pawn
  public class Mover : MonoBehaviour {
    [SerializeField] private float speed = 5f;
    public int Health { get; set; }
    public event System.Action Died;

    void Update() {
      transform.Translate(Vector3.one);
      Apply(speed);
    }
  }

  public enum State { Idle, Run }
}
"#;

    fn extraction() -> Extraction {
        extract(MOVER)
    }

    fn kind_of(e: &Extraction, fqname: &str) -> Option<String> {
        e.symbols.iter().find(|s| s.fqname == fqname).map(|s| s.kind.clone())
    }

    #[test]
    fn a_namespace_qualifies_without_becoming_a_symbol_itself() {
        let e = extraction();
        assert_eq!(kind_of(&e, "Game.Player.Mover").as_deref(), Some("class"));
        assert!(!e.symbols.iter().any(|s| s.fqname == "Game.Player"));
    }

    #[test]
    fn members_are_scoped_under_their_class() {
        let e = extraction();
        assert_eq!(kind_of(&e, "Game.Player.Mover.Update").as_deref(), Some("method"));
        assert_eq!(kind_of(&e, "Game.Player.Mover.speed").as_deref(), Some("field"));
        assert_eq!(kind_of(&e, "Game.Player.Mover.Health").as_deref(), Some("property"));
        assert_eq!(kind_of(&e, "Game.Player.Mover.Died").as_deref(), Some("event"));
    }

    #[test]
    fn an_enum_and_its_members_both_land() {
        let e = extraction();
        assert_eq!(kind_of(&e, "Game.Player.State").as_deref(), Some("enum"));
        assert_eq!(kind_of(&e, "Game.Player.State.Idle").as_deref(), Some("enum_member"));
    }

    #[test]
    fn a_triple_slash_doc_is_attached_to_the_class() {
        let e = extraction();
        let c = e.symbols.iter().find(|s| s.fqname == "Game.Player.Mover").unwrap();
        assert_eq!(c.doc.as_deref(), Some("moves the pawn"));
    }

    #[test]
    fn a_method_signature_stops_before_its_block() {
        let e = extraction();
        let m = e.symbols.iter().find(|s| s.fqname == "Game.Player.Mover.Update").unwrap();
        assert_eq!(m.signature.as_deref(), Some("void Update()"));
    }

    #[test]
    fn both_bare_and_member_calls_become_refs() {
        let e = extraction();
        let names: Vec<&str> = e
            .refs
            .iter()
            .filter(|r| r.from_symbol == "Game.Player.Mover.Update")
            .map(|r| r.to_name.as_str())
            .collect();
        assert!(names.contains(&"Apply"));
        assert!(names.contains(&"Translate"));
    }

    #[test]
    fn a_file_scoped_namespace_qualifies_the_same_way() {
        let e = extract("namespace Game;\npublic class A { void B() {} }\n");
        assert!(e.symbols.iter().any(|s| s.fqname == "Game.A.B"));
    }

    #[test]
    fn one_field_declaration_with_several_names_yields_one_symbol_each() {
        let e = extract("class A { int x, y; }");
        assert!(e.symbols.iter().any(|s| s.fqname == "A.x"));
        assert!(e.symbols.iter().any(|s| s.fqname == "A.y"));
    }
}
