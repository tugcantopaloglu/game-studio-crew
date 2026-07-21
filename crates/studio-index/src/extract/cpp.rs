use super::{
    collapse_whitespace, leading_doc, named_child_text, qualify, text, truncate, Extraction, Ref,
    Symbol,
};
use tree_sitter::{Node, Parser};

const DOC_MARKERS: &[&str] = &["///", "//"];

const REFLECTION_MACROS: &[&str] = &[
    "UCLASS",
    "USTRUCT",
    "UENUM",
    "UFUNCTION",
    "UPROPERTY",
    "UINTERFACE",
    "UDELEGATE",
    "UPARAM",
    "UMETA",
    "GENERATED_BODY",
    "GENERATED_UCLASS_BODY",
    "GENERATED_USTRUCT_BODY",
    "GENERATED_IINTERFACE_BODY",
];

pub fn extract(src: &str) -> Extraction {
    let cleaned = blank_reflection_macros(src);

    let mut parser = Parser::new();
    if parser.set_language(&tree_sitter_cpp::LANGUAGE.into()).is_err() {
        return Extraction::default();
    }
    let Some(tree) = parser.parse(&cleaned, None) else {
        return Extraction::default();
    };

    let mut out = Extraction::default();
    walk(tree.root_node(), &cleaned, "", &mut out);
    out
}

pub fn blank_reflection_macros(src: &str) -> String {
    let source: Vec<char> = src.chars().collect();
    let mut out = source.clone();
    let mut cursor = 0usize;

    while cursor < source.len() {
        if let Some(end) = reflection_macro_end(&source, cursor) {
            for slot in out.iter_mut().take(end).skip(cursor) {
                if *slot != '\n' {
                    *slot = ' ';
                }
            }
            cursor = end;
            continue;
        }

        if is_word_start(&source, cursor) {
            let end = word_end(&source, cursor);
            if is_export_macro(&source[cursor..end]) {
                for slot in out.iter_mut().take(end).skip(cursor) {
                    *slot = ' ';
                }
            }
            cursor = end;
            continue;
        }

        cursor += 1;
    }

    out.into_iter().collect()
}

fn reflection_macro_end(source: &[char], at: usize) -> Option<usize> {
    if !is_word_start(source, at) {
        return None;
    }
    let end = word_end(source, at);
    let word: String = source[at..end].iter().collect();
    if !REFLECTION_MACROS.contains(&word.as_str()) {
        return None;
    }
    balanced_paren_end(source, end)
}

fn balanced_paren_end(source: &[char], from: usize) -> Option<usize> {
    let mut cursor = from;
    while cursor < source.len() && source[cursor].is_whitespace() {
        cursor += 1;
    }
    if source.get(cursor) != Some(&'(') {
        return None;
    }

    let mut depth = 0usize;
    while cursor < source.len() {
        match source[cursor] {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(cursor + 1);
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    None
}

fn is_export_macro(word: &[char]) -> bool {
    let text: String = word.iter().collect();
    text.len() > 4
        && text.ends_with("_API")
        && text
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

fn is_word_start(source: &[char], at: usize) -> bool {
    let current = source.get(at).copied().unwrap_or(' ');
    let starts = current.is_alphabetic() || current == '_';
    let boundary = at == 0 || !is_word_char(source[at - 1]);
    starts && boundary
}

fn word_end(source: &[char], from: usize) -> usize {
    let mut end = from;
    while end < source.len() && is_word_char(source[end]) {
        end += 1;
    }
    end
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn walk(node: Node, src: &str, scope: &str, out: &mut Extraction) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "namespace_definition" => descend(child, src, scope, None, out),
            "class_specifier" => descend(child, src, scope, Some("class"), out),
            "struct_specifier" => descend(child, src, scope, Some("struct"), out),
            "union_specifier" => descend(child, src, scope, Some("union"), out),
            "enum_specifier" => descend(child, src, scope, Some("enum"), out),
            "enumerator" => {
                push(child, src, scope, "enum_member", out);
            }
            "function_definition" => push_function(child, src, scope, out),
            "declaration" | "field_declaration" => push_member(child, src, scope, out),
            "translation_unit" | "declaration_list" | "field_declaration_list"
            | "enumerator_list" | "linkage_specification" => walk(child, src, scope, out),
            _ => {}
        }
    }
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
    }
}

fn push_member(node: Node, src: &str, scope: &str, out: &mut Extraction) {
    let Some(declarator) = node.child_by_field_name("declarator") else {
        return;
    };

    let kind = if innermost_declarator(declarator).0 { "method" } else { "field" };
    let (_, name_node) = innermost_declarator(declarator);
    let Some(name_node) = name_node else { return };

    let (extra_scope, name) = split_qualified(name_node, src);
    let owner = qualify(scope, &extra_scope);
    emit(node, src, qualify(&owner, &name), kind, out);
}

fn push_function(node: Node, src: &str, scope: &str, out: &mut Extraction) {
    let Some(declarator) = node.child_by_field_name("declarator") else {
        return;
    };
    let (_, name_node) = innermost_declarator(declarator);
    let Some(name_node) = name_node else { return };

    let (extra_scope, name) = split_qualified(name_node, src);
    let owner = qualify(scope, &extra_scope);
    let fqname = qualify(&owner, &name);

    emit(node, src, fqname.clone(), "method", out);

    if let Some(body) = node.child_by_field_name("body") {
        collect_refs(body, src, &fqname, out);
    }
}

fn innermost_declarator<'a>(node: Node<'a>) -> (bool, Option<Node<'a>>) {
    match node.kind() {
        "identifier" | "field_identifier" | "qualified_identifier" | "destructor_name"
        | "operator_name" => (false, Some(node)),
        "function_declarator" => {
            let inner = node.child_by_field_name("declarator");
            (true, inner.and_then(|n| innermost_declarator(n).1))
        }
        "pointer_declarator" | "reference_declarator" | "array_declarator"
        | "init_declarator" | "parenthesized_declarator" => {
            match node.child_by_field_name("declarator") {
                Some(inner) => innermost_declarator(inner),
                None => (false, None),
            }
        }
        _ => (false, None),
    }
}

fn split_qualified(node: Node, src: &str) -> (String, String) {
    if node.kind() != "qualified_identifier" {
        return (String::new(), text(node, src).to_string());
    }

    let scope = node
        .child_by_field_name("scope")
        .map(|n| text(n, src).to_string())
        .unwrap_or_default();

    match node.child_by_field_name("name") {
        Some(name) => {
            let (deeper, leaf) = split_qualified(name, src);
            (qualify(&scope, &deeper), leaf)
        }
        None => (String::new(), text(node, src).to_string()),
    }
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

fn push(node: Node, src: &str, scope: &str, kind: &str, out: &mut Extraction) -> Option<String> {
    let name = named_child_text(node, "name", src)?;
    let fqname = qualify(scope, name);
    emit(node, src, fqname.clone(), kind, out);
    Some(fqname)
}

fn signature(node: Node, src: &str) -> String {
    let full = text(node, src);
    let head = match node.child_by_field_name("body") {
        Some(body) => {
            let end = body.start_byte().saturating_sub(node.start_byte());
            full.get(..end).unwrap_or(full)
        }
        None => full,
    };
    truncate(
        &collapse_whitespace(head.trim_end_matches([';', '{', ' ', '\n', '\t'])),
        300,
    )
}

fn collect_refs(node: Node, src: &str, from: &str, out: &mut Extraction) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "call_expression" {
            if let Some(function) = child.child_by_field_name("function") {
                let name = match function.kind() {
                    "identifier" => Some(text(function, src).to_string()),
                    "field_expression" => {
                        named_child_text(function, "field", src).map(str::to_string)
                    }
                    "qualified_identifier" => Some(split_qualified(function, src).1),
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

#[cfg(test)]
mod tests {
    use super::*;

    const PAWN: &str = r####"#pragma once

UCLASS()
class GAME_API APlayerPawn : public APawn {
    GENERATED_BODY()
public:
    /// how fast the pawn moves
    UPROPERTY(EditAnywhere, Category = "Movement") float Speed = 600.f;

    virtual void Tick(float DeltaTime) override;
private:
    void ApplyDamage(int32 Amount);
};

namespace Game {
    struct FStats { int32 Hp; };
    enum class EState : uint8 { Idle UMETA(DisplayName="Idle"), Run };
}

void APlayerPawn::ApplyDamage(int32 Amount) {
    Speed -= Amount;
    Tick(0.f);
}
"####;

    fn extraction() -> Extraction {
        extract(PAWN)
    }

    fn kind_of(e: &Extraction, fqname: &str) -> Option<String> {
        e.symbols.iter().find(|s| s.fqname == fqname).map(|s| s.kind.clone())
    }

    #[test]
    fn no_symbol_carries_a_doubled_or_dangling_separator() {
        let e = extraction();
        for symbol in &e.symbols {
            assert!(!symbol.fqname.contains(".."), "{}", symbol.fqname);
            assert!(!symbol.fqname.ends_with('.'), "{}", symbol.fqname);
            assert!(!symbol.fqname.starts_with('.'), "{}", symbol.fqname);
        }
    }

    #[test]
    fn blanking_preserves_length_and_line_count_so_line_numbers_stay_true() {
        let cleaned = blank_reflection_macros(PAWN);
        assert_eq!(cleaned.chars().count(), PAWN.chars().count());
        assert_eq!(cleaned.lines().count(), PAWN.lines().count());
    }

    #[test]
    fn unreal_macros_no_longer_defeat_the_grammar() {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_cpp::LANGUAGE.into()).unwrap();

        let raw = parser.parse(PAWN, None).unwrap();
        assert!(raw.root_node().has_error());

        let cleaned = blank_reflection_macros(PAWN);
        let fixed = parser.parse(&cleaned, None).unwrap();
        assert!(!fixed.root_node().has_error());
    }

    #[test]
    fn a_uclass_and_its_members_are_extracted() {
        let e = extraction();
        assert_eq!(kind_of(&e, "APlayerPawn").as_deref(), Some("class"));
        assert_eq!(kind_of(&e, "APlayerPawn.Speed").as_deref(), Some("field"));
        assert_eq!(kind_of(&e, "APlayerPawn.Tick").as_deref(), Some("method"));
        assert_eq!(kind_of(&e, "APlayerPawn.ApplyDamage").as_deref(), Some("method"));
    }

    #[test]
    fn a_namespaced_struct_and_enum_are_scoped_under_the_namespace() {
        let e = extraction();
        assert_eq!(kind_of(&e, "Game.FStats").as_deref(), Some("struct"));
        assert_eq!(kind_of(&e, "Game.FStats.Hp").as_deref(), Some("field"));
        assert_eq!(kind_of(&e, "Game.EState").as_deref(), Some("enum"));
        assert_eq!(kind_of(&e, "Game.EState.Idle").as_deref(), Some("enum_member"));
    }

    #[test]
    fn an_out_of_line_definition_lands_under_the_class_that_owns_it() {
        let e = extraction();
        let definitions: Vec<&Symbol> = e
            .symbols
            .iter()
            .filter(|s| s.fqname == "APlayerPawn.ApplyDamage")
            .collect();
        assert_eq!(definitions.len(), 2);
    }

    #[test]
    fn a_doc_comment_survives_the_blanked_macro_between_it_and_the_field() {
        let e = extraction();
        let speed = e.symbols.iter().find(|s| s.fqname == "APlayerPawn.Speed").unwrap();
        assert_eq!(speed.doc.as_deref(), Some("how fast the pawn moves"));
    }

    #[test]
    fn calls_in_an_out_of_line_body_are_owned_by_that_definition() {
        let e = extraction();
        let names: Vec<&str> = e
            .refs
            .iter()
            .filter(|r| r.from_symbol == "APlayerPawn.ApplyDamage")
            .map(|r| r.to_name.as_str())
            .collect();
        assert!(names.contains(&"Tick"));
    }

    #[test]
    fn a_member_call_through_this_is_recorded_by_its_field_name() {
        let e = extract("void A::B() {\n  this->Helper();\n}\n");
        assert!(e.refs.iter().any(|r| r.to_name == "Helper"));
    }

    #[test]
    fn plain_cpp_without_any_unreal_macros_still_works() {
        let e = extract("namespace N {\nclass C {\npublic:\n  void go();\n};\n}\n");
        assert_eq!(kind_of(&e, "N.C").as_deref(), Some("class"));
        assert_eq!(kind_of(&e, "N.C.go").as_deref(), Some("method"));
    }

    #[test]
    fn an_export_macro_is_blanked_but_a_normal_uppercase_name_is_not() {
        let cleaned = blank_reflection_macros("class GAME_API A {}; int MAX_HP = 3;");
        assert!(!cleaned.contains("GAME_API"));
        assert!(cleaned.contains("MAX_HP"));
    }
}
