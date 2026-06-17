//! Planning prompt catalogue.
//!
//! The prompt *rendering engine* and the shared [`SharedRegistry`] live in
//! `taskagent-ai-infra`. This module only declares the catalogue of
//! planning-operation prompts — one `prompts/*.toml` per operation
//! (decompose, scope) — because those prompts are operational, not
//! infrastructure.
//!
//! All known prompts are baked into the binary via `include_str!`; the
//! first [`PromptRegistry::load`] call parses them.
//!
//! ```ignore
//! use serde::Serialize;
//! use planning_oss::prompts::PromptRegistry;
//!
//! #[derive(Serialize)]
//! struct ScopeCtx<'a> { title: &'a str, description: &'a str }
//!
//! let s = PromptRegistry::load("scope", "up", &ScopeCtx { title: "t", description: "d" })?;
//! ```

use once_cell::sync::Lazy;
use serde::Serialize;
use taskagent_ai_infra::prompts::PromptRegistry as SharedRegistry;
use taskagent_shared::CoreError;

static PROMPTS: Lazy<SharedRegistry> = Lazy::new(|| {
    SharedRegistry::new(&[
        ("decompose", include_str!("../prompts/decompose.toml")),
        ("scope", include_str!("../prompts/scope.toml")),
    ])
});

/// Process-wide catalogue of planning prompts. All sources are baked into
/// the binary via `include_str!`; the first `load` call parses them.
pub struct PromptRegistry;

impl PromptRegistry {
    /// Render `name` / `variant` against `params`. See
    /// [`SharedRegistry::load`] for error semantics.
    pub fn load<P: Serialize>(name: &str, variant: &str, params: &P) -> Result<String, CoreError> {
        PROMPTS.load(name, variant, params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_prompt_loads() {
        for (name, _file) in PROMPTS.iter() {
            assert!(!name.is_empty());
        }
        assert!(!PROMPTS.is_empty(), "no prompts loaded");
    }

    #[test]
    fn decompose_with_hint_includes_guidance_block() {
        #[derive(Serialize)]
        struct Ctx<'a> {
            task_context: &'a str,
            hint: &'a str,
        }
        let s = PromptRegistry::load(
            "decompose",
            "with_hint",
            &Ctx {
                task_context: "Build login page",
                hint: "OAuth first",
            },
        )
        .unwrap();
        assert!(s.contains("Build login page"));
        assert!(s.contains("Additional guidance:"));
        assert!(s.contains("OAuth first"));
    }
}
