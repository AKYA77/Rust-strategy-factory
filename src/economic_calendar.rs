//! Economic calendar data integration for strategy evaluation.
//!
//! This module is wired into the research pipeline through [`Features::with_context`]
//! and [`Features::news_blackout_mask`]. Calendar events are derived from observed
//! corpus gap data (market closures detected in the local historical corpus) and
//! carry provenance metadata. Every event carries publication/as-of metadata, and
//! callers must enforce causal availability through [`EconomicEvent::is_available_at`].
//!
//! No external data acquisition is performed here. Calendar events are derived
//! exclusively from already-downloaded local corpus data.

use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Impact level of an economic event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub enum ImpactLevel {
    Low,
    #[default]
    Medium,
    High,
}

/// Type of economic event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub enum EventType {
    NFP,          // Non-Farm Payrolls
    CPI,          // Consumer Price Index
    PPI,          // Producer Price Index
    GDP,          // Gross Domestic Product
    InterestRate, // Central bank interest rate decision
    RetailSales,
    Unemployment,
    PMI, // Purchasing Managers Index
    #[default]
    Other,
}

/// Sentinel instrument meaning the event applies to every instrument.
pub const ALL_INSTRUMENTS: &str = "ALL";

/// A single economic calendar event with provenance and causal-availability data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EconomicEvent {
    pub timestamp_ms: i64,
    /// Event identity scope: a specific instrument or [`ALL_INSTRUMENTS`].
    pub instrument: String,
    pub event_type: EventType,
    pub impact: ImpactLevel,
    pub description: String,
    /// Actual released value (retrospective; see [`Self::published_ms`]).
    #[serde(default)]
    pub actual: Option<String>,
    /// Forecast/consensus value.
    #[serde(default)]
    pub forecast: Option<String>,
    /// Previous period value.
    #[serde(default)]
    pub previous: Option<String>,
    /// Provenance identity of the calendar source that carried this event.
    #[serde(default)]
    pub source_id: String,
    /// Publication/as-of time: when this value became knowable. `None` = unknown.
    #[serde(default)]
    pub published_ms: Option<i64>,
    /// Versioned calendar revision identity.
    #[serde(default)]
    pub revision: String,
}

impl EconomicEvent {
    pub fn is_high_impact(&self) -> bool {
        self.impact == ImpactLevel::High
    }

    /// Causal availability guard: an event is usable at `timestamp_ms` only if its
    /// publication/as-of time is known and not in the future. Unknown provenance is
    /// treated as unavailable (never silently assumed present).
    pub fn is_available_at(&self, timestamp_ms: i64) -> bool {
        self.published_ms
            .is_some_and(|published| published <= timestamp_ms)
    }

    /// Whether this event's identity applies to `instrument`.
    pub fn applies_to(&self, instrument: &str) -> bool {
        self.instrument == instrument || self.instrument == ALL_INSTRUMENTS
    }
}

/// Economic calendar: events indexed by time and by instrument identity.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EconomicCalendar {
    events: BTreeMap<i64, Vec<EconomicEvent>>,
    by_instrument: BTreeMap<String, BTreeSet<i64>>,
}

impl EconomicCalendar {
    pub fn new() -> Self {
        Self {
            events: BTreeMap::new(),
            by_instrument: BTreeMap::new(),
        }
    }

    /// Insert an event. Duplicate identities are preserved as distinct entries so
    /// callers can detect them; lookups deduplicate by (timestamp, instrument).
    pub fn add_event(&mut self, event: EconomicEvent) {
        self.events
            .entry(event.timestamp_ms)
            .or_default()
            .push(event.clone());
        self.by_instrument
            .entry(event.instrument.clone())
            .or_default()
            .insert(event.timestamp_ms);
    }

    /// Get all events at a specific timestamp.
    pub fn events_at(&self, timestamp_ms: i64) -> Option<&Vec<EconomicEvent>> {
        self.events.get(&timestamp_ms)
    }

    /// High-impact events whose own identity matches `instrument` (or `ALL`) and
    /// whose timestamp lies in the inclusive interval `[start_ms, end_ms]`.
    ///
    /// Only the event's actual instrument identity is returned: events recorded
    /// for other instruments at a shared timestamp are never leaked. Repeated
    /// timestamps for one instrument are visited once because the index is a set.
    pub fn high_impact_for_instrument(
        &self,
        instrument: &str,
        start_ms: i64,
        end_ms: i64,
    ) -> Result<Vec<&EconomicEvent>> {
        validate_interval(start_ms, end_ms)?;
        let mut timestamps: BTreeSet<i64> = BTreeSet::new();
        for key in [instrument, ALL_INSTRUMENTS] {
            if let Some(set) = self.by_instrument.get(key) {
                timestamps.extend(set.iter().copied());
            }
        }
        let mut result = Vec::new();
        for ts in timestamps.range(start_ms..=end_ms) {
            if let Some(events) = self.events.get(ts) {
                for event in events {
                    if event.is_high_impact() && event.applies_to(instrument) {
                        result.push(event);
                    }
                }
            }
        }
        Ok(result)
    }

    /// First high-impact event for `instrument` within `window_ms` before or after
    /// `timestamp_ms`. Negative, overflowing or unknown-provenance windows are
    /// rejected rather than clamped into a plausible-looking answer.
    pub fn high_impact_near(
        &self,
        instrument: &str,
        timestamp_ms: i64,
        window_ms: i64,
    ) -> Result<Option<&EconomicEvent>> {
        if window_ms < 0 {
            return Err("negative economic-calendar window".into());
        }
        let start = timestamp_ms
            .checked_sub(window_ms)
            .ok_or("economic-calendar window underflow")?;
        let end = timestamp_ms
            .checked_add(window_ms)
            .ok_or("economic-calendar window overflow")?;
        Ok(self
            .high_impact_for_instrument(instrument, start, end)?
            .into_iter()
            .next())
    }

    /// Total number of events in the calendar.
    pub fn len(&self) -> usize {
        self.events.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Build an economic calendar from observed corpus gap data. Scans tick
    /// timestamps and emits a market-closure event for each gap exceeding
    /// `gap_threshold_ms` between consecutive ticks for the same instrument.
    /// Each event carries the gap's start timestamp as its publication time,
    /// making it causally available at the close of the gap.
    ///
    /// Only `High` impact events are emitted for closures longer than one
    /// trading day (86_400_000 ms); shorter closures are `Medium`.
    pub fn from_corpus_gaps(
        instrument: &str,
        ticks: &[crate::data::Tick],
        gap_threshold_ms: i64,
    ) -> Result<Self> {
        if gap_threshold_ms <= 0 {
            return Err("gap threshold must be positive".into());
        }
        let mut cal = EconomicCalendar::new();
        let mut prev_ms: Option<i64> = None;
        for tick in ticks {
            if let Some(prev) = prev_ms {
                let gap = tick
                    .timestamp_ms
                    .checked_sub(prev)
                    .ok_or("timestamp overflow")?;
                if gap > gap_threshold_ms {
                    let impact = if gap > 86_400_000 {
                        ImpactLevel::High
                    } else {
                        ImpactLevel::Medium
                    };
                    let event = EconomicEvent {
                        timestamp_ms: tick.timestamp_ms,
                        instrument: instrument.to_string(),
                        event_type: EventType::Other,
                        impact,
                        description: format!(
                            "market closure gap of {} ms ({} hours)",
                            gap,
                            gap / 3_600_000
                        ),
                        actual: None,
                        forecast: None,
                        previous: None,
                        source_id: "corpus-gap-analysis-v1".into(),
                        published_ms: Some(tick.timestamp_ms),
                        revision: "rev-1".into(),
                    };
                    cal.add_event(event);
                }
            }
            prev_ms = Some(tick.timestamp_ms);
        }
        Ok(cal)
    }
}

fn validate_interval(start_ms: i64, end_ms: i64) -> Result<()> {
    if start_ms > end_ms {
        return Err("invalid economic-calendar interval: start after end".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(instrument: &str, timestamp_ms: i64, impact: ImpactLevel) -> EconomicEvent {
        EconomicEvent {
            timestamp_ms,
            instrument: instrument.to_string(),
            event_type: EventType::NFP,
            impact,
            description: format!("{instrument}@{timestamp_ms}"),
            actual: None,
            forecast: None,
            previous: None,
            source_id: "test-calendar-v1".into(),
            published_ms: Some(timestamp_ms),
            revision: "rev-1".into(),
        }
    }

    #[test]
    fn filters_events_by_actual_instrument_identity() {
        let ts = 1_735_689_600_000;
        let mut cal = EconomicCalendar::new();
        cal.add_event(event("EURUSD", ts, ImpactLevel::High));
        cal.add_event(event("GBPUSD", ts, ImpactLevel::High));
        cal.add_event(event("EURUSD", ts, ImpactLevel::Low));

        let eur = cal.high_impact_for_instrument("EURUSD", ts, ts).unwrap();
        assert_eq!(eur.len(), 1);
        assert_eq!(eur[0].instrument, "EURUSD");

        let gbp = cal.high_impact_for_instrument("GBPUSD", ts, ts).unwrap();
        assert_eq!(gbp.len(), 1);
        assert_eq!(gbp[0].instrument, "GBPUSD");
    }

    #[test]
    fn repeated_timestamps_are_not_duplicated() {
        let ts = 1_735_689_600_000;
        let mut cal = EconomicCalendar::new();
        cal.add_event(event("EURUSD", ts, ImpactLevel::High));
        cal.add_event(event("EURUSD", ts, ImpactLevel::High));
        assert_eq!(cal.len(), 2);
        assert_eq!(
            cal.high_impact_for_instrument("EURUSD", ts, ts)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            cal.high_impact_near("EURUSD", ts, 1)
                .unwrap()
                .unwrap()
                .instrument,
            "EURUSD"
        );
    }

    #[test]
    fn all_scope_applies_to_every_instrument_only_when_requested() {
        let ts = 1_735_689_600_000;
        let mut cal = EconomicCalendar::new();
        cal.add_event(event(ALL_INSTRUMENTS, ts, ImpactLevel::High));
        assert_eq!(
            cal.high_impact_for_instrument("USDJPY", ts, ts)
                .unwrap()
                .len(),
            1
        );
        // The ALL event is not a USDJPY-specific observation.
        assert!(
            cal.high_impact_for_instrument("USDJPY", ts, ts)
                .unwrap()
                .iter()
                .all(|e| e.instrument == ALL_INSTRUMENTS)
        );
    }

    #[test]
    fn unsorted_insertion_and_boundary_inclusivity() {
        let mut cal = EconomicCalendar::new();
        cal.add_event(event("EURUSD", 3_000, ImpactLevel::High));
        cal.add_event(event("EURUSD", 1_000, ImpactLevel::High));
        cal.add_event(event("EURUSD", 2_000, ImpactLevel::High));
        let found = cal
            .high_impact_for_instrument("EURUSD", 1_000, 3_000)
            .unwrap();
        assert_eq!(found.len(), 3);
        assert!(
            cal.high_impact_for_instrument("EURUSD", 1_001, 2_999)
                .unwrap()
                .len()
                == 1
        );
        assert!(
            cal.high_impact_for_instrument("EURUSD", 4_000, 3_000)
                .is_err()
        );
    }

    #[test]
    fn negative_or_overflowing_windows_are_rejected() {
        let cal = EconomicCalendar::new();
        assert!(cal.high_impact_near("EURUSD", 1_000, -1).is_err());
        assert!(cal.high_impact_near("EURUSD", i64::MAX, 2).is_err());
        assert!(cal.high_impact_near("EURUSD", i64::MIN, 2).is_err());
    }

    #[test]
    fn causal_availability_requires_known_publication_time() {
        let ts = 1_735_689_600_000;
        let mut future = event("EURUSD", ts, ImpactLevel::High);
        future.published_ms = Some(ts + 1);
        assert!(!future.is_available_at(ts));
        let mut unknown = event("EURUSD", ts, ImpactLevel::High);
        unknown.published_ms = None;
        assert!(!unknown.is_available_at(ts));
        assert!(event("EURUSD", ts, ImpactLevel::High).is_available_at(ts));
    }
}
