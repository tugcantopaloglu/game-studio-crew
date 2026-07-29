use std::path::{Path, PathBuf};

use crate::settings::on_path;

pub const CUTOUT_HELPER: &str = "remove_chroma_key.py";
pub const CUTOUT_CHECK: &str = include_str!("../helpers/cutout_check.py");
pub const GENERATED_DIR: &str = "generated_images";
pub const KEY_COLOR: &str = "#00ff00";
pub const GREEN_SUBJECT_KEY: &str = "#ff00ff";
pub const CORNER_ALPHA_CEILING: u32 = 8;
pub const MIN_SUBJECT_PERCENT: u64 = 2;
pub const MAX_SUBJECT_PERCENT: u64 = 98;
pub const IMAGE_EXTENSIONS: [&str; 2] = ["png", "webp"];

pub const IMAGE_ANSWER_SCHEMA: &str = r#"{
  "type": "object",
  "additionalProperties": false,
  "required": ["image_path", "notes"],
  "properties": {
    "image_path": { "type": "string" },
    "notes": { "type": "string" }
  }
}"#;

pub fn codex_home() -> PathBuf {
    if let Ok(named) = std::env::var("CODEX_HOME") {
        if !named.trim().is_empty() {
            return PathBuf::from(named);
        }
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    PathBuf::from(home).join(".codex")
}

pub fn cutout_script() -> PathBuf {
    codex_home()
        .join("skills")
        .join(".system")
        .join("imagegen")
        .join("scripts")
        .join(CUTOUT_HELPER)
}

pub const INTERPRETERS: [&str; 3] = ["python3", "python", "py"];

static RESOLVED: std::sync::RwLock<Option<(Vec<PathBuf>, Result<PathBuf, String>)>> =
    std::sync::RwLock::new(None);

pub fn candidates() -> Vec<PathBuf> {
    INTERPRETERS.into_iter().filter_map(on_path).collect()
}

pub fn runs_python(candidate: &Path) -> bool {
    studio_core::command(candidate)
        .args(["-c", "import sys"])
        .output()
        .map(|said| said.status.success())
        .unwrap_or(false)
}

fn interrogate(found: &[PathBuf]) -> Result<PathBuf, String> {
    if found.is_empty() {
        return Err(
            "python is not on PATH, so a generated image cannot have its background removed; \
             install python 3.10 or newer and `pip install pillow`, or ask for a texture, which \
             keeps its background"
                .to_string(),
        );
    }

    let mut not_an_interpreter = None;
    let mut without_pillow = None;
    for candidate in found {
        let said = match studio_core::command(candidate)
            .args(["-c", "import PIL"])
            .output()
        {
            Ok(said) => said,
            Err(_) => continue,
        };
        if said.status.success() {
            return Ok(candidate.clone());
        }
        if !runs_python(candidate) {
            if not_an_interpreter.is_none() {
                not_an_interpreter = Some(candidate.clone());
            }
            continue;
        }
        if without_pillow.is_none() {
            without_pillow = Some(candidate.clone());
        }
    }

    if let Some(bare) = without_pillow {
        return Err(format!(
            "the python at {} cannot import pillow, which is what removes a generated image's \
             background; run `{} -m pip install pillow`, or ask for a texture, which keeps its \
             background",
            bare.display(),
            bare.display()
        ));
    }
    if let Some(fake) = not_an_interpreter {
        return Err(format!(
            "the only python on PATH is {}, which runs no python at all; on Windows that is \
             usually the Microsoft Store shortcut. Install python 3.10 or newer, or turn the \
             alias off in Settings > Apps > Advanced app settings > App execution aliases",
            fake.display()
        ));
    }
    Err(format!(
        "none of the {} interpreters on PATH would start, so a generated image cannot have its \
         background removed",
        found.len()
    ))
}

pub fn interpreter_without_pillow() -> Option<PathBuf> {
    for candidate in candidates() {
        let Ok(said) = studio_core::command(&candidate)
            .args(["-c", "import PIL"])
            .output()
        else {
            continue;
        };
        if said.status.success() {
            return None;
        }
        if !runs_python(&candidate) {
            continue;
        }
        return Some(candidate);
    }
    None
}

pub fn python() -> Result<PathBuf, String> {
    let found = candidates();
    if let Ok(held) = RESOLVED.read() {
        if let Some((seen, verdict)) = held.as_ref() {
            if *seen == found {
                return verdict.clone();
            }
        }
    }
    let verdict = interrogate(&found);
    if let Ok(mut held) = RESOLVED.write() {
        *held = Some((found, verdict.clone()));
    }
    verdict
}

pub fn blockers() -> Vec<String> {
    let mut out = Vec::new();
    if let Err(why) = python() {
        out.push(why);
    }
    let script = cutout_script();
    if !script.is_file() {
        out.push(format!(
            "codex's imagegen skill is not installed at {}, so there is no background remover to \
             run; update codex with `{}` and open it once so it unpacks its bundled skills",
            script.display(),
            crate::health::codex_install_command()
        ));
    }
    out
}

pub fn key_for(description: &str) -> &'static str {
    let words = description.to_lowercase();
    let greenish = ["green", "emerald", "lime", "jade", "moss", "olive", "yeşil"];
    if greenish.iter().any(|w| words.contains(w)) {
        GREEN_SUBJECT_KEY
    } else {
        KEY_COLOR
    }
}

pub fn prompt_for(
    title: &str,
    name: &str,
    description: &str,
    shape: &str,
    key: Option<&str>,
    engine: &str,
) -> String {
    let background = match key {
        Some(key) => format!(
            "Create the subject on a perfectly flat solid {key} chroma-key background, because \
             the studio removes that colour afterwards to make the background transparent. The \
             background must be one uniform colour with no shadows, gradients, texture, \
             reflections, floor plane or lighting variation. Keep the subject fully separated \
             from the background with crisp edges and generous padding. Do not use {key} \
             anywhere in the subject. No cast shadow, no contact shadow, no reflection, no \
             watermark and no text unless the description asks for text."
        ),
        None => "Fill the whole frame with the surface itself. There is no subject to cut out \
                 and no background to separate: no border, no vignette, no drop shadow, no \
                 watermark and no text."
            .to_string(),
    };

    format!(
        "You are producing one game asset for a {engine} project as a raster image, using your \
         built-in image generation tool.\n\n\
         The asset is a {title} named \"{name}\".\n\n\
         What it is: {}\n\n\
         {shape}\n\n\
         {background}\n\n\
         Generate exactly one image. Do not move, copy, rename or post-process the file, do not \
         run any shell commands, and do not write anything into the working directory: the \
         studio copies the image where the engine expects it and does its own post-processing.\n\n\
         Put the absolute path of the image you generated in the image_path field and one \
         sentence about what you made in the notes field.",
        description.trim()
    )
}

pub fn parse_answer(raw: &str) -> Result<(String, String), String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(
            "codex finished without writing an answer, so there is no image to collect; run it \
             once by hand to see what it says"
                .to_string(),
        );
    }
    let parsed: serde_json::Value = serde_json::from_str(trimmed).map_err(|e| {
        format!(
            "codex answered with something that is not the json schema it was given ({e}); the \
             first 200 characters were: {}",
            trimmed.chars().take(200).collect::<String>()
        )
    })?;
    let path = parsed
        .get("image_path")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let notes = parsed
        .get("notes")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    if path.is_empty() {
        return Err(
            "codex answered without naming an image, so it most likely described one instead of \
             drawing it; check that `codex features list` still reports image_generation as true"
                .to_string(),
        );
    }
    Ok((path, notes))
}

pub fn source_in(home: &Path, given: &str) -> Result<PathBuf, String> {
    let given = given.trim();
    let ext = given
        .rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .unwrap_or_default();
    if !IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        return Err(format!(
            "codex named {given}, which is not one of the image types it generates ({}); nothing \
             was copied",
            IMAGE_EXTENSIONS.join(", ")
        ));
    }

    let made = home.join(GENERATED_DIR);
    let settled = Path::new(given)
        .canonicalize()
        .map_err(|e| format!("codex named an image at {given} that cannot be read: {e}"))?;
    let root = made.canonicalize().map_err(|e| {
        format!(
            "codex has generated no images yet at {} ({e}), so the path it named cannot be one \
             of them",
            made.display()
        )
    })?;
    if !settled.starts_with(&root) {
        return Err(format!(
            "codex named {given}, which is outside {}; only files it generated itself are copied \
             into a project",
            made.display()
        ));
    }
    if !settled.is_file() {
        return Err(format!("there is no image at {}", settled.display()));
    }
    Ok(settled)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Png {
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    pub alpha: bool,
}

pub fn inspect(path: &Path) -> Result<Png, String> {
    let raw = std::fs::read(path)
        .map_err(|e| format!("could not read the generated image at {}: {e}", path.display()))?;
    if raw.len() < 26 || raw[..8] != [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a] {
        return Err(format!(
            "{} is not a png, so the studio cannot tell what was generated",
            path.display()
        ));
    }
    if &raw[12..16] != b"IHDR" {
        return Err(format!("{} has no png header chunk", path.display()));
    }
    let width = u32::from_be_bytes([raw[16], raw[17], raw[18], raw[19]]);
    let height = u32::from_be_bytes([raw[20], raw[21], raw[22], raw[23]]);
    if width == 0 || height == 0 {
        return Err(format!("{} has no pixels", path.display()));
    }
    Ok(Png {
        width,
        height,
        bytes: raw.len() as u64,
        alpha: matches!(raw[25], 4 | 6),
    })
}

pub fn cut_out(python: &Path, input: &Path, out: &Path, key: &str) -> Result<String, String> {
    let script = cutout_script();
    if !script.is_file() {
        return Err(format!(
            "codex's background remover is missing at {}; update codex and open it once so it \
             unpacks its bundled skills",
            script.display()
        ));
    }
    if let Some(parent) = out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let said = studio_core::command(python)
        .arg(&script)
        .arg("--input")
        .arg(input)
        .arg("--out")
        .arg(out)
        .arg("--key-color")
        .arg(key)
        .args(["--auto-key", "border"])
        .arg("--soft-matte")
        .args(["--transparent-threshold", "12"])
        .args(["--opaque-threshold", "220"])
        .arg("--despill")
        .output()
        .map_err(|e| format!("could not run the background remover: {e}"))?;

    let recorded = format!(
        "{}{}",
        String::from_utf8_lossy(&said.stdout),
        String::from_utf8_lossy(&said.stderr)
    );
    if !said.status.success() {
        if recorded.contains("No module named 'PIL'") {
            return Err(format!(
                "the background remover needs pillow, which the python at {} does not have; \
                 install it with `{} -m pip install pillow` and try again",
                python.display(),
                python.display()
            ));
        }
        if !runs_python(python) {
            return Err(format!(
                "{} runs no python at all, so the background remover ran nothing; on Windows that \
                 is usually the Microsoft Store shortcut. Install python 3.10 or newer, or turn \
                 the alias off in Settings > Apps > Advanced app settings > App execution aliases",
                python.display()
            ));
        }
        return Err(format!(
            "the background could not be removed: {}",
            recorded.trim().chars().take(400).collect::<String>()
        ));
    }
    if !out.is_file() {
        return Err(format!(
            "the background remover reported success but wrote nothing to {}",
            out.display()
        ));
    }
    Ok(recorded)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cut {
    pub width: u32,
    pub height: u32,
    pub corners: u32,
    pub opaque: u64,
    pub clear: u64,
    pub total: u64,
}

impl Cut {
    pub fn subject_percent(&self) -> u64 {
        if self.total == 0 {
            return 0;
        }
        self.opaque * 100 / self.total
    }
}

pub fn parse_check_line(text: &str) -> Option<Cut> {
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("cutout ") else {
            continue;
        };
        let (size, tail) = rest.split_once(" (")?;
        let (width, height) = size.split_once('x')?;
        let inside = tail.strip_suffix(')')?;

        let mut corners = None;
        let mut opaque = None;
        let mut clear = None;
        let mut total = None;
        for part in inside.split(',') {
            let part = part.trim();
            if let Some(n) = part.strip_prefix("corners ") {
                corners = n.trim().parse::<u32>().ok();
            }
            if let Some(n) = part.strip_suffix(" opaque") {
                opaque = n.trim().parse::<u64>().ok();
            }
            if let Some(n) = part.strip_suffix(" clear") {
                clear = n.trim().parse::<u64>().ok();
            }
            if let Some(n) = part.strip_suffix(" total") {
                total = n.trim().parse::<u64>().ok();
            }
        }
        return Some(Cut {
            width: width.trim().parse().ok()?,
            height: height.trim().parse().ok()?,
            corners: corners?,
            opaque: opaque?,
            clear: clear?,
            total: total?,
        });
    }
    None
}

pub fn judge(cut: &Cut) -> Result<(), String> {
    if cut.corners > CORNER_ALPHA_CEILING {
        return Err(format!(
            "the background was not removed: the corners are still {}/255 opaque, so the key \
             colour did not match what was drawn; ask again, or ask for a texture if the asset \
             is meant to fill its frame",
            cut.corners
        ));
    }
    let share = cut.subject_percent();
    if share < MIN_SUBJECT_PERCENT {
        return Err(format!(
            "the cut-out is {share}% subject, which means the background remover ate the asset \
             as well as its background; ask again with a subject that does not share the key \
             colour"
        ));
    }
    if share > MAX_SUBJECT_PERCENT {
        return Err(format!(
            "the cut-out is {share}% subject, so nothing was actually removed; the image most \
             likely came back without the flat background it was asked for"
        ));
    }
    Ok(())
}

pub fn check(python: &Path, work: &Path, image: &Path) -> Result<Cut, String> {
    let probe = work.join("cutout_check.py");
    std::fs::write(&probe, CUTOUT_CHECK)
        .map_err(|e| format!("could not write {}: {e}", probe.display()))?;

    let said = studio_core::command(python)
        .arg(&probe)
        .arg(image)
        .output()
        .map_err(|e| format!("could not run the cut-out check: {e}"))?;
    let recorded = format!(
        "{}{}",
        String::from_utf8_lossy(&said.stdout),
        String::from_utf8_lossy(&said.stderr)
    );
    if !said.status.success() {
        return Err(format!(
            "the cut-out could not be inspected: {}",
            recorded.trim().chars().take(300).collect::<String>()
        ));
    }
    let cut = parse_check_line(&recorded).ok_or_else(|| {
        format!(
            "the cut-out check said something this build could not read: {}",
            recorded.trim().chars().take(300).collect::<String>()
        )
    })?;
    judge(&cut)?;
    Ok(cut)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_generated_image_is_only_collected_from_the_folder_codex_writes_to() {
        let home = std::env::temp_dir().join("studio-imagegen-home");
        let _ = std::fs::remove_dir_all(&home);
        let made = home.join(GENERATED_DIR).join("session");
        std::fs::create_dir_all(&made).unwrap();
        let mine = made.join("asset.png");
        std::fs::write(&mine, b"pretend png").unwrap();

        let secret = std::env::temp_dir().join("studio-imagegen-secret.png");
        std::fs::write(&secret, b"not yours").unwrap();

        assert!(source_in(&home, &mine.to_string_lossy()).is_ok());

        let err = source_in(&home, &secret.to_string_lossy()).unwrap_err();
        assert!(err.contains("outside"), "{err}");

        let wrong_kind = made.join("asset.txt");
        std::fs::write(&wrong_kind, b"nope").unwrap();
        assert!(source_in(&home, &wrong_kind.to_string_lossy())
            .unwrap_err()
            .contains("not one of the image types"));
    }

    #[test]
    fn an_answer_without_a_path_says_codex_described_an_image_instead_of_drawing_one() {
        let err = parse_answer(r#"{"image_path":"","notes":"a lantern"}"#).unwrap_err();
        assert!(err.contains("described one instead of drawing it"));
        assert!(err.contains("image_generation"));

        assert!(parse_answer("").unwrap_err().contains("without writing an answer"));
        assert!(parse_answer("I drew you a lantern")
            .unwrap_err()
            .contains("not the json schema"));

        let (path, notes) =
            parse_answer(r#"{"image_path":"C:\\x\\a.png","notes":"a lantern"}"#).unwrap();
        assert_eq!(path, "C:\\x\\a.png");
        assert_eq!(notes, "a lantern");
    }

    #[test]
    fn a_png_header_is_read_for_the_size_and_whether_it_carries_alpha() {
        let dir = std::env::temp_dir().join("studio-imagegen-png");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut raw = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        raw.extend_from_slice(&13u32.to_be_bytes());
        raw.extend_from_slice(b"IHDR");
        raw.extend_from_slice(&64u32.to_be_bytes());
        raw.extend_from_slice(&32u32.to_be_bytes());
        raw.push(8);
        raw.push(6);
        raw.extend_from_slice(&[0, 0, 0]);
        let path = dir.join("rgba.png");
        std::fs::write(&path, &raw).unwrap();

        let png = inspect(&path).unwrap();
        assert_eq!(png.width, 64);
        assert_eq!(png.height, 32);
        assert!(png.alpha, "colour type 6 is rgba");

        raw[25] = 2;
        let opaque = dir.join("rgb.png");
        std::fs::write(&opaque, &raw).unwrap();
        assert!(!inspect(&opaque).unwrap().alpha);

        let lie = dir.join("lie.png");
        std::fs::write(&lie, b"this is not a png at all really").unwrap();
        assert!(inspect(&lie).unwrap_err().contains("not a png"));
    }

    #[test]
    fn the_cut_out_check_line_is_read_for_the_corners_and_the_subject_share() {
        let said = "cutout 1024x1024 (corners 0, 262144 opaque, 780000 clear, 1048576 total)";
        let cut = parse_check_line(said).unwrap();
        assert_eq!(cut.width, 1024);
        assert_eq!(cut.corners, 0);
        assert_eq!(cut.opaque, 262144);
        assert_eq!(cut.subject_percent(), 25);
        assert!(judge(&cut).is_ok());

        assert_eq!(parse_check_line("nothing useful"), None);
    }

    #[test]
    fn an_image_that_kept_its_background_is_refused_rather_than_shipped_as_a_sprite() {
        let opaque_corners = Cut {
            width: 100,
            height: 100,
            corners: 255,
            opaque: 5000,
            clear: 0,
            total: 10000,
        };
        let err = judge(&opaque_corners).unwrap_err();
        assert!(err.contains("background was not removed"));
        assert!(err.contains("255"));

        let everything = Cut {
            corners: 0,
            opaque: 9999,
            ..opaque_corners
        };
        assert!(judge(&everything).unwrap_err().contains("nothing was actually removed"));

        let nothing = Cut {
            corners: 0,
            opaque: 10,
            ..opaque_corners
        };
        assert!(judge(&nothing).unwrap_err().contains("ate the asset"));
    }

    #[test]
    fn a_green_subject_gets_a_key_colour_it_cannot_be_confused_with() {
        assert_eq!(key_for("a rusty iron lantern"), KEY_COLOR);
        assert_eq!(key_for("a green slime monster"), GREEN_SUBJECT_KEY);
        assert_eq!(key_for("an EMERALD gemstone"), GREEN_SUBJECT_KEY);
    }

    #[test]
    fn the_prompt_asks_for_a_key_colour_only_when_the_studio_will_remove_it() {
        let cut = prompt_for(
            "sprite",
            "Lantern",
            "a rusty iron lantern",
            "A sprite reads at small sizes.",
            Some(KEY_COLOR),
            "web",
        );
        assert!(cut.contains("chroma-key background"));
        assert!(cut.contains(KEY_COLOR));
        assert!(cut.contains("built-in image generation tool"));
        assert!(cut.contains("image_path"));

        let tiling = prompt_for("texture", "Bark", "rough bark", "It tiles.", None, "godot");
        assert!(!tiling.contains("chroma-key"));
        assert!(tiling.contains("Fill the whole frame"));
    }

    #[test]
    fn the_background_remover_is_looked_for_inside_the_codex_home_that_is_actually_set() {
        let script = cutout_script();
        assert!(script.ends_with(Path::new("scripts").join(CUTOUT_HELPER)));
        assert!(script.to_string_lossy().contains("imagegen"));
    }

    #[test]
    fn a_non_interpreter_is_told_apart_by_running_it_rather_than_by_reading_its_complaint() {
        assert!(
            !runs_python(Path::new("studio-no-such-python-anywhere")),
            "a name that starts nothing is not an interpreter"
        );

        let Some(real) = candidates().into_iter().find(|c| runs_python(c)) else {
            return;
        };
        assert!(
            studio_core::command(&real)
                .args(["-c", "import sys"])
                .output()
                .map(|s| s.status.success())
                .unwrap_or(false),
            "{} was accepted as an interpreter but cannot run the simplest program there is",
            real.display()
        );
    }

    #[test]
    fn the_interpreter_the_pillow_remedy_names_is_always_one_that_actually_runs_python() {
        let Some(named) = interpreter_without_pillow() else {
            return;
        };
        assert!(
            runs_python(&named),
            "the remedy points `pip install pillow` at {}, which runs no python; the Store \
             shortcut sits on PATH as python3 and announces itself in the machine's own language, \
             so a check that reads its English wording lets it through on every localised Windows",
            named.display()
        );
    }

    #[test]
    fn a_python_that_cannot_import_pillow_is_named_along_with_the_command_that_fixes_it() {
        let interpreter = if cfg!(windows) { "python" } else { "python3" };
        let Some(real) = on_path(interpreter) else {
            return;
        };
        let answered = interrogate(&[real.clone()]);
        match answered {
            Ok(found) => assert_eq!(found, real, "the interpreter that imports PIL is the one used"),
            Err(why) => assert!(
                why.contains("pip install pillow") || why.contains("Microsoft Store"),
                "a refusal has to name what to install: {why}"
            ),
        }
    }

    #[test]
    fn nothing_on_path_is_reported_as_something_to_install_rather_than_as_a_crash() {
        let why = interrogate(&[]).unwrap_err();
        assert!(why.contains("not on PATH"));
        assert!(why.contains("pillow"));
        assert!(
            why.contains("texture"),
            "a blocked sprite still has a way forward, and saying so is the point: {why}"
        );
    }
}
