use serde::{Deserialize, Serialize};
use serde_json::json;

pub const CHARS_PER_TOKEN: f64 = 3.6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Model {
    Fable,
    Opus,
    Haiku,
}

impl Model {
    pub fn cli_alias(&self) -> &'static str {
        match self {
            Model::Fable => "fable",
            Model::Opus => "opus",
            Model::Haiku => "haiku",
        }
    }

    pub fn min_cacheable_tokens(&self) -> usize {
        match self {
            Model::Fable => 2048,
            Model::Opus | Model::Haiku => 4096,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum FreezeError {
    #[error("layer {layer} contains an unsubstituted template marker at byte {offset}")]
    TemplateMarker { layer: &'static str, offset: usize },

    #[error("layer {layer} is empty; a frozen prefix must carry all three layers")]
    EmptyLayer { layer: &'static str },

    #[error("tool allowlist contains a duplicate entry: {tool}")]
    DuplicateTool { tool: String },

    #[error("tool allowlist contains an empty entry")]
    EmptyTool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CharterSource {
    pub studio_conventions: String,
    pub engine_profile: String,
    pub role_charter: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FrozenPrefix {
    pub bytes: String,
    pub prefix_hash: String,
    pub tools: Vec<String>,
    pub model: Model,
    pub estimated_tokens: usize,
    pub padded_tokens: usize,
}

impl FrozenPrefix {
    pub fn prompt_frozen_data(&self, role: &str) -> serde_json::Value {
        json!({
            "role": role,
            "prefix_hash": self.prefix_hash,
            "layers": ["L0", "L1", "L2"],
            "bytes": self.bytes.len(),
            "tools": self.tools,
            "model": self.model,
            "estimated_tokens": self.estimated_tokens,
            "padded_tokens": self.padded_tokens,
        })
    }
}

pub fn estimate_tokens(s: &str) -> usize {
    (s.chars().count() as f64 / CHARS_PER_TOKEN).ceil() as usize
}

fn normalize(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

fn reject_template_markers(layer: &'static str, s: &str) -> Result<(), FreezeError> {
    match s.find("{{") {
        Some(offset) => Err(FreezeError::TemplateMarker { layer, offset }),
        None => Ok(()),
    }
}

fn check_layer(layer: &'static str, s: &str) -> Result<String, FreezeError> {
    let normalized = normalize(s);
    if normalized.trim().is_empty() {
        return Err(FreezeError::EmptyLayer { layer });
    }
    reject_template_markers(layer, &normalized)?;
    Ok(normalized)
}

fn normalize_tools(tools: &[String]) -> Result<Vec<String>, FreezeError> {
    let mut out = Vec::with_capacity(tools.len());
    for t in tools {
        let t = t.trim();
        if t.is_empty() {
            return Err(FreezeError::EmptyTool);
        }
        if out.iter().any(|existing: &String| existing == t) {
            return Err(FreezeError::DuplicateTool { tool: t.to_string() });
        }
        out.push(t.to_string());
    }
    out.sort();
    Ok(out)
}

const PADDING_PREAMBLE: &str =
    "\nThe following conventions are restated to hold this charter above the model's\nminimum cacheable prefix. They are load-bearing studio rules, not filler.\n\n";

const PADDING_CONVENTIONS: [&str; 10] = [
    "Capsules are the only inter-agent channel. Emit exactly one; never address another worker directly.",
    "Artifacts are passed by reference. A capsule carries paths and symbol names, never file bodies.",
    "Dead ends belong in do_not_revisit so the next worker does not re-derive the same failure.",
    "Escalation goes to the declared parent role. Never escalate laterally.",
    "Verification belongs to the daemon. Never parse raw engine logs.",
    "The frozen prefix carries no per-run identifiers. Everything volatile belongs to the task brief.",
    "Budget is enforced by the daemon. On a budget warning, summarize and return.",
    "Decisions that bind other roles are promoted to ADRs and cited by identifier thereafter.",
    "Symbol slices are pulled on demand. Request a full body only when the signature is insufficient.",
    "Engine specialization arrives as a prompt layer. A role charter never names an engine.",
];

fn pad_to_minimum(body: &str, target_tokens: usize) -> (String, usize) {
    if estimate_tokens(body) >= target_tokens {
        return (body.to_string(), 0);
    }

    let start = estimate_tokens(body);
    let mut out = String::from(body);
    out.push_str(PADDING_PREAMBLE);

    let mut i = 0usize;
    while estimate_tokens(&out) < target_tokens {
        let line = PADDING_CONVENTIONS[i % PADDING_CONVENTIONS.len()];
        out.push_str(&format!("{:04}. {}\n", i + 1, line));
        i += 1;
    }

    let added = estimate_tokens(&out) - start;
    (out, added)
}

pub fn freeze(
    source: &CharterSource,
    tools: &[String],
    model: Model,
) -> Result<FrozenPrefix, FreezeError> {
    let l0 = check_layer("L0", &source.studio_conventions)?;
    let l1 = check_layer("L1", &source.engine_profile)?;
    let l2 = check_layer("L2", &source.role_charter)?;
    let tools = normalize_tools(tools)?;

    let joined = format!(
        "{}\n{}\n{}\n",
        l0.trim_end(),
        l1.trim_end(),
        l2.trim_end()
    );

    let (bytes, padded_tokens) = pad_to_minimum(&joined, model.min_cacheable_tokens());

    let mut hasher = blake3::Hasher::new();
    hasher.update(bytes.as_bytes());
    hasher.update(b"\x00tools\x00");
    for t in &tools {
        hasher.update(t.as_bytes());
        hasher.update(b"\x00");
    }
    hasher.update(b"\x00model\x00");
    hasher.update(model.cli_alias().as_bytes());

    let estimated_tokens = estimate_tokens(&bytes);

    Ok(FrozenPrefix {
        bytes,
        prefix_hash: hasher.finalize().to_hex().to_string(),
        tools,
        model,
        estimated_tokens,
        padded_tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src() -> CharterSource {
        CharterSource {
            studio_conventions: "L0 studio conventions.".into(),
            engine_profile: "L1 engine idioms.".into(),
            role_charter: "L2 role mandate.".into(),
        }
    }

    fn tools() -> Vec<String> {
        vec!["Read".into(), "Grep".into(), "Glob".into()]
    }

    #[test]
    fn freezing_is_deterministic() {
        let a = freeze(&src(), &tools(), Model::Opus).unwrap();
        let b = freeze(&src(), &tools(), Model::Opus).unwrap();
        assert_eq!(a.prefix_hash, b.prefix_hash);
        assert_eq!(a.bytes, b.bytes);
    }

    #[test]
    fn line_endings_are_normalized_so_a_checkout_cannot_break_the_cache() {
        let unix = src();
        let windows = CharterSource {
            studio_conventions: "L0 studio conventions.\r\n".into(),
            engine_profile: "L1 engine idioms.\r\n".into(),
            role_charter: "L2 role mandate.\r\n".into(),
        };
        let a = freeze(&unix, &tools(), Model::Opus).unwrap();
        let b = freeze(&windows, &tools(), Model::Opus).unwrap();
        assert_eq!(a.prefix_hash, b.prefix_hash);
        assert!(!a.bytes.contains('\r'));
    }

    #[test]
    fn tool_order_does_not_change_the_hash() {
        let a = freeze(&src(), &["Read".into(), "Grep".into()], Model::Opus).unwrap();
        let b = freeze(&src(), &["Grep".into(), "Read".into()], Model::Opus).unwrap();
        assert_eq!(a.prefix_hash, b.prefix_hash);
        assert_eq!(a.tools, vec!["Grep".to_string(), "Read".to_string()]);
    }

    #[test]
    fn a_different_allowlist_mints_a_different_prefix() {
        let a = freeze(&src(), &["Read".into(), "Grep".into()], Model::Opus).unwrap();
        let b = freeze(&src(), &["Read".into(), "Grep".into(), "Edit".into()], Model::Opus).unwrap();
        assert_ne!(
            a.prefix_hash, b.prefix_hash,
            "the tool allowlist is part of the cache key; adding a tool must cold-start the cache"
        );
        assert_eq!(a.bytes, b.bytes, "the charter text itself is unchanged");
    }

    #[test]
    fn a_different_model_mints_a_different_prefix() {
        let a = freeze(&src(), &tools(), Model::Opus).unwrap();
        let b = freeze(&src(), &tools(), Model::Fable).unwrap();
        assert_ne!(a.prefix_hash, b.prefix_hash);
    }

    #[test]
    fn unsubstituted_template_markers_fail_loudly() {
        let mut s = src();
        s.role_charter = "L2 mandate for {{role_name}}.".into();
        let err = freeze(&s, &tools(), Model::Opus).unwrap_err();
        assert!(matches!(err, FreezeError::TemplateMarker { layer: "L2", .. }));
    }

    #[test]
    fn empty_layers_are_rejected() {
        let mut s = src();
        s.engine_profile = "   \n".into();
        assert_eq!(
            freeze(&s, &tools(), Model::Opus).unwrap_err(),
            FreezeError::EmptyLayer { layer: "L1" }
        );
    }

    #[test]
    fn duplicate_and_empty_tools_are_rejected() {
        assert_eq!(
            freeze(&src(), &["Read".into(), "Read".into()], Model::Opus).unwrap_err(),
            FreezeError::DuplicateTool { tool: "Read".into() }
        );
        assert_eq!(
            freeze(&src(), &["Read".into(), "  ".into()], Model::Opus).unwrap_err(),
            FreezeError::EmptyTool
        );
    }

    #[test]
    fn short_charters_are_padded_past_the_model_minimum() {
        let opus = freeze(&src(), &tools(), Model::Opus).unwrap();
        assert!(opus.estimated_tokens >= Model::Opus.min_cacheable_tokens());
        assert!(opus.padded_tokens > 0);

        let fable = freeze(&src(), &tools(), Model::Fable).unwrap();
        assert!(fable.estimated_tokens >= Model::Fable.min_cacheable_tokens());
        assert!(
            fable.estimated_tokens < opus.estimated_tokens,
            "fable's lower minimum should need less padding"
        );
    }

    #[test]
    fn padding_is_never_whitespace_filler() {
        let p = freeze(&src(), &tools(), Model::Opus).unwrap();
        let tail = &p.bytes[p.bytes.len() / 2..];
        assert!(tail.contains("Capsules are the only inter-agent channel"));
        assert!(!tail.trim().is_empty());
    }

    #[test]
    fn padding_is_stable_across_runs() {
        let a = freeze(&src(), &tools(), Model::Opus).unwrap();
        let b = freeze(&src(), &tools(), Model::Opus).unwrap();
        assert_eq!(a.bytes, b.bytes);
        assert_eq!(a.padded_tokens, b.padded_tokens);
    }

    #[test]
    fn an_already_long_charter_is_not_padded() {
        let long = "All studio conventions restated at length. ".repeat(2000);
        let s = CharterSource {
            studio_conventions: long,
            engine_profile: "L1.".into(),
            role_charter: "L2.".into(),
        };
        let p = freeze(&s, &tools(), Model::Opus).unwrap();
        assert_eq!(p.padded_tokens, 0);
    }

    #[test]
    fn prompt_frozen_data_carries_the_hash_and_the_allowlist() {
        let p = freeze(&src(), &tools(), Model::Opus).unwrap();
        let d = p.prompt_frozen_data("gameplay_engineer");
        assert_eq!(d["role"], "gameplay_engineer");
        assert_eq!(d["prefix_hash"], p.prefix_hash);
        assert_eq!(d["tools"][0], "Glob");
    }
}
