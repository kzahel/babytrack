//! Day totals over the portable current-record read model.

use crate::read_model::Activity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayWindow {
    pub start_utc_ms: i64,
    pub end_utc_ms: i64,
    pub through_utc_ms: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DaySummary {
    pub sleep_ms: u64,
    pub feed_count: u64,
    pub bottle_ml: u64,
    pub diaper_count: u64,
    pub wet_diaper_count: u64,
    pub dirty_diaper_count: u64,
}

/// Count current child records in the viewer's local-day UTC window. The
/// caller supplies actual local midnight bounds, including DST-short/long
/// days; a running sleep contributes only through the observed instant.
pub fn summarize_day(
    activities: impl IntoIterator<Item = Activity>,
    child_id: [u8; 16],
    window: DayWindow,
) -> Result<DaySummary, &'static str> {
    if window.start_utc_ms < 0
        || window.end_utc_ms <= window.start_utc_ms
        || window.through_utc_ms < 0
    {
        return Err("invalid local-day window");
    }
    let mut summary = DaySummary::default();
    for activity in activities {
        if activity.child_id != child_id {
            continue;
        }
        if activity.kind == "sleep" {
            let from = activity.start_utc_ms.max(window.start_utc_ms);
            let to = activity
                .end_utc_ms
                .unwrap_or(window.through_utc_ms)
                .min(window.end_utc_ms)
                .min(window.through_utc_ms);
            if to > from {
                let duration = u64::try_from(i128::from(to) - i128::from(from))
                    .expect("positive i64 instant difference fits u64");
                summary.sleep_ms = summary.sleep_ms.saturating_add(duration);
            }
        }
        if activity.start_utc_ms < window.start_utc_ms
            || activity.start_utc_ms >= window.end_utc_ms
            || activity.start_utc_ms > window.through_utc_ms
        {
            continue;
        }
        if matches!(
            activity.kind.as_str(),
            "feed.breast" | "feed.bottle" | "feed.solids"
        ) {
            summary.feed_count = summary.feed_count.saturating_add(1);
        }
        if activity.kind == "feed.bottle" {
            summary.bottle_ml = summary.bottle_ml.saturating_add(
                activity
                    .bottle_ml
                    .and_then(|ml| u64::try_from(ml).ok())
                    .unwrap_or(0),
            );
        }
        if activity.kind == "diaper" {
            summary.diaper_count = summary.diaper_count.saturating_add(1);
            if matches!(activity.diaper_kind, Some(1 | 3)) {
                summary.wet_diaper_count = summary.wet_diaper_count.saturating_add(1);
            }
            if matches!(activity.diaper_kind, Some(2 | 3)) {
                summary.dirty_diaper_count = summary.dirty_diaper_count.saturating_add(1);
            }
        }
    }
    Ok(summary)
}
