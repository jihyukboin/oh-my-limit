use super::app::RateLimitUsage;
use serde_json::Value;

pub(super) fn rate_limit_summary(params: &Value) -> Option<String> {
    rate_limit_usage(params)?;
    Some("Ready.".to_owned())
}

pub(super) fn rate_limit_usage(params: &Value) -> Option<RateLimitUsage> {
    let rate_limits = params.get("rateLimits").unwrap_or(params);
    let mut usage = RateLimitUsage::default();

    assign_rate_limit_window(
        &mut usage,
        rate_limits.get("primary"),
        RateLimitFallback::FiveHour,
    );
    assign_rate_limit_window(
        &mut usage,
        rate_limits.get("secondary"),
        RateLimitFallback::Weekly,
    );

    if usage.five_hour_percent.is_some() || usage.weekly_percent.is_some() {
        Some(usage)
    } else {
        None
    }
}

enum RateLimitFallback {
    FiveHour,
    Weekly,
}

fn assign_rate_limit_window(
    usage: &mut RateLimitUsage,
    window: Option<&Value>,
    fallback: RateLimitFallback,
) {
    let Some(window) = window else {
        return;
    };
    let Some(percent) = limit_window_percent(window) else {
        return;
    };

    match window.get("windowDurationMins").and_then(Value::as_u64) {
        Some(minutes) if minutes <= 5 * 60 => usage.five_hour_percent = Some(percent),
        Some(minutes) if minutes >= 7 * 24 * 60 => usage.weekly_percent = Some(percent),
        _ => match fallback {
            RateLimitFallback::FiveHour => usage.five_hour_percent = Some(percent),
            RateLimitFallback::Weekly => usage.weekly_percent = Some(percent),
        },
    }
}

fn limit_window_percent(window: &Value) -> Option<u16> {
    let percent = window.get("usedPercent").and_then(Value::as_u64)?;
    Some(percent.min(100) as u16)
}
