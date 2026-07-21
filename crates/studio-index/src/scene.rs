#[derive(Debug, Clone, PartialEq)]
pub struct SceneNode {
    pub node_path: String,
    pub node_type: Option<String>,
    pub parent_path: Option<String>,
    pub script: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scene {
    pub asset_type: String,
    pub uid: Option<String>,
    pub nodes: Vec<SceneNode>,
}

pub const ROOT_PATH: &str = ".";

pub fn parse(src: &str) -> Option<Scene> {
    let mut scene = Scene::default();
    let mut external: Vec<(String, String)> = Vec::new();
    let mut pending: Option<SceneNode> = None;
    let mut seen_header = false;

    for line in src.lines() {
        let line = line.trim();

        if let Some(header) = section_header(line) {
            if let Some(node) = pending.take() {
                scene.nodes.push(node);
            }

            let (tag, attributes) = split_tag(header);
            match tag {
                "gd_scene" => {
                    seen_header = true;
                    scene.asset_type = "scene".into();
                    scene.uid = attribute(&attributes, "uid");
                }
                "gd_resource" => {
                    seen_header = true;
                    scene.asset_type = attribute(&attributes, "type")
                        .map(|t| t.to_ascii_lowercase())
                        .unwrap_or_else(|| "resource".into());
                    scene.uid = attribute(&attributes, "uid");
                }
                "ext_resource" => {
                    if let (Some(id), Some(path)) =
                        (attribute(&attributes, "id"), attribute(&attributes, "path"))
                    {
                        external.push((id, strip_res_prefix(&path)));
                    }
                }
                "node" => {
                    if let Some(name) = attribute(&attributes, "name") {
                        let parent = attribute(&attributes, "parent");
                        pending = Some(SceneNode {
                            node_path: node_path(&name, parent.as_deref()),
                            node_type: attribute(&attributes, "type"),
                            parent_path: parent,
                            script: None,
                        });
                    }
                }
                _ => {}
            }
            continue;
        }

        if let Some(node) = pending.as_mut() {
            if let Some(id) = script_resource_id(line) {
                node.script = external
                    .iter()
                    .find(|(external_id, _)| *external_id == id)
                    .map(|(_, path)| path.clone());
            }
        }
    }

    if let Some(node) = pending.take() {
        scene.nodes.push(node);
    }

    if !seen_header {
        return None;
    }
    Some(scene)
}

fn section_header(line: &str) -> Option<&str> {
    let inner = line.strip_prefix('[')?.strip_suffix(']')?;
    if inner.is_empty() {
        None
    } else {
        Some(inner)
    }
}

fn split_tag(header: &str) -> (&str, &str) {
    match header.find(' ') {
        Some(space) => (&header[..space], &header[space + 1..]),
        None => (header, ""),
    }
}

fn attribute(attributes: &str, key: &str) -> Option<String> {
    let mut rest = attributes;
    while let Some(position) = rest.find(key) {
        let after = &rest[position + key.len()..];
        let before_ok = position == 0
            || rest[..position]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_whitespace());

        if before_ok {
            if let Some(value) = after.strip_prefix('=') {
                return Some(read_value(value));
            }
        }
        rest = &rest[position + key.len()..];
    }
    None
}

fn read_value(value: &str) -> String {
    let value = value.trim_start();
    match value.strip_prefix('"') {
        Some(quoted) => quoted.split('"').next().unwrap_or_default().to_string(),
        None => value.split_whitespace().next().unwrap_or_default().to_string(),
    }
}

fn node_path(name: &str, parent: Option<&str>) -> String {
    match parent {
        None => ROOT_PATH.to_string(),
        Some(p) if p == ROOT_PATH => name.to_string(),
        Some(p) => format!("{p}/{name}"),
    }
}

fn script_resource_id(line: &str) -> Option<String> {
    let value = line.strip_prefix("script")?.trim_start().strip_prefix('=')?;
    let inside = value.trim().strip_prefix("ExtResource(")?.strip_suffix(')')?;
    Some(inside.trim().trim_matches('"').to_string())
}

fn strip_res_prefix(path: &str) -> String {
    path.strip_prefix("res://").unwrap_or(path).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN: &str = r#"[gd_scene load_steps=4 format=3 uid="uid://bqk8x2vn1a3ym"]

[ext_resource type="Script" path="res://scripts/player.gd" id="1_p4yer"]
[ext_resource type="Texture2D" path="res://art/hero.png" id="2_hero"]

[sub_resource type="RectangleShape2D" id="RectangleShape2D_a1"]
size = Vector2(32, 48)

[node name="Main" type="Node2D"]

[node name="Player" type="CharacterBody2D" parent="."]
position = Vector2(100, 200)
script = ExtResource("1_p4yer")

[node name="Sprite" type="Sprite2D" parent="Player"]
texture = ExtResource("2_hero")

[node name="Muzzle" type="Marker2D" parent="Player/Sprite"]
"#;

    fn main_scene() -> Scene {
        parse(MAIN).unwrap()
    }

    fn node<'a>(scene: &'a Scene, path: &str) -> &'a SceneNode {
        scene.nodes.iter().find(|n| n.node_path == path).unwrap()
    }

    #[test]
    fn the_scene_header_yields_its_type_and_uid() {
        let scene = main_scene();
        assert_eq!(scene.asset_type, "scene");
        assert_eq!(scene.uid.as_deref(), Some("uid://bqk8x2vn1a3ym"));
    }

    #[test]
    fn the_root_node_is_the_only_one_without_a_parent() {
        let scene = main_scene();
        let roots: Vec<&SceneNode> = scene.nodes.iter().filter(|n| n.parent_path.is_none()).collect();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].node_path, ROOT_PATH);
        assert_eq!(roots[0].node_type.as_deref(), Some("Node2D"));
    }

    #[test]
    fn node_paths_are_built_the_way_godot_addresses_them() {
        let scene = main_scene();
        let paths: Vec<&str> = scene.nodes.iter().map(|n| n.node_path.as_str()).collect();
        assert_eq!(paths, vec![".", "Player", "Player/Sprite", "Player/Sprite/Muzzle"]);
    }

    #[test]
    fn a_script_is_resolved_through_its_ext_resource_id_to_a_project_path() {
        let scene = main_scene();
        assert_eq!(node(&scene, "Player").script.as_deref(), Some("scripts/player.gd"));
    }

    #[test]
    fn only_the_node_carrying_the_script_property_gets_the_script() {
        let scene = main_scene();
        assert_eq!(node(&scene, ".").script, None);
        assert_eq!(node(&scene, "Player/Sprite").script, None);
    }

    #[test]
    fn a_sub_resource_section_does_not_become_a_node() {
        let scene = main_scene();
        assert_eq!(scene.nodes.len(), 4);
    }

    #[test]
    fn a_texture_ext_resource_is_not_mistaken_for_a_script() {
        let scene = main_scene();
        assert_eq!(node(&scene, "Player/Sprite").script, None);
    }

    #[test]
    fn a_tres_resource_reports_its_declared_type_and_has_no_nodes() {
        let src = "[gd_resource type=\"ShaderMaterial\" format=3 uid=\"uid://xyz\"]\n\nshader = null\n";
        let scene = parse(src).unwrap();
        assert_eq!(scene.asset_type, "shadermaterial");
        assert!(scene.nodes.is_empty());
    }

    #[test]
    fn a_file_without_a_godot_header_is_not_a_scene() {
        assert!(parse("[application]\nconfig/name=\"Snake\"\n").is_none());
        assert!(parse("just some text\n").is_none());
    }

    #[test]
    fn an_instanced_child_scene_has_no_type_but_still_lands() {
        let src = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="PackedScene" path="res://scenes/enemy.tscn" id="1_e"]

[node name="Main" type="Node2D"]

[node name="Enemy" parent="." instance=ExtResource("1_e")]
"#;
        let scene = parse(src).unwrap();
        let enemy = node(&scene, "Enemy");
        assert_eq!(enemy.node_type, None);
        assert_eq!(enemy.parent_path.as_deref(), Some("."));
    }

    #[test]
    fn a_node_named_like_an_attribute_keyword_still_parses() {
        let src = "[gd_scene format=3]\n\n[node name=\"type\" type=\"Node\"]\n";
        let scene = parse(src).unwrap();
        assert_eq!(scene.nodes[0].node_type.as_deref(), Some("Node"));
    }
}
