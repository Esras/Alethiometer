//! AI token and Glean call usage.

use super::Sql;
use crate::domain::usage::{AiUsage, Usage, UsageViews};
use crate::error::Result;
use crate::time::{Date, Ts};
use rusqlite::{params, OptionalExtension};

impl UsageViews for Sql<'_> {
    fn ai_by_model(&self, from: Ts, to: Ts) -> Result<Vec<(String, i64, i64)>> {
        Ok(self
            .conn()
            .prepare_cached("SELECT model, count(*), sum(input_tokens) + sum(output_tokens) FROM ai_usage WHERE at >= ?1 AND at < ?2 GROUP BY model")?
            .query_map(params![from, to], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn ai_since(&self, today_start: Ts, since: Ts) -> Result<Vec<AiUsage>> {
        Ok(self
            .conn()
            .prepare_cached(
                "SELECT tier, model, at >= ?1, count(*), sum(input_tokens), sum(output_tokens)
                   FROM ai_usage WHERE at >= ?2 GROUP BY 1, 2, 3 ORDER BY tier DESC",
            )?
            .query_map(params![today_start, since], |r| {
                Ok(AiUsage {
                    tier: r.get(0)?,
                    model: r.get(1)?,
                    today: r.get(2)?,
                    calls: r.get(3)?,
                    input: r.get(4)?,
                    output: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn glean_calls(&self, day: Date) -> Result<i64> {
        Ok(self
            .conn()
            .query_row("SELECT n FROM glean_calls WHERE day = ?1", [day], |r| r.get(0))
            .optional()?
            .unwrap_or(0))
    }
}

impl Usage for Sql<'_> {
    fn record_ai(&self, tier: &str, model: &str, input_tokens: i64, output_tokens: i64) -> Result<()> {
        self.conn().execute(
            "INSERT INTO ai_usage (tier, model, input_tokens, output_tokens) VALUES (?1, ?2, ?3, ?4)",
            params![tier, model, input_tokens, output_tokens],
        )?;
        Ok(())
    }

    fn take_glean_call(&self, day: Date, per_day: i64) -> Result<bool> {
        if self.glean_calls(day)? >= per_day {
            return Ok(false);
        }
        self.conn().execute(
            "INSERT INTO glean_calls (day, n) VALUES (?1, 1) ON CONFLICT (day) DO UPDATE SET n = n + 1",
            [day],
        )?;
        Ok(true)
    }
}
