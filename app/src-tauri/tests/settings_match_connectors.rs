//! The typed source configs in `alethiometer-settings` and the connectors' own config parsing agree on defaults.
//! Connectors read their config from the JSON themselves, so nothing else keeps these in step.

use alethiometer_settings::sources::{GcalConfig, SlackConfig};
use serde_json::json;

#[test]
fn defaults_match_connectors() {
    let slack = alethiometer_lib::sync::connectors::slack::Config::from_json(&json!({}));
    let d = SlackConfig::default();
    assert_eq!(
        (
            slack.lookback_hours,
            slack.max_dms as i64,
            slack.thread_days,
            slack.max_threads as i64,
            slack.catchup_hours,
            slack.dms,
            slack.mentions
        ),
        (
            d.lookback_hours,
            d.max_dms,
            d.thread_days,
            d.max_threads,
            d.catchup_hours,
            d.dms,
            d.mentions
        )
    );
    assert_eq!(
        (slack.user_token_secret, slack.app_token_secret),
        (d.user_token_secret, d.app_token_secret)
    );
    let g = alethiometer_lib::sync::connectors::gcal::Config::from(&json!({}));
    let d = GcalConfig::default();
    assert_eq!(
        (
            g.calendar_id,
            g.days_back,
            g.days_ahead,
            g.hold_minutes,
            g.hold_day_start,
            g.hold_day_end,
            g.hold_search_days as i64,
            g.default_category
        ),
        (
            d.calendar_id,
            d.days_back,
            d.days_ahead,
            d.hold_minutes,
            d.hold_hours[0],
            d.hold_hours[1],
            d.hold_search_days,
            d.default_category
        )
    );
}
