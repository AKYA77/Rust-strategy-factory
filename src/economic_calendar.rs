//! Economic calendar data integration for strategy evaluation.
//! Provides high-impact news event awareness so strategies can avoid
//! trading during volatile news periods and so the validation pipeline
//! can annotate strategies with news-context metadata.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Impact level of an economic event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ImpactLevel {
    Low,
    Medium,
    High,
}

/// Type of economic event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EventType {
    NFP,        // Non-Farm Payrolls
    CPI,        // Consumer Price Index
    PPI,        // Producer Price Index
    GDP,        // Gross Domestic Product
    InterestRate, // Central bank interest rate decision
    RetailSales,
    Unemployment,
    PMI,        // Purchasing Managers Index
    Other,
}

/// A single economic calendar event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EconomicEvent {
    pub timestamp_ms: i64,
    pub instrument: String,      // e.g. "EURUSD", "USDCHF", "XAUUSD", or "ALL"
    pub event_type: EventType,
    pub impact: ImpactLevel,
    pub description: String,
    pub actual: Option<String>,   // Actual released value
    pub forecast: Option<String>, // Forecast/consensus value
    pub previous: Option<String>, // Previous period value
}

impl EconomicEvent {
    pub fn is_high_impact(&self) -> bool {
        self.impact == ImpactLevel::High
    }
}

/// Economic calendar: a collection of events indexed by time and instrument.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EconomicCalendar {
    events: BTreeMap<i64, Vec<EconomicEvent>>,  // timestamp_ms -> events at that time
    by_instrument: BTreeMap<String, Vec<i64>>, // instrument -> sorted timestamps
}

impl EconomicCalendar {
    pub fn new() -> Self {
        Self {
            events: BTreeMap::new(),
            by_instrument: BTreeMap::new(),
        }
    }

    pub fn add_event(&mut self, event: EconomicEvent) {
        self.events
            .entry(event.timestamp_ms)
            .or_default()
            .push(event.clone());

        self.by_instrument
            .entry(event.instrument.clone())
            .or_default()
            .push(event.timestamp_ms);
    }

    /// Get all events at a specific timestamp.
    pub fn events_at(&self, timestamp_ms: i64) -> Option<&Vec<EconomicEvent>> {
        self.events.get(&timestamp_ms)
    }

    /// Get all high-impact events for a specific instrument within a time window.
    pub fn high_impact_for_instrument(
        &self,
        instrument: &str,
        start_ms: i64,
        end_ms: i64,
    ) -> Vec<&EconomicEvent> {
        let mut result = Vec::new();
        if let Some(timestamps) = self.by_instrument.get(instrument) {
            for &ts in timestamps {
                if ts >= start_ms && ts <= end_ms {
                    if let Some(events) = self.events.get(&ts) {
                        for event in events {
                            if event.is_high_impact() {
                                result.push(event);
                            }
                        }
                    }
                }
            }
        }
        result
    }

    /// Check if there is a high-impact event for an instrument within `window_ms`
    /// before or after the given timestamp.
    pub fn high_impact_near(
        &self,
        instrument: &str,
        timestamp_ms: i64,
        window_ms: i64,
    ) -> Option<&EconomicEvent> {
        let start = timestamp_ms.saturating_sub(window_ms);
        let end = timestamp_ms + window_ms;
        let events = self.high_impact_for_instrument(instrument, start, end);
        events.into_iter().next()
    }

    /// Total number of events in the calendar.
    pub fn len(&self) -> usize {
        self.events.values().map(|v| v.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

impl Default for ImpactLevel {
    fn default() -> Self {
        ImpactLevel::Medium
    }
}

impl Default for EventType {
    fn default() -> Self {
        EventType::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_lookup_event() {
        let mut cal = EconomicCalendar::new();
        cal.add_event(EconomicEvent {
            timestamp_ms: 1735689600000,
            instrument: "EURUSD".to_string(),
            event_type: EventType::NFP,
            impact: ImpactLevel::High,
            description: "Non-Farm Payrolls".to_string(),
            actual: Some("250K".to_string()),
            forecast: Some("200K".to_string()),
            previous: Some("180K".to_string()),
        });

        assert!(!cal.is_empty());
        assert_eq!(cal.len(), 1);

        let events = cal.events_at(1735689600000);
        assert!(events.is_some());
        assert_eq!(events.unwrap().len(), 1);
        assert_eq!(events.unwrap()[0].event_type, EventType::NFP);
        assert!(events.unwrap()[0].is_high_impact());

        // High impact near should find it
        let near = cal.high_impact_near("EURUSD", 1735689600000, 3600000); // 1 hour window
        assert!(near.is_some());
        assert_eq!(near.unwrap().event_type, EventType::NFP);

        // Different instrument should not match
        let near_other = cal.high_impact_near("GBPUSD", 1735689600000, 3600000);
        assert!(near_other.is_none());

        // Outside window should not match
        let far = cal.high_impact_near("EURUSD", 1735689600000 + 7200000, 3600000); // 2 hours away, 1 hour window
        assert!(far.is_none());
    }

    #[test]
    fn test_multiple_events_same_timestamp() {
        let mut cal = EconomicCalendar::new();
        cal.add_event(EconomicEvent {
            timestamp_ms: 1735689600000,
            instrument: "EURUSD".to_string(),
            event_type: EventType::NFP,
            impact: ImpactLevel::High,
            description: "NFP".to_string(),
            actual: None,
            forecast: None,
            previous: None,
        });
        cal.add_event(EconomicEvent {
            timestamp_ms: 1735689600000,
            instrument: "EURUSD".to_string(),
            event_type: EventType::CPI,
            impact: ImpactLevel::Medium,
            description: "CPI".to_string(),
            actual: None,
            forecast: None,
            previous: None,
        });

        assert_eq!(cal.len(), 2);
        let events = cal.events_at(1735689600000).unwrap();
        assert_eq!(events.len(), 2);
    }
}
