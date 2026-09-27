//! Work AI gate. Boundary rule 2: work data may only go to providers on the
//! allow-list in settings (BAA-covered or local). Anything else fails closed;
//! there is no fallback provider.
//!
//! The Bedrock client itself is not wired yet (UI-shell phase). `ask` returns a
//! clear error so the UI can show it, and never silently answers.

use crate::error::{msg, Result};
use crate::settings::{AiSettings, AiTier};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Small,
    Large,
}

/// Validate that `tier` may receive work data. Returns the tier config on success.
pub fn check<'a>(ai: &'a AiSettings, tier: Tier) -> Result<&'a AiTier> {
    let t = match tier {
        Tier::Small => &ai.small,
        Tier::Large => &ai.large,
    };
    if !ai.allowed_providers.iter().any(|p| p == &t.provider) {
        return Err(msg(format!(
            "AI provider '{}' is not on the allow-list; refusing to send work data",
            t.provider
        )));
    }
    if t.model.is_empty() {
        return Err(msg(format!("No model set for the {:?} tier in settings.json", tier)));
    }
    Ok(t)
}

/// Short label for the assistant pane header, e.g. "Bedrock · <model>".
pub fn label(ai: &AiSettings) -> String {
    match check(ai, Tier::Large) {
        Ok(t) => format!("{} · {}", t.provider, t.model),
        Err(_) => format!("{} · not configured", ai.large.provider),
    }
}

pub fn ask(ai: &AiSettings, _question: &str) -> Result<String> {
    let t = check(ai, Tier::Large)?;
    match t.provider.as_str() {
        "bedrock" => Err(msg("Bedrock client not built yet (Phase 1)")),
        other => Err(msg(format!("No client for provider '{other}'"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    #[test]
    fn provider_off_allow_list_fails_closed() {
        let mut ai = Settings::default().ai;
        ai.large = AiTier { provider: "openai".into(), model: "x".into() };
        let err = check(&ai, Tier::Large).unwrap_err().to_string();
        assert!(err.contains("not on the allow-list"), "{err}");
    }

    #[test]
    fn missing_model_fails() {
        let ai = Settings::default().ai;
        assert!(check(&ai, Tier::Small).is_err());
    }

    #[test]
    fn allowed_provider_with_model_passes() {
        let mut ai = Settings::default().ai;
        ai.small.model = "some-model".into();
        assert!(check(&ai, Tier::Small).is_ok());
    }
}
