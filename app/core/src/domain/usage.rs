//! What the app spends: AI tokens and Glean calls.

use crate::error::Result;
use crate::time::{Date, Ts};

/// AI calls for one tier and model since a time, split at the start of today.
#[derive(Debug, Clone)]
pub struct AiUsage {
    pub tier: String,
    pub model: String,
    pub today: bool,
    pub calls: i64,
    pub input: i64,
    pub output: i64,
}

pub trait UsageViews {
    /// Calls and tokens per model in `[from, to)`.
    fn ai_by_model(&self, from: Ts, to: Ts) -> Result<Vec<(String, i64, i64)>>;
    /// Calls since `since`, grouped by tier, model and whether they were after `today_start`.
    fn ai_since(&self, today_start: Ts, since: Ts) -> Result<Vec<AiUsage>>;
    /// Glean calls made on `day`.
    fn glean_calls(&self, day: Date) -> Result<i64>;
}

pub trait Usage {
    fn record_ai(&self, tier: &str, model: &str, input_tokens: i64, output_tokens: i64) -> Result<()>;
    /// Count one Glean call on `day` unless `per_day` were made already. False when over.
    fn take_glean_call(&self, day: Date, per_day: i64) -> Result<bool>;
}
