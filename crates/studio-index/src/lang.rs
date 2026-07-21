#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    GdScript,
    CSharp,
    Cpp,
}

impl Lang {
    pub fn wire(self) -> &'static str {
        match self {
            Lang::GdScript => "gdscript",
            Lang::CSharp => "csharp",
            Lang::Cpp => "cpp",
        }
    }

    pub fn from_path(path: &str) -> Option<Lang> {
        match extension(path)?.as_str() {
            "gd" => Some(Lang::GdScript),
            "cs" => Some(Lang::CSharp),
            "cpp" | "cc" | "cxx" | "h" | "hpp" | "inl" => Some(Lang::Cpp),
            _ => None,
        }
    }

    pub fn has_extractor(self) -> bool {
        matches!(self, Lang::GdScript | Lang::CSharp)
    }
}

const BINARY_EXTENSIONS: &[&str] = &[
    "uasset", "umap", "ubulk", "uexp", "png", "jpg", "jpeg", "gif", "bmp", "tga", "psd", "exr",
    "fbx", "obj", "blend", "glb", "gltf", "wav", "ogg", "mp3", "flac", "ttf", "otf", "dll", "so",
    "dylib", "exe", "pdb", "lib", "a", "zip", "pck", "ctex", "stex", "mesh", "res", "assets",
    "unitypackage", "aab", "apk",
];

pub fn is_godot_asset_path(path: &str) -> bool {
    matches!(extension(path).as_deref(), Some("tscn" | "tres" | "escn"))
}

pub fn is_binary_path(path: &str) -> bool {
    extension(path).is_some_and(|e| BINARY_EXTENSIONS.contains(&e.as_str()))
}

fn extension(path: &str) -> Option<String> {
    let name = path.rsplit(['/', '\\']).next()?;
    let dot = name.rfind('.')?;
    if dot == 0 {
        return None;
    }
    Some(name[dot + 1..].to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_the_three_engine_languages() {
        assert_eq!(Lang::from_path("src/Player.gd"), Some(Lang::GdScript));
        assert_eq!(Lang::from_path("Assets/Mover.cs"), Some(Lang::CSharp));
        assert_eq!(Lang::from_path("Source/Game/Pawn.cpp"), Some(Lang::Cpp));
        assert_eq!(Lang::from_path("Source/Game/Pawn.h"), Some(Lang::Cpp));
        assert_eq!(Lang::from_path("README.md"), None);
    }

    #[test]
    fn cpp_is_a_language_without_an_extractor_yet() {
        assert!(Lang::GdScript.has_extractor());
        assert!(Lang::CSharp.has_extractor());
        assert!(!Lang::Cpp.has_extractor());
    }

    #[test]
    fn ue_and_unity_binaries_are_flagged_but_text_scenes_are_not() {
        assert!(is_binary_path("Content/Maps/Main.umap"));
        assert!(is_binary_path("Content/Mesh.uasset"));
        assert!(is_binary_path("Art/hero.PNG"));
        assert!(!is_binary_path("scenes/main.tscn"));
        assert!(!is_binary_path("scenes/theme.tres"));
        assert!(!is_binary_path("project.godot"));
    }

    #[test]
    fn godot_text_assets_are_recognised_but_scripts_and_config_are_not() {
        assert!(is_godot_asset_path("scenes/main.tscn"));
        assert!(is_godot_asset_path("themes/dark.tres"));
        assert!(!is_godot_asset_path("scripts/player.gd"));
        assert!(!is_godot_asset_path("project.godot"));
    }

    #[test]
    fn a_dotfile_has_no_extension() {
        assert!(!is_binary_path(".gitignore"));
        assert_eq!(Lang::from_path(".gitignore"), None);
    }
}
