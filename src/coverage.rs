//! Observed event coverage only. An elapsed quote gap is not proof of missing data.
use crate::{
    Result,
    data::{self, Dataset, Tick},
    validation::Interval,
};
use chrono::{Datelike, TimeZone, Utc};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Event {
    pub timestamp_ms: i64,
    /// Decimal string preserves the full source sequence in JSON consumers.
    pub sequence: String,
}
impl From<&Tick> for Event {
    fn from(t: &Tick) -> Self {
        Self {
            timestamp_ms: t.timestamp_ms,
            sequence: t.sequence.to_string(),
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Month {
    pub utc_month: String,
    pub requested: Interval,
    pub events: usize,
    pub first: Option<Event>,
    pub latest: Option<Event>,
}
#[derive(Debug, Serialize)]
pub struct Gap {
    pub previous: Event,
    pub next: Event,
    pub elapsed_ms: i64,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub version: &'static str,
    pub dataset_identity: String,
    pub manifest: data::Manifest,
    pub requested: Interval,
    pub events: usize,
    pub first: Option<Event>,
    pub latest: Option<Event>,
    pub months: Vec<Month>,
    pub gap_threshold_ms: i64,
    pub gap_count: usize,
    pub gaps: Vec<Gap>,
    pub gap_details_truncated: bool,
    pub limitations: &'static str,
}
/// Bounded, read-only inspection of one verified normalized artifact, no provider access.
/// Both gap endpoints must be in scope; requested boundary silence is not a quote gap.
pub fn report(
    dataset: &Dataset,
    requested: Interval,
    gap_threshold_ms: i64,
    detail_limit: usize,
) -> Result<Report> {
    requested.validate()?;
    if gap_threshold_ms <= 0 || detail_limit > 1000 || dataset.ticks.len() > 2_000_000 {
        return Err(
            "coverage requires positive gap threshold, at most1000 details and2million events"
                .into(),
        );
    }
    dataset.verify()?;
    let start = dataset
        .ticks
        .partition_point(|t| t.timestamp_ms < requested.start_ms);
    let end = dataset
        .ticks
        .partition_point(|t| t.timestamp_ms < requested.end_exclusive_ms);
    let ticks = &dataset.ticks[start..end];
    let date = chrono::DateTime::from_timestamp_millis(requested.start_ms)
        .ok_or("invalid coverage date")?;
    let mut year = date.year();
    let mut month = date.month();
    let mut months = Vec::new();
    loop {
        if months.len() == 120 {
            return Err("coverage interval exceeds120 UTC months".into());
        }
        let month_start = Utc
            .with_ymd_and_hms(year, month, 1, 0, 0, 0)
            .single()
            .ok_or("unsupported month")?
            .timestamp_millis();
        let (next_year, next_month) = if month == 12 {
            (year.checked_add(1).ok_or("year overflow")?, 1)
        } else {
            (year, month + 1)
        };
        let month_end = Utc
            .with_ymd_and_hms(next_year, next_month, 1, 0, 0, 0)
            .single()
            .ok_or("unsupported next month")?
            .timestamp_millis();
        let window = Interval {
            start_ms: month_start.max(requested.start_ms),
            end_exclusive_ms: month_end.min(requested.end_exclusive_ms),
        };
        let a = ticks.partition_point(|t| t.timestamp_ms < window.start_ms);
        let b = ticks.partition_point(|t| t.timestamp_ms < window.end_exclusive_ms);
        let observed = &ticks[a..b];
        months.push(Month {
            utc_month: format!("{year:04}-{month:02}"),
            requested: window,
            events: observed.len(),
            first: observed.first().map(Event::from),
            latest: observed.last().map(Event::from),
        });
        if month_end >= requested.end_exclusive_ms {
            break;
        }
        (year, month) = (next_year, next_month);
    }
    let mut gaps = Vec::new();
    let mut gap_count = 0;
    for pair in ticks.windows(2) {
        let elapsed_ms = pair[1]
            .timestamp_ms
            .checked_sub(pair[0].timestamp_ms)
            .ok_or("gap duration overflow")?;
        if elapsed_ms > gap_threshold_ms {
            gap_count += 1;
            if gaps.len() < detail_limit {
                gaps.push(Gap {
                    previous: Event::from(&pair[0]),
                    next: Event::from(&pair[1]),
                    elapsed_ms,
                });
            }
        }
    }
    Ok(Report {
        version: "observed-event-coverage-v1",
        dataset_identity: data::dataset_identity(&dataset.manifest)?,
        manifest: dataset.manifest.clone(),
        requested,
        events: ticks.len(),
        first: ticks.first().map(Event::from),
        latest: ticks.last().map(Event::from),
        months,
        gap_threshold_ms,
        gap_count,
        gap_details_truncated: gaps.len() < gap_count,
        gaps,
        limitations: "Observed events only. Empty months and elapsed gaps do not prove missing source ranges without an approved trading calendar/source inventory. No acquisition, completeness, quality/profile approval or research eligibility is established. Boundary silence is not counted as an internal gap; only adjacent in-scope events are compared.",
    })
}
